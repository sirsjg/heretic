//! Turning a model profile into an actual command line.
//!
//! Kept free of process handling so the exact argv for every backend can be
//! asserted in tests — a wrong flag here is the difference between an agent that
//! works autonomously and one that hangs forever on a permission prompt.

use crate::config::{ModelProfile, ReasoningEffort, RunnerKind};
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

/// The provider id Heretic declares in the opencode configuration it generates.
///
/// opencode addresses a model as `provider/model`, splitting at the first
/// slash, so a model id containing slashes survives the prefix intact.
const OPENCODE_PROVIDER: &str = "heretic-host";

/// Ensure a host address ends at the OpenAI-compatible path.
fn openai_base(url: &str) -> String {
    crate::detect::openai_base(url)
}

/// An opencode configuration declaring one provider pointing at `base_url`.
///
/// opencode reads providers from a configuration file rather than from flags,
/// so this is handed over in `OPENCODE_CONFIG_CONTENT` — which replaces the
/// file rather than merging with it, leaving the user's own configuration
/// untouched on disk.
fn opencode_host_config(
    base_url: &str,
    model: Option<&str>,
    context_window: Option<u64>,
    reasoning: Option<ReasoningEffort>,
) -> String {
    let mut entry = serde_json::Map::new();
    if let Some(window) = context_window.filter(|w| *w > 0) {
        // opencode wants an output ceiling alongside the window and reserves
        // that much of it for the reply; without both it takes neither.
        entry.insert(
            "limit".into(),
            serde_json::json!({
                "context": window,
                "output": (window / 4).clamp(1_024, 32_768),
            }),
        );
    }
    if let Some(effort) = reasoning {
        // Passed through to the provider; models that do not reason ignore it.
        entry.insert(
            "options".into(),
            serde_json::json!({ "reasoningEffort": effort.as_str() }),
        );
    }

    let mut models = serde_json::Map::new();
    if let Some(model) = model {
        models.insert(model.to_string(), serde_json::Value::Object(entry));
    }

    serde_json::json!({
        "provider": {
            OPENCODE_PROVIDER: {
                "npm": "@ai-sdk/openai-compatible",
                "name": "Heretic host",
                "options": {
                    "baseURL": openai_base(base_url),
                    // Local servers ignore this, but the OpenAI-compatible
                    // provider will not start without something here.
                    "apiKey": "heretic",
                },
                "models": models,
            }
        }
    })
    .to_string()
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
            // Claude Code takes its thinking budget from the environment, not a
            // flag. The profile's own env wins over the derived value.
            if let Some(effort) = profile.reasoning_effort {
                env.entry("MAX_THINKING_TOKENS".into())
                    .or_insert_with(|| effort.thinking_tokens().to_string());
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
                // Codex tries to refresh its own model catalogue from
                // `{base_url}/models` in its internal schema, which no
                // OpenAI-compatible server answers; the failure is harmless but
                // logged at ERROR on every run. Silence just that module,
                // unless the profile sets its own filter.
                env.entry("RUST_LOG".into())
                    .or_insert_with(|| "error,codex_models_manager=off".into());

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

            if let Some(effort) = profile.reasoning_effort {
                args.push("-c".into());
                args.push(format!("model_reasoning_effort=\"{}\"", effort.as_str()));
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

        RunnerKind::OpenCode { base_url } => {
            // `opencode run` is the non-interactive command; `--auto` is what
            // stops it waiting on a permission prompt no one is there to answer.
            let mut args = vec![
                "run".to_string(),
                "--format".to_string(),
                "json".to_string(),
            ];

            if profile.autonomous {
                args.push("--auto".into());
            }

            let model = profile.model.as_deref().filter(|m| !m.is_empty());

            match base_url.as_deref().filter(|url| !url.is_empty()) {
                // A host of our own: declared as a provider in a generated
                // configuration, since opencode takes no endpoint flag.
                Some(url) => {
                    env.entry("OPENCODE_CONFIG_CONTENT".into())
                        .or_insert_with(|| {
                            opencode_host_config(
                                url,
                                model,
                                profile.context_window,
                                profile.reasoning_effort,
                            )
                        });
                    if let Some(model) = model {
                        args.push("-m".into());
                        args.push(format!("{OPENCODE_PROVIDER}/{model}"));
                    }
                }
                // No host: the model is addressed through the user's own
                // opencode providers, so it already carries its prefix.
                None => {
                    if let Some(model) = model {
                        args.push("-m".into());
                        args.push(model.into());
                    }
                }
            }

            args.extend(profile.extra_args.iter().cloned());
            args.push(prompt.to_string());

            AgentCommand {
                program: "opencode".into(),
                args,
                env,
                prompt_via_stdin: false,
                output: OutputFormat::OpenCodeJson,
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
            if let Some(effort) = profile.reasoning_effort {
                env.entry("HERETIC_REASONING_EFFORT".into())
                    .or_insert_with(|| effort.as_str().to_string());
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
            reasoning_effort: None,
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
    fn claude_reasoning_effort_becomes_a_thinking_budget() {
        let mut p = profile(RunnerKind::ClaudeCode);
        p.reasoning_effort = Some(ReasoningEffort::High);
        let command = build_command(&p, "hi");
        assert_eq!(
            command.env.get("MAX_THINKING_TOKENS").map(String::as_str),
            Some("31999")
        );
    }

    #[test]
    fn a_profile_supplied_thinking_budget_wins() {
        let mut p = profile(RunnerKind::ClaudeCode);
        p.reasoning_effort = Some(ReasoningEffort::Low);
        p.env
            .insert("MAX_THINKING_TOKENS".into(), "12345".into());
        let command = build_command(&p, "hi");
        assert_eq!(
            command.env.get("MAX_THINKING_TOKENS").map(String::as_str),
            Some("12345")
        );
    }

    #[test]
    fn no_reasoning_effort_leaves_the_backend_default_alone() {
        let claude = build_command(&profile(RunnerKind::ClaudeCode), "hi");
        assert!(!claude.env.contains_key("MAX_THINKING_TOKENS"));

        let codex = build_command(&profile(RunnerKind::Codex), "hi");
        assert!(!codex.args.join(" ").contains("model_reasoning_effort"));
    }

    #[test]
    fn codex_reasoning_effort_is_a_config_override() {
        let mut p = profile(RunnerKind::Codex);
        p.reasoning_effort = Some(ReasoningEffort::Medium);
        let joined = build_command(&p, "hi").args.join(" ");
        assert!(
            joined.contains("model_reasoning_effort=\"medium\""),
            "{joined}"
        );
    }

    #[test]
    fn local_codex_reasoning_effort_is_passed_the_same_way() {
        let mut p = profile(RunnerKind::CodexOss { base_url: None });
        p.reasoning_effort = Some(ReasoningEffort::Low);
        let joined = build_command(&p, "hi").args.join(" ");
        assert!(joined.contains("model_reasoning_effort=\"low\""), "{joined}");
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
            joined.contains("model_providers.heretic-oss.base_url=\"http://spark.local:11434/v1\""),
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
    fn local_codex_runs_silence_the_model_catalogue_noise() {
        // Codex's catalogue refresh always fails against an OpenAI-compatible
        // host and logs it at ERROR; only that module is turned off.
        let with_host = build_command(
            &profile(RunnerKind::CodexOss {
                base_url: Some("http://spark.local:11434".into()),
            }),
            "hi",
        );
        assert_eq!(
            with_host.env.get("RUST_LOG").map(String::as_str),
            Some("error,codex_models_manager=off")
        );

        let oss = build_command(&profile(RunnerKind::CodexOss { base_url: None }), "hi");
        assert!(oss.env.contains_key("RUST_LOG"));

        // A cloud Codex profile keeps its logging untouched.
        let cloud = build_command(&profile(RunnerKind::Codex), "hi");
        assert!(!cloud.env.contains_key("RUST_LOG"));
    }

    #[test]
    fn a_profile_supplied_log_filter_wins() {
        let mut p = profile(RunnerKind::CodexOss { base_url: None });
        p.env.insert("RUST_LOG".into(), "debug".into());
        let command = build_command(&p, "hi");
        assert_eq!(
            command.env.get("RUST_LOG").map(String::as_str),
            Some("debug")
        );
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

    // --- opencode -------------------------------------------------------------

    #[test]
    fn opencode_runs_non_interactively_and_streams_json() {
        let mut p = profile(RunnerKind::OpenCode { base_url: None });
        p.model = Some("anthropic/claude-opus-5".into());

        let command = build_command(&p, "build it");
        assert_eq!(command.program, "opencode");
        assert_eq!(
            command.args,
            vec![
                "run",
                "--format",
                "json",
                "--auto",
                "-m",
                "anthropic/claude-opus-5",
                "build it"
            ]
        );
        assert_eq!(command.output, OutputFormat::OpenCodeJson);
        assert!(!command.prompt_via_stdin);
        // Without a host of our own, the user's opencode configuration stands.
        assert!(!command.env.contains_key("OPENCODE_CONFIG_CONTENT"));
    }

    #[test]
    fn a_supervised_opencode_profile_still_asks_permission() {
        let mut p = profile(RunnerKind::OpenCode { base_url: None });
        p.autonomous = false;
        let command = build_command(&p, "hi");
        assert!(!command.args.iter().any(|a| a == "--auto"));
    }

    #[test]
    fn an_opencode_host_is_declared_in_a_generated_configuration() {
        // opencode takes no endpoint flag, so a host of our own has to arrive
        // as a provider in the configuration it reads.
        let mut p = profile(RunnerKind::OpenCode {
            base_url: Some("http://spark.local:11434".into()),
        });
        p.model = Some("qwen3-coder:30b".into());
        p.context_window = Some(262_144);

        let command = build_command(&p, "build it");

        let model = command.args.iter().position(|a| a == "-m").unwrap();
        assert_eq!(command.args[model + 1], "heretic-host/qwen3-coder:30b");
        assert_eq!(command.args.last().unwrap(), "build it");

        let config: serde_json::Value =
            serde_json::from_str(command.env.get("OPENCODE_CONFIG_CONTENT").unwrap()).unwrap();
        let provider = &config["provider"]["heretic-host"];
        assert_eq!(provider["npm"], "@ai-sdk/openai-compatible");
        assert_eq!(
            provider["options"]["baseURL"],
            "http://spark.local:11434/v1"
        );
        // The real window, rather than whatever opencode would assume, plus the
        // output ceiling it insists on alongside it.
        let limit = &provider["models"]["qwen3-coder:30b"]["limit"];
        assert_eq!(limit["context"], 262_144);
        assert_eq!(limit["output"], 32_768);
    }

    #[test]
    fn an_opencode_model_id_containing_slashes_keeps_its_prefix() {
        // opencode splits `provider/model` at the first slash, so a vLLM-style
        // id survives being prefixed.
        let mut p = profile(RunnerKind::OpenCode {
            base_url: Some("http://spark.local:8000/v1".into()),
        });
        p.model = Some("Qwen/Qwen3-Coder-30B".into());

        let command = build_command(&p, "hi");
        let model = command.args.iter().position(|a| a == "-m").unwrap();
        assert_eq!(command.args[model + 1], "heretic-host/Qwen/Qwen3-Coder-30B");

        let config: serde_json::Value =
            serde_json::from_str(command.env.get("OPENCODE_CONFIG_CONTENT").unwrap()).unwrap();
        // Registered under the id the server knows, not the prefixed one.
        assert!(config["provider"]["heretic-host"]["models"]["Qwen/Qwen3-Coder-30B"].is_object());
        assert!(!config["provider"]["heretic-host"]["options"]["baseURL"]
            .as_str()
            .unwrap()
            .contains("/v1/v1"));
    }

    #[test]
    fn an_unknown_context_window_leaves_opencode_to_decide() {
        let mut p = profile(RunnerKind::OpenCode {
            base_url: Some("http://spark.local:11434".into()),
        });
        p.model = Some("qwen3-coder:30b".into());

        let command = build_command(&p, "hi");
        let config: serde_json::Value =
            serde_json::from_str(command.env.get("OPENCODE_CONFIG_CONTENT").unwrap()).unwrap();
        assert!(config["provider"]["heretic-host"]["models"]["qwen3-coder:30b"]["limit"].is_null());
    }

    #[test]
    fn opencode_reasoning_effort_lands_in_the_generated_model_options() {
        let mut p = profile(RunnerKind::OpenCode {
            base_url: Some("http://spark.local:11434".into()),
        });
        p.model = Some("qwen3-coder:30b".into());
        p.reasoning_effort = Some(ReasoningEffort::High);

        let command = build_command(&p, "hi");
        let config: serde_json::Value =
            serde_json::from_str(command.env.get("OPENCODE_CONFIG_CONTENT").unwrap()).unwrap();
        assert_eq!(
            config["provider"]["heretic-host"]["models"]["qwen3-coder:30b"]["options"]
                ["reasoningEffort"],
            "high"
        );
    }

    #[test]
    fn opencode_without_a_host_leaves_reasoning_to_its_own_configuration() {
        // Without a host of our own there is no generated configuration to put
        // the option in, and replacing the user's file just for this would
        // throw their providers away.
        let mut p = profile(RunnerKind::OpenCode { base_url: None });
        p.reasoning_effort = Some(ReasoningEffort::High);
        let command = build_command(&p, "hi");
        assert!(!command.env.contains_key("OPENCODE_CONFIG_CONTENT"));
    }

    #[test]
    fn custom_runners_learn_the_effort_from_the_environment() {
        let mut p = profile(RunnerKind::Custom {
            command: "my-agent".into(),
            args: vec![],
        });
        p.reasoning_effort = Some(ReasoningEffort::Medium);
        let command = build_command(&p, "hi");
        assert_eq!(
            command
                .env
                .get("HERETIC_REASONING_EFFORT")
                .map(String::as_str),
            Some("medium")
        );
    }

    #[test]
    fn a_profile_supplied_opencode_configuration_wins() {
        let mut p = profile(RunnerKind::OpenCode {
            base_url: Some("http://spark.local:11434".into()),
        });
        p.env
            .insert("OPENCODE_CONFIG_CONTENT".into(), "{\"provider\":{}}".into());
        let command = build_command(&p, "hi");
        assert_eq!(
            command
                .env
                .get("OPENCODE_CONFIG_CONTENT")
                .map(String::as_str),
            Some("{\"provider\":{}}")
        );
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
        assert!(shown.contains("model_provider=\"heretic-oss\""), "{shown}");
    }
}
