//! Turning a model profile into an actual command line.
//!
//! Kept free of process handling so the exact argv for every backend can be
//! asserted in tests — a wrong flag here is the difference between an agent that
//! works autonomously and one that hangs forever on a permission prompt.

use crate::config::{ModelProfile, RunnerKind};
use crate::runner::stream::OutputFormat;
use std::collections::BTreeMap;

/// A fully resolved command, ready to spawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentCommand {
    pub program: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    /// When true the prompt is written to the child's stdin instead of being
    /// passed as an argument. Only custom runners without a `{{prompt}}`
    /// placeholder take this path.
    pub prompt_via_stdin: bool,
    /// How this backend prints its progress, so the output can be parsed.
    pub output: OutputFormat,
}

/// The provider id Heretic declares when pointing Codex at a host of our
/// own. Deliberately not one of Codex's built-in ids, which it refuses to let
/// configuration override.
const OSS_PROVIDER: &str = "heretic-oss";

/// Ensure a host address ends at the OpenAI-compatible path.
fn openai_base(url: &str) -> String {
    crate::detect::openai_base(url)
}

/// Build the command for `profile`, carrying `prompt`.
pub fn build_command(profile: &ModelProfile, prompt: &str) -> AgentCommand {
    let mut env = profile.env.clone();

    match &profile.runner {
        RunnerKind::ClaudeCode => {
            let mut args = vec![
                "-p".to_string(),
                "--output-format".to_string(),
                "stream-json".to_string(),
                "--verbose".to_string(),
            ];
            if let Some(model) = profile.model.as_deref().filter(|m| !m.is_empty()) {
                args.push("--model".into());
                args.push(model.into());
            }
            if profile.autonomous {
                args.push("--dangerously-skip-permissions".into());
            }
            args.extend(profile.extra_args.iter().cloned());
            args.push(prompt.to_string());

            AgentCommand {
                program: "claude".into(),
                args,
                env,
                prompt_via_stdin: false,
                output: OutputFormat::ClaudeStreamJson,
            }
        }

        RunnerKind::Codex | RunnerKind::CodexOss { .. } => {
            // `codex exec` is already non-interactive, so approvals are never
            // asked for. What an unattended run needs is permission to write:
            // `--full-auto` belongs to the interactive command and is rejected
            // here.
            let mut args = vec!["exec".to_string(), "--json".to_string()];

            if profile.autonomous {
                args.push("--sandbox".into());
                args.push("workspace-write".into());
            }

            if let RunnerKind::CodexOss { base_url } = &profile.runner {
                match base_url.as_deref().filter(|url| !url.is_empty()) {
                    // A host of our own: Codex refuses to override its built-in
                    // provider ids, so declare a separate one pointing at it.
                    // Local servers speak the Responses API, which is the only
                    // wire format current Codex accepts from a custom provider.
                    Some(url) => {
                        args.push("-c".into());
                        args.push(format!(
                            "model_providers.{OSS_PROVIDER}.name=\"Heretic host\""
                        ));
                        args.push("-c".into());
                        args.push(format!(
                            "model_providers.{OSS_PROVIDER}.base_url=\"{}\"",
                            openai_base(url)
                        ));
                        args.push("-c".into());
                        args.push(format!(
                            "model_providers.{OSS_PROVIDER}.wire_api=\"responses\""
                        ));
                        args.push("-c".into());
                        args.push(format!("model_provider=\"{OSS_PROVIDER}\""));
                    }
                    // No host configured: use Codex's own local-model support.
                    None => {
                        args.push("--oss".into());
                        args.push("--local-provider".into());
                        args.push("ollama".into());
                    }
                }
            }

            if let Some(model) = profile.model.as_deref().filter(|m| !m.is_empty()) {
                args.push("-m".into());
                args.push(model.into());
            }

            // Codex only knows its own catalogue, so a local model gets fallback
            // metadata unless the real window is supplied.
            if let Some(window) = profile.context_window.filter(|w| *w > 0) {
                args.push("-c".into());
                args.push(format!("model_context_window={window}"));
            }

            args.extend(profile.extra_args.iter().cloned());
            args.push(prompt.to_string());

            AgentCommand {
                program: "codex".into(),
                args,
                env,
                prompt_via_stdin: false,
                output: OutputFormat::CodexJsonl,
            }
        }

        RunnerKind::Custom { command, args } => {
            let model = profile.model.clone().unwrap_or_default();
            let mut resolved = Vec::with_capacity(args.len());
            let mut prompt_used = false;

            for arg in args {
                if arg.contains("{{prompt}}") {
                    prompt_used = true;
                }
                resolved.push(
                    arg.replace("{{prompt}}", prompt)
                        .replace("{{model}}", &model),
                );
            }
            resolved.extend(profile.extra_args.iter().cloned());

            // Give custom runners the model in the environment too, since many
            // CLIs read it from there rather than from a flag.
            if !model.is_empty() {
                env.entry("HERETIC_MODEL".into()).or_insert(model);
            }

            AgentCommand {
                program: command.clone(),
                args: resolved,
                env,
                prompt_via_stdin: !prompt_used,
                output: OutputFormat::Plain,
            }
        }
    }
}

impl AgentCommand {
    /// A shell-ish rendering for logs and the UI. Not for execution.
    ///
    /// Only the prompt itself is elided. Configuration arguments are shown in
    /// full even when they contain spaces — hiding them makes a log useless for
    /// working out why a backend rejected an argument.
    pub fn display(&self, prompt: &str) -> String {
        let mut parts = vec![self.program.clone()];
        for arg in &self.args {
            if arg == prompt {
                parts.push("'<prompt>'".to_string());
            } else if arg.contains(char::is_whitespace) {
                parts.push(format!("'{arg}'"));
            } else {
                parts.push(arg.clone());
            }
        }
        parts.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(runner: RunnerKind) -> ModelProfile {
        ModelProfile {
            id: "p".into(),
            name: "Profile".into(),
            runner,
            model: None,
            extra_args: Vec::new(),
            env: BTreeMap::new(),
            timeout_secs: None,
            context_window: None,
            autonomous: true,
        }
    }

    #[test]
    fn claude_runs_headless_and_streams_json() {
        let command = build_command(&profile(RunnerKind::ClaudeCode), "do the thing");
        assert_eq!(command.program, "claude");
        assert_eq!(
            command.args,
            vec![
                "-p",
                "--output-format",
                "stream-json",
                "--verbose",
                "--dangerously-skip-permissions",
                "do the thing"
            ]
        );
        assert_eq!(command.output, OutputFormat::ClaudeStreamJson);
        assert!(!command.prompt_via_stdin);
    }

    #[test]
    fn a_non_autonomous_profile_keeps_its_permission_prompts() {
        let mut p = profile(RunnerKind::ClaudeCode);
        p.autonomous = false;
        let command = build_command(&p, "hi");
        assert!(!command
            .args
            .iter()
            .any(|a| a == "--dangerously-skip-permissions"));
    }

    #[test]
    fn claude_passes_the_model_through() {
        let mut p = profile(RunnerKind::ClaudeCode);
        p.model = Some("opus".into());
        let command = build_command(&p, "hi");
        let position = command.args.iter().position(|a| a == "--model").unwrap();
        assert_eq!(command.args[position + 1], "opus");
    }

    #[test]
    fn codex_exec_asks_for_a_writable_workspace_not_full_auto() {
        // `--full-auto` belongs to the interactive command; `codex exec`
        // rejects it outright, which is how this was found.
        let command = build_command(&profile(RunnerKind::Codex), "build it");
        assert_eq!(command.program, "codex");
        assert_eq!(command.args[0], "exec");
        assert!(!command.args.iter().any(|a| a == "--full-auto"));

        let sandbox = command.args.iter().position(|a| a == "--sandbox").unwrap();
        assert_eq!(command.args[sandbox + 1], "workspace-write");
        assert!(command.args.contains(&"--json".to_string()));
        assert_eq!(command.output, OutputFormat::CodexJsonl);
    }

    #[test]
    fn a_supervised_codex_profile_gets_no_write_sandbox() {
        let mut p = profile(RunnerKind::Codex);
        p.autonomous = false;
        let command = build_command(&p, "hi");
        assert!(!command.args.iter().any(|a| a == "--sandbox"));
    }

    #[test]
    fn a_local_model_with_no_host_uses_codex_own_ollama_support() {
        let mut p = profile(RunnerKind::CodexOss { base_url: None });
        p.model = Some("qwen3-coder:30b".into());

        let command = build_command(&p, "build it");
        assert!(command.args.contains(&"--oss".to_string()));
        let provider = command
            .args
            .iter()
            .position(|a| a == "--local-provider")
            .expect("should name the local provider");
        assert_eq!(command.args[provider + 1], "ollama");
    }

    #[test]
    fn a_model_on_another_machine_is_reached_through_a_declared_provider() {
        // Codex refuses to let configuration override its built-in provider
        // ids, so a host of our own has to be declared separately.
        let mut p = profile(RunnerKind::CodexOss {
            base_url: Some("http://spark.local:11434".into()),
        });
        p.model = Some("qwen3-coder:480b".into());

        let command = build_command(&p, "build it");
        let joined = command.args.join(" ");

        assert!(!command.args.contains(&"--oss".to_string()));
        assert!(
            joined.contains(
                "model_providers.heretic-oss.base_url=\"http://spark.local:11434/v1\""
            ),
            "{joined}"
        );
        // Current Codex accepts only the responses wire format from a custom
        // provider; "chat" is rejected at config load.
        assert!(joined.contains("model_providers.heretic-oss.wire_api=\"responses\""));
        assert!(joined.contains("model_provider=\"heretic-oss\""));

        let model = command.args.iter().position(|a| a == "-m").unwrap();
        assert_eq!(command.args[model + 1], "qwen3-coder:480b");
        assert_eq!(command.args.last().unwrap(), "build it");
    }

    #[test]
    fn a_known_context_window_is_declared_rather_than_guessed() {
        let mut p = profile(RunnerKind::CodexOss {
            base_url: Some("http://spark.local:11434".into()),
        });
        p.model = Some("qwen3.8:latest".into());
        p.context_window = Some(262_144);

        let joined = build_command(&p, "hi").args.join(" ");
        assert!(joined.contains("model_context_window=262144"), "{joined}");
    }

    #[test]
    fn an_unknown_context_window_is_left_to_the_backend() {
        let p = profile(RunnerKind::Codex);
        let joined = build_command(&p, "hi").args.join(" ");
        assert!(!joined.contains("model_context_window"));
    }

    #[test]
    fn a_host_pasted_with_v1_does_not_end_up_doubled() {
        let p = profile(RunnerKind::CodexOss {
            base_url: Some("http://spark.local:11434/v1".into()),
        });
        let joined = build_command(&p, "hi").args.join(" ");
        assert!(joined.contains("http://spark.local:11434/v1\""), "{joined}");
        assert!(!joined.contains("/v1/v1"));
    }

    #[test]
    fn custom_runners_substitute_the_prompt_placeholder() {
        let mut p = profile(RunnerKind::Custom {
            command: "aider".into(),
            args: vec![
                "--model".into(),
                "{{model}}".into(),
                "--message".into(),
                "{{prompt}}".into(),
            ],
        });
        p.model = Some("ollama/qwen".into());

        let command = build_command(&p, "fix the bug");
        assert_eq!(command.program, "aider");
        assert_eq!(
            command.args,
            vec!["--model", "ollama/qwen", "--message", "fix the bug"]
        );
        assert!(!command.prompt_via_stdin);
    }

    #[test]
    fn custom_runners_without_a_placeholder_get_the_prompt_on_stdin() {
        let p = profile(RunnerKind::Custom {
            command: "my-agent".into(),
            args: vec!["--headless".into()],
        });
        let command = build_command(&p, "fix the bug");
        assert!(command.prompt_via_stdin);
        assert_eq!(command.args, vec!["--headless"]);
    }

    #[test]
    fn display_hides_the_prompt_but_keeps_the_flags() {
        let prompt = "a very long prompt that should not be shown in full";
        let command = build_command(&profile(RunnerKind::ClaudeCode), prompt);
        let shown = command.display(prompt);

        assert!(shown.contains("'<prompt>'"));
        assert!(!shown.contains("should not be shown"));
        // The flags are the whole point of a command log.
        assert!(shown.contains("--output-format stream-json"));
    }

    #[test]
    fn display_keeps_configuration_arguments_legible() {
        let p = profile(RunnerKind::CodexOss {
            base_url: Some("http://spark.local:11434".into()),
        });
        let shown = build_command(&p, "hi").display("hi");
        // Exactly what a user needs to see when a backend rejects a flag.
        assert!(
            shown.contains("model_provider=\"heretic-oss\""),
            "{shown}"
        );
    }
}
