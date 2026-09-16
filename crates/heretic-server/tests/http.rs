//! The server end to end: bind to a free port, then talk to it like a phone.

use heretic_core::config::RemoteConfig;
use heretic_core::store::SettingsStore;
use heretic_core::{Engine, Service, Settings};
use std::sync::Arc;

const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

async fn serve() -> (heretic_server::Running, String) {
    let path = std::env::temp_dir().join(format!(
        "heretic-server-test-{}-{:?}.json",
        std::process::id(),
        std::thread::current().id()
    ));
    let service = Arc::new(Service::new(
        Arc::new(Engine::new(Settings::default())),
        SettingsStore::new(path),
    ));
    let config = RemoteConfig {
        enabled: true,
        bind: "127.0.0.1".into(),
        port: 0,
        token: Some(TOKEN.into()),
        public_url: None,
    };
    let running = heretic_server::start(service, &config).await.unwrap();
    let base = format!("http://{}", running.addr);
    (running, base)
}

#[tokio::test]
async fn the_api_needs_the_token_and_the_bundle_does_not() {
    let (running, base) = serve().await;
    let client = reqwest::Client::new();

    // The page itself is not a secret.
    let page = client.get(&base).send().await.unwrap();
    assert!(page.status().is_success());
    assert!(page
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with("text/html"));

    // A route the interface handles gets the page too; a missing file does not.
    assert!(client
        .get(format!("{base}/runs/abc"))
        .send()
        .await
        .unwrap()
        .status()
        .is_success());
    assert_eq!(
        client
            .get(format!("{base}/assets/missing.js"))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );

    // No token, wrong token, token in the wrong place: all refused.
    assert_eq!(
        client
            .get(format!("{base}/api/status"))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        client
            .get(format!("{base}/api/status"))
            .bearer_auth("not-it")
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        client
            .get(format!("{base}/api/status?token={TOKEN}"))
            .send()
            .await
            .unwrap()
            .status(),
        401,
        "the query parameter is for the socket only"
    );

    let status: serde_json::Value = client
        .get(format!("{base}/api/status"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(status["line"], "Heretic is idle");
    assert_eq!(status["active"], 0);

    let ticker = client
        .get(format!("{base}/api/ticker"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert_eq!(ticker, "Heretic is idle");

    running.stop().await;
}

#[tokio::test]
async fn calls_mirror_the_desktop_commands() {
    let (running, base) = serve().await;
    let client = reqwest::Client::new();
    let call = |command: &str| {
        client
            .post(format!("{base}/api/call/{command}"))
            .bearer_auth(TOKEN)
    };

    let settings: serde_json::Value = call("get_settings")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(settings["remote"]["port"], 7411);

    let runs: serde_json::Value = call("list_runs")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(runs, serde_json::json!([]));

    // Either spelling of an argument works.
    for body in [r#"{"runId":"nope"}"#, r#"{"run_id":"nope"}"#] {
        let stopped: serde_json::Value = call("stop_run")
            .header("content-type", "application/json")
            .body(body)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(stopped, serde_json::json!(false));
    }

    // A missing argument is a 400 with a readable message, not a 500.
    let response = call("stop_run").body("{}").send().await.unwrap();
    assert_eq!(response.status(), 400);
    let error: serde_json::Value = response.json().await.unwrap();
    assert!(error["error"].as_str().unwrap().contains("arguments"));

    let response = call("no_such_thing").send().await.unwrap();
    assert_eq!(response.status(), 404);

    // Signing in is the desktop's job.
    let response = call("flux_sign_in").send().await.unwrap();
    assert_eq!(response.status(), 400);

    let platform: serde_json::Value = call("platform").send().await.unwrap().json().await.unwrap();
    assert_eq!(platform, "remote");

    running.stop().await;
}

#[tokio::test]
async fn a_stopped_server_stops_answering() {
    let (running, base) = serve().await;
    running.stop().await;
    assert!(reqwest::get(&base).await.is_err());
}
