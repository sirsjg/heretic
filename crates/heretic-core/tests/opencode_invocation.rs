//! Does the command Heretic builds for opencode actually run?
//!
//! Ignored by default: needs the `opencode` CLI, and for the host case a model
//! server it can reach. To run:
//!
//! ```text
//! cargo test --test opencode_invocation -- --ignored --nocapture
//! ```
//!
//! `OPENCODE_TEST_MODEL` overrides the `provider/model` used for the hosted
//! case. `OPENCODE_TEST_HOST` and `OPENCODE_TEST_HOST_MODEL` set the endpoint
//! and the bare model id for the host case, which is skipped without them.

use heretic_core::config::{ModelProfile, RunnerKind};
use heretic_core::runner::{build_command, run_agent, AgentEvent, CancelToken, Completion};
use std::collections::BTreeMap;
use std::path::Path;
use tokio::sync::mpsc;

fn profile(base_url: Option<String>, model: Option<String>) -> ModelProfile {
    ModelProfile {
        id: "opencode".into(),
        name: "opencode".into(),
        runner: RunnerKind::OpenCode { base_url },
        model,
        extra_args: Vec::new(),
        env: BTreeMap::new(),
        timeout_secs: Some(180),
        context_window: None,
        autonomous: true,
    }
}

async fn run(profile: &ModelProfile, prompt: &str) -> (Completion, Vec<AgentEvent>) {
    let command = build_command(profile, prompt);
    eprintln!("running: {}", command.display(prompt));

    let (tx, mut rx) = mpsc::channel(256);
    let collected = tokio::spawn(async move {
        let mut events = Vec::new();
        while let Some(event) = rx.recv().await {
            events.push(event);
        }
        events
    });

    let outcome = run_agent(
        command,
        prompt,
        Path::new("."),
        Some(std::time::Duration::from_secs(180)),
        CancelToken::new(),
        tx,
    )
    .await
    .expect("opencode should spawn — is it on PATH?");

    (outcome.completion, collected.await.unwrap())
}

/// Everything that has to hold for a run to be worth watching: the CLI accepted
/// the arguments, and its output parsed into events rather than arriving as raw
/// JSON lines.
fn assert_a_usable_run(completion: Completion, events: &[AgentEvent]) {
    let complaints: Vec<&AgentEvent> = events
        .iter()
        .filter(|event| match event {
            AgentEvent::Error { message } => message.contains("nknown argument"),
            AgentEvent::Raw { text } => text.contains("nknown argument"),
            _ => false,
        })
        .collect();
    assert!(
        complaints.is_empty(),
        "opencode rejected an argument: {complaints:?}"
    );

    assert_eq!(
        completion,
        Completion::Succeeded,
        "run did not complete: {events:#?}"
    );

    assert!(
        events.iter().any(|e| matches!(e, AgentEvent::Text { .. })),
        "expected the agent's message to be parsed: {events:#?}"
    );
    assert!(
        events.iter().any(|e| matches!(e, AgentEvent::Usage { .. })),
        "expected the run's tokens to be accounted for: {events:#?}"
    );
}

#[tokio::test]
#[ignore = "needs the opencode CLI and a configured provider"]
async fn opencode_runs_against_its_own_providers() {
    let model = std::env::var("OPENCODE_TEST_MODEL").ok();
    let (completion, events) = run(&profile(None, model), "Say hello in three words.").await;
    assert_a_usable_run(completion, &events);
}

/// The case Heretic generates a configuration for: opencode takes no endpoint
/// flag, so a host of our own only works if the provider it is handed parses.
#[tokio::test]
#[ignore = "needs the opencode CLI and a model server to point it at"]
async fn opencode_runs_against_a_host_of_our_own() {
    let (Ok(host), Ok(model)) = (
        std::env::var("OPENCODE_TEST_HOST"),
        std::env::var("OPENCODE_TEST_HOST_MODEL"),
    ) else {
        eprintln!("skipped: set OPENCODE_TEST_HOST and OPENCODE_TEST_HOST_MODEL");
        return;
    };

    let (completion, events) = run(
        &profile(Some(host), Some(model)),
        "Say hello in three words.",
    )
    .await;
    assert_a_usable_run(completion, &events);
}
