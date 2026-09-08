//! Keeps the listener matching the settings.
//!
//! Settings can change at any moment — the switch flipped, the port edited,
//! the token rotated — and none of it should need a restart. The supervisor
//! watches for a change to the remote configuration and stops, starts or
//! restarts the server to suit, then reports what it is doing so the
//! Settings screen can show the pairing code and any problem.

use crate::net;
use heretic_core::config::RemoteConfig;
use heretic_core::Service;
use serde::Serialize;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

/// How often settings are checked for a change to the remote configuration.
const CHECK_EVERY: Duration = Duration::from_secs(2);

/// One address a phone could use.
#[derive(Debug, Clone, Serialize)]
pub struct Address {
    pub ip: String,
    pub label: String,
    pub url: String,
    pub tailscale: bool,
}

/// What the Settings screen shows.
#[derive(Debug, Clone, Serialize)]
pub struct RemoteStatus {
    pub enabled: bool,
    /// The address actually bound, once listening.
    pub listening: Option<String>,
    /// Why it is not listening, when it should be.
    pub error: Option<String>,
    /// The addresses a phone could use given what is bound, best first.
    pub addresses: Vec<Address>,
    /// Every address this machine has, for choosing what to bind to.
    pub interfaces: Vec<Address>,
    /// The link a phone opens to pair — the interface's address with the
    /// token in the fragment, where it never reaches a server log.
    pub pairing_url: Option<String>,
    /// The same link as a QR code, as inline SVG.
    pub pairing_qr: Option<String>,
}

struct Inner {
    running: Option<crate::Running>,
    applied: Option<RemoteConfig>,
    error: Option<String>,
}

pub struct Supervisor {
    service: Arc<Service>,
    inner: Mutex<Inner>,
}

impl Supervisor {
    pub fn new(service: Arc<Service>) -> Arc<Self> {
        Arc::new(Self {
            service,
            inner: Mutex::new(Inner {
                running: None,
                applied: None,
                error: None,
            }),
        })
    }

    /// Follow the settings for as long as the process runs.
    pub async fn run(self: Arc<Self>) {
        loop {
            self.reconcile().await;
            tokio::time::sleep(CHECK_EVERY).await;
        }
    }

    /// Bring the listener in line with the settings, if they changed.
    pub async fn reconcile(&self) {
        let config = self.service.settings().await.remote;
        let mut inner = self.inner.lock().await;
        if inner.applied.as_ref() == Some(&config) {
            return;
        }

        if let Some(running) = inner.running.take() {
            running.stop().await;
        }
        inner.error = None;

        if config.enabled {
            match crate::start(Arc::clone(&self.service), &config).await {
                Ok(running) => inner.running = Some(running),
                Err(error) => {
                    tracing::warn!(%error, "remote access could not start");
                    inner.error = Some(error.to_string());
                }
            }
        }
        inner.applied = Some(config);
    }

    /// Stop listening, whatever the settings say. Used on the way out.
    pub async fn stop(&self) {
        let mut inner = self.inner.lock().await;
        if let Some(running) = inner.running.take() {
            running.stop().await;
        }
        inner.applied = None;
    }

    pub async fn status(&self) -> RemoteStatus {
        let config = self.service.settings().await.remote;
        let inner = self.inner.lock().await;
        let listening = inner
            .running
            .as_ref()
            .map(|running| running.addr.to_string());
        let port = inner
            .running
            .as_ref()
            .map(|running| running.addr.port())
            .unwrap_or(config.port);

        let addresses = reachable(&config, port);
        let interfaces = net::interfaces()
            .into_iter()
            .map(|iface| Address {
                url: config.base_url_for(&iface.ip.to_string()),
                ip: iface.ip.to_string(),
                label: iface.label,
                tailscale: iface.tailscale,
            })
            .collect();
        let pairing_url = config.token.as_deref().and_then(|token| {
            let first = addresses.first()?;
            Some(format!("{}/#token={token}", first.url))
        });
        let pairing_qr = pairing_url.as_deref().and_then(qr_svg);

        RemoteStatus {
            enabled: config.enabled,
            listening,
            error: inner.error.clone(),
            addresses,
            interfaces,
            pairing_url,
            pairing_qr,
        }
    }
}

/// The addresses a phone could use, given what the server is bound to.
fn reachable(config: &RemoteConfig, port: u16) -> Vec<Address> {
    let config = RemoteConfig {
        port,
        ..config.clone()
    };
    let bound: Option<IpAddr> = config.bind.trim().parse().ok();

    let address = |ip: String, label: String, tailscale: bool| Address {
        url: config.base_url_for(&ip),
        ip,
        label,
        tailscale,
    };

    match bound {
        Some(ip) if ip.is_loopback() => {
            vec![address(ip.to_string(), "This machine only".into(), false)]
        }
        Some(ip) if !ip.is_unspecified() => {
            let known = net::interfaces().into_iter().find(|iface| iface.ip == ip);
            let (label, tailscale) = known
                .map(|iface| (iface.label, iface.tailscale))
                .unwrap_or_else(|| ("Configured address".into(), false));
            vec![address(ip.to_string(), label, tailscale)]
        }
        _ => {
            let mut all: Vec<Address> = net::interfaces()
                .into_iter()
                .map(|iface| address(iface.ip.to_string(), iface.label, iface.tailscale))
                .collect();
            if all.is_empty() {
                all.push(address(
                    "127.0.0.1".into(),
                    "This machine only".into(),
                    false,
                ));
            }
            all
        }
    }
}

/// The pairing link as a QR code.
fn qr_svg(url: &str) -> Option<String> {
    use qrcode::render::svg;
    let code = qrcode::QrCode::new(url.as_bytes()).ok()?;
    Some(
        code.render::<svg::Color>()
            .min_dimensions(180, 180)
            .quiet_zone(true)
            .dark_color(svg::Color("currentColor"))
            .light_color(svg::Color("transparent"))
            .build(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_loopback_bind_offers_only_this_machine() {
        let config = RemoteConfig {
            enabled: true,
            bind: "127.0.0.1".into(),
            port: 7411,
            token: Some("t".into()),
            public_url: None,
        };
        let addresses = reachable(&config, 7411);
        assert_eq!(addresses.len(), 1);
        assert_eq!(addresses[0].url, "http://127.0.0.1:7411");
    }

    #[test]
    fn a_public_url_replaces_the_bound_address_in_links() {
        let config = RemoteConfig {
            enabled: true,
            bind: "0.0.0.0".into(),
            port: 7411,
            token: Some("t".into()),
            public_url: Some("https://heretic.tail.ts.net".into()),
        };
        for address in reachable(&config, 7411) {
            assert_eq!(address.url, "https://heretic.tail.ts.net");
        }
    }

    #[test]
    fn the_qr_code_is_inline_svg() {
        let svg = qr_svg("http://100.64.0.2:7411/#token=abc").unwrap();
        assert!(svg.starts_with("<?xml") || svg.starts_with("<svg"));
        assert!(svg.contains("currentColor"));
    }
}
