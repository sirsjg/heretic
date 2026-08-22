//! Finding out what is actually available to run.
//!
//! Two kinds of thing get discovered: agent CLIs installed on this machine
//! (Claude Code, Codex), and model servers that hold weights — Ollama on
//! localhost, or a box on the network such as a DGX Spark.
//!
//! Parsing is kept separate from I/O so the response shapes can be tested
//! without a server.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// How long to wait on a machine that may be asleep or off the network.
const PROBE_TIMEOUT: Duration = Duration::from_secs(6);

// --- Agent CLIs --------------------------------------------------------------

/// Whether an agent CLI is installed, and which version.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliStatus {
    /// The executable looked for, e.g. `claude`.
    pub program: String,
    /// What the user knows it as.
    pub label: String,
    pub found: bool,
    /// First line of `--version`, when it answered.
    pub version: Option<String>,
    /// Why it could not be used, phrased for the user.
    pub problem: Option<String>,
}

/// The agent CLIs Accelerate knows how to drive.
pub const KNOWN_CLIS: [(&str, &str); 2] = [("claude", "Claude Code"), ("codex", "Codex")];

/// Ask a CLI for its version, which also proves it is on `PATH` and runnable.
pub async fn probe_cli(program: &str, label: &str) -> CliStatus {
    let result = tokio::time::timeout(
        PROBE_TIMEOUT,
        tokio::process::Command::new(program)
            .arg("--version")
            .stdin(std::process::Stdio::null())
            .output(),
    )
    .await;

    match result {
        Ok(Ok(output)) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout);
            let version = text
                .lines()
                .next()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_string);
            CliStatus {
                program: program.to_string(),
                label: label.to_string(),
                found: true,
                version,
                problem: None,
            }
        }
        Ok(Ok(output)) => CliStatus {
            program: program.to_string(),
            label: label.to_string(),
            found: false,
            version: None,
            problem: Some(format!(
                "`{program}` is installed but exited with an error: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )),
        },
        Ok(Err(error)) if error.kind() == std::io::ErrorKind::NotFound => CliStatus {
            program: program.to_string(),
            label: label.to_string(),
            found: false,
            version: None,
            problem: Some(format!("`{program}` was not found on PATH.")),
        },
        Ok(Err(error)) => CliStatus {
            program: program.to_string(),
            label: label.to_string(),
            found: false,
            version: None,
            problem: Some(format!("Could not run `{program}`: {error}")),
        },
        Err(_) => CliStatus {
            program: program.to_string(),
            label: label.to_string(),
            found: false,
            version: None,
            problem: Some(format!("`{program} --version` did not answer in time.")),
        },
    }
}

/// Probe every known agent CLI at once.
pub async fn probe_clis() -> Vec<CliStatus> {
    let probes = KNOWN_CLIS
        .iter()
        .map(|(program, label)| probe_cli(program, label));
    futures_util::future::join_all(probes).await
}

// --- Model hosts -------------------------------------------------------------

/// A machine serving models: Ollama on this laptop, or a box on the network.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelHost {
    pub id: String,
    /// What the user calls it, e.g. "DGX Spark".
    pub name: String,
    /// Root address, e.g. `http://spark.local:11434`.
    pub base_url: String,
}

impl ModelHost {
    /// The Ollama every install starts with.
    pub fn localhost() -> Self {
        Self {
            id: "local-ollama".into(),
            name: "This machine".into(),
            base_url: "http://localhost:11434".into(),
        }
    }
}

/// Which API a host speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostKind {
    /// Ollama's native API, at `/api/tags`.
    Ollama,
    /// Anything OpenAI-shaped: vLLM, LM Studio, llama.cpp, NIM.
    OpenAiCompatible,
}

/// A model a host is holding.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiscoveredModel {
    /// What to pass as the model id, e.g. `qwen3-coder:30b`.
    pub id: String,
    /// Parameter count as the server reports it, e.g. "30.5B".
    pub parameter_size: Option<String>,
    /// Quantisation, e.g. "Q4_K_M".
    pub quantization: Option<String>,
    /// On-disk size in bytes.
    pub size_bytes: Option<u64>,
}

impl DiscoveredModel {
    /// A one-line description for the picker.
    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        if let Some(size) = &self.parameter_size {
            parts.push(size.clone());
        }
        if let Some(quant) = &self.quantization {
            parts.push(quant.clone());
        }
        if let Some(bytes) = self.size_bytes {
            parts.push(human_size(bytes));
        }
        parts.join(" · ")
    }
}

/// The result of looking at a host.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostProbe {
    pub host: ModelHost,
    pub reachable: bool,
    pub kind: Option<HostKind>,
    pub models: Vec<DiscoveredModel>,
    pub problem: Option<String>,
    /// Server version, when it reports one.
    #[serde(default)]
    pub version: Option<String>,
    /// Something that will stop a run even though the host answered.
    #[serde(default)]
    pub warning: Option<String>,
}

/// The oldest Ollama the Codex CLI will drive.
///
/// Below this it refuses outright with "Ollama x is too old", which otherwise
/// only shows up when a run fails.
pub const MINIMUM_OLLAMA: (u32, u32, u32) = (0, 13, 4);

fn parse_version(version: &str) -> Option<(u32, u32, u32)> {
    let cleaned = version.trim().trim_start_matches('v');
    let mut parts = cleaned
        .split(|c: char| !c.is_ascii_digit())
        .filter(|p| !p.is_empty())
        .map(|p| p.parse::<u32>().ok());

    Some((
        parts.next()??,
        parts.next()??,
        parts.next().flatten().unwrap_or(0),
    ))
}

/// Whether this Ollama is too old for Codex to use.
pub fn ollama_too_old(version: &str) -> bool {
    match parse_version(version) {
        Some(found) => found < MINIMUM_OLLAMA,
        // Unparseable means unknown, and warning on a guess would be worse.
        None => false,
    }
}

/// Strip a trailing `/v1` and any trailing slash, so a host can be pasted in
/// either form.
pub fn normalise_host_base(url: &str) -> String {
    let trimmed = url.trim().trim_end_matches('/');
    trimmed
        .strip_suffix("/v1")
        .unwrap_or(trimmed)
        .trim_end_matches('/')
        .to_string()
}

/// The OpenAI-compatible base a runner should be pointed at.
pub fn openai_base(url: &str) -> String {
    format!("{}/v1", normalise_host_base(url))
}

/// Look at a host and report what it is holding.
///
/// Tries Ollama's native API first, since it reports parameter counts and
/// quantisation that the OpenAI-shaped listing does not.
pub async fn probe_host(host: &ModelHost) -> HostProbe {
    let base = normalise_host_base(&host.base_url);

    let client = match reqwest::Client::builder().timeout(PROBE_TIMEOUT).build() {
        Ok(client) => client,
        Err(error) => {
            return HostProbe {
                host: host.clone(),
                reachable: false,
                kind: None,
                models: Vec::new(),
                problem: Some(error.to_string()),
                version: None,
                warning: None,
            }
        }
    };

    // Ollama's native API first: it reports parameter counts and quantisation
    // that the OpenAI-shaped listing does not. A server that answers this path
    // with something else is not Ollama, so fall through rather than give up —
    // several OpenAI-compatible servers respond to it.
    if let Ok(response) = client.get(format!("{base}/api/tags")).send().await {
        if response.status().is_success() {
            if let Ok(body) = response.text().await {
                if let Ok(models) = parse_ollama_tags(&body) {
                    let version = ollama_version(&client, &base).await;
                    let warning = version.as_deref().filter(|v| ollama_too_old(v)).map(|v| {
                        let (major, minor, patch) = MINIMUM_OLLAMA;
                        format!(
                            "Ollama {v} is too old for Codex, which needs {major}.{minor}.{patch} \
or newer. Update it on this host, or runs using its models will fail."
                        )
                    });

                    return HostProbe {
                        host: host.clone(),
                        reachable: true,
                        kind: Some(HostKind::Ollama),
                        models,
                        problem: None,
                        version,
                        warning,
                    };
                }
            }
        }
    }

    match client.get(format!("{base}/v1/models")).send().await {
        Ok(response) if response.status().is_success() => {
            let body = response.text().await.unwrap_or_default();
            match parse_openai_models(&body) {
                Ok(models) => HostProbe {
                    host: host.clone(),
                    reachable: true,
                    kind: Some(HostKind::OpenAiCompatible),
                    models,
                    problem: None,
                    version: None,
                    warning: None,
                },
                Err(error) => HostProbe {
                    host: host.clone(),
                    reachable: true,
                    kind: Some(HostKind::OpenAiCompatible),
                    models: Vec::new(),
                    problem: Some(error),
                    version: None,
                    warning: None,
                },
            }
        }
        Ok(response) => HostProbe {
            host: host.clone(),
            reachable: true,
            kind: None,
            models: Vec::new(),
            problem: Some(format!(
                "{base} answered with status {} — it does not look like Ollama or an \
OpenAI-compatible server.",
                response.status()
            )),
            version: None,
            warning: None,
        },
        Err(error) => HostProbe {
            host: host.clone(),
            reachable: false,
            kind: None,
            models: Vec::new(),
            problem: Some(describe_unreachable(&base, &error)),
            version: None,
            warning: None,
        },
    }
}

/// Ollama's reported version, when it offers one.
async fn ollama_version(client: &reqwest::Client, base: &str) -> Option<String> {
    let response = client
        .get(format!("{base}/api/version"))
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let value: serde_json::Value = response.json().await.ok()?;
    value
        .get("version")
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

fn describe_unreachable(base: &str, error: &reqwest::Error) -> String {
    if error.is_timeout() {
        format!("{base} did not answer in time. Is the machine awake and on this network?")
    } else if error.is_connect() {
        format!(
            "Could not connect to {base}. Check the address, and that the server is \
listening on more than loopback."
        )
    } else {
        format!("Could not reach {base}: {error}")
    }
}

/// Probe several hosts concurrently, so one asleep machine does not hold up the rest.
pub async fn probe_hosts(hosts: &[ModelHost]) -> Vec<HostProbe> {
    futures_util::future::join_all(hosts.iter().map(probe_host)).await
}

/// Parse Ollama's `/api/tags`.
pub fn parse_ollama_tags(body: &str) -> std::result::Result<Vec<DiscoveredModel>, String> {
    #[derive(Deserialize)]
    struct Details {
        parameter_size: Option<String>,
        quantization_level: Option<String>,
    }
    #[derive(Deserialize)]
    struct Entry {
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        model: Option<String>,
        #[serde(default)]
        size: Option<u64>,
        #[serde(default)]
        details: Option<Details>,
    }
    #[derive(Deserialize)]
    struct Tags {
        /// Required on purpose. Without it an OpenAI-shaped body parses as an
        /// Ollama holding nothing, which is worse than failing: the host looks
        /// present but empty. Codex's own decoder draws the same line.
        models: Vec<Entry>,
    }

    let tags: Tags =
        serde_json::from_str(body).map_err(|error| format!("unexpected response: {error}"))?;

    let mut models: Vec<DiscoveredModel> = tags
        .models
        .into_iter()
        .filter_map(|entry| {
            // Ollama has used both keys across versions.
            let id = entry.name.or(entry.model)?;
            Some(DiscoveredModel {
                id,
                parameter_size: entry
                    .details
                    .as_ref()
                    .and_then(|d| d.parameter_size.clone()),
                quantization: entry
                    .details
                    .as_ref()
                    .and_then(|d| d.quantization_level.clone()),
                size_bytes: entry.size,
            })
        })
        .collect();

    models.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(models)
}

/// Parse an OpenAI-shaped `/v1/models`.
pub fn parse_openai_models(body: &str) -> std::result::Result<Vec<DiscoveredModel>, String> {
    #[derive(Deserialize)]
    struct Entry {
        id: String,
    }
    #[derive(Deserialize)]
    struct Listing {
        #[serde(default)]
        data: Vec<Entry>,
    }

    let listing: Listing =
        serde_json::from_str(body).map_err(|error| format!("unexpected response: {error}"))?;

    let mut models: Vec<DiscoveredModel> = listing
        .data
        .into_iter()
        .map(|entry| DiscoveredModel {
            id: entry.id,
            parameter_size: None,
            quantization: None,
            size_bytes: None,
        })
        .collect();

    models.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(models)
}

/// Bytes as something a person can read.
fn human_size(bytes: u64) -> String {
    const GB: f64 = 1_000_000_000.0;
    const MB: f64 = 1_000_000.0;
    let bytes = bytes as f64;

    if bytes >= GB {
        format!("{:.1} GB", bytes / GB)
    } else {
        format!("{:.0} MB", bytes / MB)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ollama_tags_are_parsed_with_their_details() {
        // Shape taken from a real Ollama /api/tags response.
        let body = r#"{
          "models": [
            {
              "name": "qwen3-coder:30b",
              "model": "qwen3-coder:30b",
              "size": 18500000000,
              "details": {"parameter_size": "30.5B", "quantization_level": "Q4_K_M"}
            },
            {
              "name": "llama3.1:8b",
              "size": 4700000000,
              "details": {"parameter_size": "8.0B", "quantization_level": "Q4_0"}
            }
          ]
        }"#;

        let models = parse_ollama_tags(body).unwrap();
        assert_eq!(models.len(), 2);
        // Sorted, so llama comes first.
        assert_eq!(models[0].id, "llama3.1:8b");
        assert_eq!(models[1].id, "qwen3-coder:30b");
        assert_eq!(models[1].parameter_size.as_deref(), Some("30.5B"));
        assert_eq!(models[1].quantization.as_deref(), Some("Q4_K_M"));
        assert_eq!(models[1].describe(), "30.5B · Q4_K_M · 18.5 GB");
    }

    #[test]
    fn an_empty_ollama_is_not_an_error() {
        let models = parse_ollama_tags(r#"{"models":[]}"#).unwrap();
        assert!(models.is_empty());
    }

    #[test]
    fn missing_details_still_yield_a_usable_model() {
        let models = parse_ollama_tags(r#"{"models":[{"name":"mistral:7b"}]}"#).unwrap();
        assert_eq!(models[0].id, "mistral:7b");
        assert_eq!(models[0].describe(), "");
    }

    #[test]
    fn a_non_ollama_response_is_rejected_rather_than_guessed_at() {
        assert!(parse_ollama_tags("<html>not ollama</html>").is_err());
    }

    #[test]
    fn openai_style_listings_are_parsed() {
        // What vLLM, LM Studio and NIM return.
        let body = r#"{"object":"list","data":[
          {"id":"Qwen/Qwen3-Coder-30B","object":"model"},
          {"id":"meta-llama/Llama-3.1-8B","object":"model"}
        ]}"#;

        let models = parse_openai_models(body).unwrap();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, "Qwen/Qwen3-Coder-30B");
        // No size metadata in this shape, and that is fine.
        assert!(models[0].parameter_size.is_none());
    }

    #[test]
    fn a_host_can_be_pasted_in_either_form() {
        for input in [
            "http://spark.local:11434",
            "http://spark.local:11434/",
            "http://spark.local:11434/v1",
            "http://spark.local:11434/v1/",
            "  http://spark.local:11434/v1  ",
        ] {
            assert_eq!(normalise_host_base(input), "http://spark.local:11434");
        }
    }

    #[test]
    fn runners_are_pointed_at_the_openai_path() {
        assert_eq!(
            openai_base("http://spark.local:11434"),
            "http://spark.local:11434/v1"
        );
        // Already-suffixed input must not end up with /v1/v1.
        assert_eq!(
            openai_base("http://spark.local:11434/v1"),
            "http://spark.local:11434/v1"
        );
    }

    #[test]
    fn sizes_are_readable() {
        assert_eq!(human_size(18_500_000_000), "18.5 GB");
        assert_eq!(human_size(700_000_000), "700 MB");
    }

    #[tokio::test]
    async fn a_missing_cli_is_reported_without_a_version() {
        let status = probe_cli("definitely-not-installed-xyz", "Nothing").await;
        assert!(!status.found);
        assert!(status.problem.unwrap().contains("PATH"));
    }

    #[tokio::test]
    async fn an_installed_cli_reports_its_version() {
        // `sh` exists everywhere this app runs.
        let status = probe_cli("git", "Git").await;
        assert!(
            status.found,
            "git should be installed in the test environment"
        );
        assert!(status.version.is_some());
    }

    /// Serve one canned JSON response per path, so the real HTTP probe can be
    /// exercised without Ollama installed.
    async fn fake_server(routes: Vec<(&'static str, &'static str)>) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let routes = routes.clone();
                tokio::spawn(async move {
                    let mut buffer = [0u8; 2048];
                    let Ok(n) = socket.read(&mut buffer).await else {
                        return;
                    };
                    let request = String::from_utf8_lossy(&buffer[..n]).to_string();
                    let path = request.split_whitespace().nth(1).unwrap_or("/").to_string();

                    let response = match routes.iter().find(|(route, _)| *route == path) {
                        Some((_, body)) => format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        ),
                        None => "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string(),
                    };
                    let _ = socket.write_all(response.as_bytes()).await;
                    let _ = socket.flush().await;
                });
            }
        });

        format!("http://127.0.0.1:{port}")
    }

    fn host_at(base_url: String) -> ModelHost {
        ModelHost {
            id: "test".into(),
            name: "Test host".into(),
            base_url,
        }
    }

    #[tokio::test]
    async fn an_ollama_host_is_probed_over_http() {
        let base = fake_server(vec![(
            "/api/tags",
            r#"{"models":[{"name":"qwen3-coder:30b","size":18500000000,"details":{"parameter_size":"30.5B","quantization_level":"Q4_K_M"}}]}"#,
        )])
        .await;

        let probe = probe_host(&host_at(base)).await;
        assert!(probe.reachable);
        assert_eq!(probe.kind, Some(HostKind::Ollama));
        assert_eq!(probe.models.len(), 1);
        assert_eq!(probe.models[0].id, "qwen3-coder:30b");
    }

    #[test]
    fn an_ollama_too_old_for_codex_is_recognised() {
        // Codex refuses anything below 0.13.4, which is otherwise only
        // discovered when a run dies.
        assert!(ollama_too_old("0.12.0"));
        assert!(ollama_too_old("0.13.3"));
        assert!(!ollama_too_old("0.13.4"));
        assert!(!ollama_too_old("0.15.0"));
        assert!(!ollama_too_old("1.0.0"));
        assert!(!ollama_too_old("v0.14.1"));
        // An unreadable version is not worth a false alarm.
        assert!(!ollama_too_old("unknown"));
    }

    #[tokio::test]
    async fn an_old_ollama_is_flagged_before_a_run_fails() {
        let base = fake_server(vec![
            ("/api/tags", r#"{"models":[{"name":"qwen3:8b"}]}"#),
            ("/api/version", r#"{"version":"0.12.0"}"#),
        ])
        .await;

        let probe = probe_host(&host_at(base)).await;
        assert!(probe.reachable);
        assert_eq!(probe.version.as_deref(), Some("0.12.0"));
        assert!(probe.warning.unwrap().contains("too old"));
    }

    #[tokio::test]
    async fn a_current_ollama_draws_no_warning() {
        let base = fake_server(vec![
            ("/api/tags", r#"{"models":[{"name":"qwen3:8b"}]}"#),
            ("/api/version", r#"{"version":"0.15.0"}"#),
        ])
        .await;

        let probe = probe_host(&host_at(base)).await;
        assert!(probe.warning.is_none());
    }

    #[tokio::test]
    async fn a_host_that_only_speaks_openai_is_still_understood() {
        // vLLM, LM Studio and NIM have no /api/tags, so the probe falls through.
        let base = fake_server(vec![(
            "/v1/models",
            r#"{"object":"list","data":[{"id":"Qwen/Qwen3-Coder-30B"}]}"#,
        )])
        .await;

        let probe = probe_host(&host_at(base)).await;
        assert!(probe.reachable);
        assert_eq!(probe.kind, Some(HostKind::OpenAiCompatible));
        assert_eq!(probe.models[0].id, "Qwen/Qwen3-Coder-30B");
    }

    #[tokio::test]
    async fn a_host_pasted_with_the_v1_suffix_still_probes() {
        let base = fake_server(vec![(
            "/api/tags",
            r#"{"models":[{"name":"llama3.1:8b"}]}"#,
        )])
        .await;

        // The user copies the endpoint their other tools use, which ends in /v1.
        let probe = probe_host(&host_at(format!("{base}/v1"))).await;
        assert!(probe.reachable);
        assert_eq!(probe.models[0].id, "llama3.1:8b");
    }

    #[tokio::test]
    async fn a_server_answering_api_tags_with_openai_json_is_still_understood() {
        // Some OpenAI-compatible servers respond to /api/tags with their own
        // shape. Treating that as a broken Ollama would hide a usable host.
        let base = fake_server(vec![
            (
                "/api/tags",
                r#"{"object":"list","data":[{"id":"qwen3.8:latest","owned_by":"library"}]}"#,
            ),
            (
                "/v1/models",
                r#"{"object":"list","data":[{"id":"qwen3.8:latest","owned_by":"library"}]}"#,
            ),
        ])
        .await;

        let probe = probe_host(&host_at(base)).await;
        assert!(probe.reachable);
        assert_eq!(probe.kind, Some(HostKind::OpenAiCompatible));
        assert_eq!(probe.models[0].id, "qwen3.8:latest");
        assert!(probe.problem.is_none());
    }

    #[tokio::test]
    async fn something_that_is_not_a_model_server_is_reported_as_such() {
        let base = fake_server(vec![("/", "{}")]).await;
        let probe = probe_host(&host_at(base)).await;
        // It answered, so it is reachable — but it holds no models.
        assert!(probe.reachable);
        assert!(probe.models.is_empty());
        assert!(probe.problem.is_some());
    }

    #[tokio::test]
    async fn an_unreachable_host_explains_itself() {
        let host = ModelHost {
            id: "dead".into(),
            name: "Nowhere".into(),
            // Reserved for documentation, so nothing is listening.
            base_url: "http://127.0.0.1:1".into(),
        };
        let probe = probe_host(&host).await;
        assert!(!probe.reachable);
        assert!(probe.problem.unwrap().contains("Could not connect"));
    }
}
