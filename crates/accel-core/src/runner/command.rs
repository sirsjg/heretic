//! Turning a model profile into an actual command line.
//!
//! Kept free of process handling so the exact argv for every backend can be
//! asserted in tests — a wrong flag here is the difference between an agent that
//! works autonomously and one that hangs forever on a permission prompt.

use crate::config::{ModelProfile, RunnerKind};
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
    /// Whether stdout is Claude's `stream-json`, which we parse into events.
    pub emits_stream_json: bool,
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
                emits_stream_json: true,
            }
        }

        RunnerKind::Codex | RunnerKind::CodexOss { .. } => {
            let mut args = vec!["exec".to_string()];

            if let RunnerKind::CodexOss { base_url } = &profile.runner {
                args.push("--oss".into());
                if let Some(url) = base_url.as_deref().filter(|u| !u.is_empty()) {
                    // Codex reads provider settings from config; `-c` overrides one
                    // key for this invocation only.
                    args.push("-c".into());
                    args.push(format!("model_providers.oss.base_url=\"{url}\""));
                }
            }

            if let Some(model) = profile.model.as_deref().filter(|m| !m.is_empty()) {
                args.push("-m".into());
                args.push(model.into());
            }
            if profile.autonomous {
                // Full auto: edits and commands run without stopping to ask.
                args.push("--full-auto".into());
            }
            args.extend(profile.extra_args.iter().cloned());
            args.push(prompt.to_string());

            AgentCommand {
                program: "codex".into(),
                args,
                env,
                prompt_via_stdin: false,
                emits_stream_json: false,
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
                env.entry("ACCELERATE_MODEL".into()).or_insert(model);
            }

            AgentCommand {
                program: command.clone(),
                args: resolved,
                env,
                prompt_via_stdin: !prompt_used,
                emits_stream_json: false,
            }
        }
    }
}

impl AgentCommand {
    /// A shell-ish rendering for logs and the UI. Not for execution.
    pub fn display(&self) -> String {
        let mut parts = vec![self.program.clone()];
        for arg in &self.args {
            if arg.len() > 60 || arg.contains(char::is_whitespace) {
                parts.push("'<prompt>'".to_string());
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
        assert!(command.emits_stream_json);
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
    fn codex_open_models_target_a_local_server() {
        let mut p = profile(RunnerKind::CodexOss {
            base_url: Some("http://192.168.1.10:11434/v1".into()),
        });
        p.model = Some("qwen3-coder:30b".into());

        let command = build_command(&p, "build it");
        assert_eq!(command.program, "codex");
        assert_eq!(command.args[0], "exec");
        assert!(command.args.contains(&"--oss".to_string()));
        assert!(command.args.contains(
            &"model_providers.oss.base_url=\"http://192.168.1.10:11434/v1\"".to_string()
        ));
        let position = command.args.iter().position(|a| a == "-m").unwrap();
        assert_eq!(command.args[position + 1], "qwen3-coder:30b");
        assert_eq!(command.args.last().unwrap(), "build it");
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
    fn display_redacts_the_prompt() {
        let command = build_command(
            &profile(RunnerKind::ClaudeCode),
            "a very long prompt that should not be shown in full",
        );
        assert!(command.display().contains("'<prompt>'"));
    }
}
