//! Runs IPC-contract calls against a data directory and prints the results as JSON. Used by the
//! round-trip tests (layer D, tests/parity/roundtrip.spec.ts): the 1.x app and the 2.0 core take
//! turns on the same data.
//!
//! stdin: {"appData": "...", "credentialsFile": "...", "keychainService": "...",
//!         "calls": [["channel", [args...]], ...]}
//! stdout: [{"ok": value} | {"error": message}, ...]

use std::io::Read;
use std::sync::Arc;

use apm_core::core::{Core, CoreConfig, DispatchError};
use apm_core::env::{Clock, Paths, Platform};
use apm_core::host::NullHost;
use serde_json::{json, Value};

#[tokio::main]
async fn main() {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).expect("read stdin");
    let req: Value = serde_json::from_str(&input).expect("JSON on stdin");
    let s = |k: &str| req[k].as_str().unwrap_or_else(|| panic!("missing {k}")).to_string();
    let cfg = CoreConfig {
        paths: Paths { app_data: s("appData").into(), credentials_file: s("credentialsFile").into() },
        platform: Platform::current(),
        keychain_service: s("keychainService"),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        clock: Clock::default(),
        http: apm_core::http::HttpConfig { user_agent: "AWSProfileManager/2.0.0".into(), ..Default::default() },
        update_public_key: None,
        resources_dir: std::env::temp_dir(),
    };
    let core = Core::new(cfg, Arc::new(NullHost)).into_shared();
    let mut out = Vec::new();
    for call in req["calls"].as_array().cloned().unwrap_or_default() {
        let channel = call[0].as_str().unwrap_or("");
        let args = call[1].as_array().cloned().unwrap_or_default();
        out.push(match core.dispatch(channel, &args).await {
            Ok(v) => json!({ "ok": v }),
            Err(DispatchError::Failed(m)) => json!({ "error": m }),
            Err(DispatchError::Unsupported(c)) => json!({ "error": format!("unsupported channel {c}") }),
        });
    }
    println!("{}", Value::Array(out));
}
