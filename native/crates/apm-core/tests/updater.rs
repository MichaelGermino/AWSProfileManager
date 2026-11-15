//! The updater end to end (plan §7.2): check GitHub releases, download the platform's installer from
//! the release manifest, verify sha256 + minisign, and hand the verified file to the host. A tiny
//! local HTTP server stands in for GitHub; no scenario covers updates because the 1.x updater
//! (electron-updater) was never testable this way.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use apm_core::core::{Core, CoreConfig};
use apm_core::env::{Clock, Paths, Platform};
use apm_core::extras::updater::{platform_key, MANIFEST_ASSET};
use apm_core::host::{DialogRequest, Host};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Default)]
struct Rec {
    events: Mutex<Vec<Value>>,
    installed: Mutex<Vec<Vec<u8>>>,
}

impl Host for Rec {
    fn emit(&self, channel: &str, payload: Vec<Value>) {
        if channel == "update" {
            self.events.lock().unwrap().push(payload[0].clone());
        }
    }
    fn open_external(&self, _: &str, _: &str) {}
    fn dialog(&self, _: DialogRequest) -> Option<String> {
        None
    }
    fn install_update(&self, installer: &std::path::Path) -> Result<(), String> {
        self.installed.lock().unwrap().push(std::fs::read(installer).unwrap());
        Ok(())
    }
}

/// Serves fixed bodies by path over plain HTTP on an already-bound listener.
fn serve(listener: tokio::net::TcpListener, routes: HashMap<String, Vec<u8>>) {
    let routes = Arc::new(routes);
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else { break };
            let routes = routes.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]);
                let path = req.split_whitespace().nth(1).unwrap_or("/").split('?').next().unwrap().to_string();
                let (status, body) = match routes.get(&path) {
                    Some(b) => ("200 OK", b.clone()),
                    None => ("404 Not Found", b"not found".to_vec()),
                };
                let head = format!("HTTP/1.1 {status}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n", body.len());
                let _ = sock.write_all(head.as_bytes()).await;
                let _ = sock.write_all(&body).await;
            });
        }
    });
}

async fn run(tamper: bool) -> (Arc<Rec>, Value) {
    let installer = b"pretend NSIS installer bytes".to_vec();
    let kp = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
    let sig = minisign::sign(Some(&kp.pk), &kp.sk, std::io::Cursor::new(&installer), None, None).unwrap();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server = format!("http://{}", listener.local_addr().unwrap());
    let manifest = json!({ "version": "9.0.0", "platforms": { platform_key(): {
        "url": format!("{server}/dl/setup.exe"),
        "sha256": hex::encode(Sha256::digest(&installer)),
        "signature": sig.into_string(),
    }}});
    let releases = json!([
        { "tag_name": "v9.1.0-beta.1", "prerelease": true, "draft": false, "assets": [] },
        { "tag_name": "v9.0.0", "prerelease": false, "draft": false, "assets": [
            { "name": MANIFEST_ASSET, "browser_download_url": format!("{server}/dl/{MANIFEST_ASSET}") }
        ]},
    ]);
    let mut routes: HashMap<String, Vec<u8>> = HashMap::new();
    routes.insert("/dl/setup.exe".into(), if tamper { b"swapped".to_vec() } else { installer.clone() });
    routes.insert(format!("/dl/{MANIFEST_ASSET}"), manifest.to_string().into_bytes());
    routes.insert("/github/repos/MichaelGermino/AWSProfileManager/releases".into(), releases.to_string().into_bytes());
    serve(listener, routes);

    let dir = std::env::temp_dir().join(format!("apm-updater-{}-{}", std::process::id(), tamper));
    let _ = std::fs::remove_dir_all(&dir);
    let host = Arc::new(Rec::default());
    let cfg = CoreConfig {
        paths: Paths { app_data: dir.join("appdata"), credentials_file: dir.join("credentials") },
        platform: Platform::current(),
        keychain_service: "AWSProfileManager-test-updater".into(),
        app_version: "2.0.0".into(),
        clock: Clock::default(),
        http: apm_core::http::HttpConfig {
            host_map: [("api.github.com".to_string(), format!("{server}/github"))].into_iter().collect(),
            extra_ca_pem: None,
            user_agent: "AWSProfileManager/2.0.0".into(),
        },
        update_public_key: Some(kp.pk.to_box().unwrap().to_string()),
        resources_dir: dir.clone(),
    };
    let core = Core::new(cfg, host.clone()).into_shared();
    let checked = core.dispatch("update:checkNow", &[]).await.unwrap();
    for _ in 0..100 {
        let last = core.dispatch("update:getStatus", &[]).await.unwrap();
        if matches!(last["type"].as_str(), Some("downloaded") | Some("error")) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    core.dispatch("update:installAndRestart", &[]).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    let _ = std::fs::remove_dir_all(&dir);
    (host, checked)
}

#[tokio::test(flavor = "multi_thread")]
async fn downloads_verifies_and_installs() {
    let (host, checked) = run(false).await;
    assert_eq!(checked, json!({ "type": "available", "version": "9.0.0" }), "pre-releases are skipped unless allowed");
    let events = host.events.lock().unwrap().clone();
    let types: Vec<&str> = events.iter().filter_map(|e| e["type"].as_str()).collect();
    assert_eq!(types.first(), Some(&"available"));
    assert!(types.contains(&"downloading"), "{types:?}");
    assert!(types.contains(&"downloaded"), "{types:?}");
    assert_eq!(types.last(), Some(&"installing"));
    assert_eq!(host.installed.lock().unwrap().as_slice(), &[b"pretend NSIS installer bytes".to_vec()]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_swapped_installer_is_never_installed() {
    let (host, _) = run(true).await;
    let events = host.events.lock().unwrap().clone();
    assert!(events.iter().any(|e| e["type"] == "error" && e["message"].as_str().unwrap_or("").contains("checksum")), "{events:?}");
    assert!(host.installed.lock().unwrap().is_empty());
}
