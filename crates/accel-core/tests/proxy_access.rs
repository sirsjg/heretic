//! Reaching a Flux server that sits behind an identity proxy.
//!
//! These talk to a live server, so they are ignored by default. To run them,
//! start Flux and a proxy in front of it, then:
//!
//! ```text
//! FLUX_PROXY_URL=http://localhost:8080 \
//! FLUX_API_KEY=flx_… \
//! cargo test --test proxy_access -- --ignored
//! ```
//!
//! The proxy is expected to accept a Cloudflare-Access-style service token
//! (`test.access` / `shh`) or a `demo_session=letmein` cookie, and to redirect
//! everything else to a sign-in page.

use accel_core::flux::{FluxClient, FluxConfig};
use std::collections::BTreeMap;

fn base_url() -> String {
    std::env::var("FLUX_PROXY_URL").expect("set FLUX_PROXY_URL to the proxy address")
}

fn api_key() -> Option<String> {
    std::env::var("FLUX_API_KEY").ok()
}

fn service_token() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("CF-Access-Client-Id".to_string(), "test.access".to_string()),
        ("CF-Access-Client-Secret".to_string(), "shh".to_string()),
    ])
}

#[tokio::test]
#[ignore = "needs a live Flux server behind a proxy"]
async fn without_a_proxy_credential_the_sign_in_page_is_reported_clearly() {
    let client = FluxClient::new(FluxConfig {
        base_url: base_url(),
        api_key: api_key(),
        ..FluxConfig::default()
    })
    .unwrap();

    let error = client
        .list_projects()
        .await
        .expect_err("the proxy should have blocked this");

    assert!(
        error.is_proxy_challenge(),
        "expected a proxy challenge, got: {error}"
    );
    // The message has to send the user to the right place — this failure is not
    // a missing Flux API key.
    assert!(error.to_string().contains("sign-in page"), "{error}");
}

#[tokio::test]
#[ignore = "needs a live Flux server behind a proxy"]
async fn a_service_token_gets_through_to_flux() {
    let client = FluxClient::new(FluxConfig {
        base_url: base_url(),
        api_key: api_key(),
        headers: service_token(),
        ..FluxConfig::default()
    })
    .unwrap();

    let projects = client
        .list_projects()
        .await
        .expect("the service token should have been accepted");
    assert!(!projects.is_empty(), "expected the seeded board");
}

#[tokio::test]
#[ignore = "needs a live Flux server behind a proxy"]
async fn a_session_cookie_gets_through_to_flux() {
    let client = FluxClient::new(FluxConfig {
        base_url: base_url(),
        api_key: api_key(),
        cookie: Some("demo_session=letmein".into()),
        ..FluxConfig::default()
    })
    .unwrap();

    client
        .list_projects()
        .await
        .expect("the session cookie should have been accepted");
}

#[tokio::test]
#[ignore = "needs a live Flux server behind a proxy"]
async fn a_wrong_service_token_is_still_reported_as_a_proxy_problem() {
    let mut headers = service_token();
    headers.insert("CF-Access-Client-Secret".into(), "wrong".into());

    let client = FluxClient::new(FluxConfig {
        base_url: base_url(),
        api_key: api_key(),
        headers,
        ..FluxConfig::default()
    })
    .unwrap();

    let error = client.list_projects().await.expect_err("should be refused");
    assert!(error.is_proxy_challenge(), "got: {error}");
}

#[tokio::test]
#[ignore = "needs a live Flux server behind a proxy"]
async fn writes_reach_flux_through_the_proxy() {
    let client = FluxClient::new(FluxConfig {
        base_url: base_url(),
        api_key: api_key(),
        headers: service_token(),
        ..FluxConfig::default()
    })
    .unwrap();

    // A comment is a POST, which exercises the proxy on a non-GET method and
    // proves Flux's own key survived alongside the proxy credential.
    let projects = client.list_projects().await.unwrap();
    let project = projects.first().expect("seeded project");
    let tasks = client.list_tasks(&project.id).await.unwrap();
    let task = tasks.first().expect("seeded task");

    client
        .add_comment(
            &task.id,
            "Reached Flux through the proxy.",
            Some("Accelerate"),
        )
        .await
        .expect("the write should have gone through");
}

#[tokio::test]
#[ignore = "needs a live Flux server behind a proxy"]
async fn the_event_stream_survives_the_proxy() {
    use accel_core::flux::{FluxEvent, FluxWatcher};

    let watcher = FluxWatcher::start(FluxConfig {
        base_url: base_url(),
        api_key: api_key(),
        headers: service_token(),
        ..FluxConfig::default()
    });
    let mut events = watcher.subscribe();

    // Flux greets a new listener with `connected`, which is enough to prove the
    // stream was authenticated rather than handed a login page.
    let first = tokio::time::timeout(std::time::Duration::from_secs(15), events.recv())
        .await
        .expect("timed out waiting for the event stream")
        .expect("stream closed");

    assert!(
        matches!(first, FluxEvent::Connected),
        "expected a connected event, got {first:?}"
    );
}
