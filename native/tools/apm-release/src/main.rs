//! apm-release: release tooling for AWS Profile Manager 2.0 (see lib.rs).
//!
//!   apm-release keygen --dir <secret dir> --public <path of update_key.pub>
//!       New updater key pair. Writes <dir>/updater.key (encrypted minisign secret key) and
//!       <dir>/updater.key.password.txt, and the public key file. Never overwrites a key.
//!
//!   apm-release feeds --installer <exe> --version <x.y.z> --out <dir>
//!       [--platform windows-x86_64] [--url <download url>] [--merge <apm-update.json>]
//!       [--key-file <updater.key>] [--password-file <updater.key.password.txt>]
//!       [--release-date <ISO-8601>]
//!       Writes apm-update.json, latest.yml (+ <channel>.yml for a pre-release) and
//!       <installer>.minisig. The key comes from --key-file / --password-file, else from the
//!       APM_UPDATER_KEY / APM_UPDATER_KEY_PASSWORD environment variables (CI secrets), else from
//!       %USERPROFILE%\.apm-release. Refuses a key that does not match the committed public key.
//!
//!   apm-release notices --out <THIRD_PARTY_NOTICES.txt> [--target x86_64-pc-windows-msvc]
//!       The licence notices of every crate compiled into the release app (cargo tree of apm-app,
//!       normal dependencies, no default features) and of the bundled fonts and icons.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use apm_release::*;

mod notices;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("keygen") => keygen(&flags(&args[1..])),
        Some("feeds") => feeds(&flags(&args[1..])),
        Some("notices") => notices::run(&flags(&args[1..])),
        _ => Err("usage: apm-release keygen|feeds ... (see the source header)".into()),
    };
    if let Err(e) = result {
        eprintln!("apm-release: {e}");
        std::process::exit(1);
    }
}

fn flags(args: &[String]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if let Some(name) = a.strip_prefix("--") {
            map.insert(name.to_string(), it.next().cloned().unwrap_or_default());
        }
    }
    map
}

fn need<'a>(f: &'a HashMap<String, String>, name: &str) -> Result<&'a str, String> {
    f.get(name).map(String::as_str).filter(|v| !v.is_empty()).ok_or_else(|| format!("--{name} is required"))
}

fn default_secret_dir() -> PathBuf {
    PathBuf::from(std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).unwrap_or_default()).join(".apm-release")
}

fn keygen(f: &HashMap<String, String>) -> Result<(), String> {
    let dir = f.get("dir").map(PathBuf::from).unwrap_or_else(default_secret_dir);
    let public = PathBuf::from(need(f, "public")?);
    let key_path = dir.join("updater.key");
    let pw_path = dir.join("updater.key.password.txt");
    if key_path.exists() || pw_path.exists() {
        return Err(format!("{} already exists; refusing to replace an existing key", key_path.display()));
    }
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let password = random_password();
    let (sk, pk) = generate_keypair(&password)?;
    std::fs::write(&key_path, sk).map_err(|e| e.to_string())?;
    std::fs::write(&pw_path, &password).map_err(|e| e.to_string())?;
    std::fs::write(&public, pk).map_err(|e| e.to_string())?;
    println!("secret key: {}", key_path.display());
    println!("password:   {}", pw_path.display());
    println!("public key: {}", public.display());
    Ok(())
}

fn read_key(f: &HashMap<String, String>) -> Result<minisign::SecretKey, String> {
    let env = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
    let (text, password) = if let Some(file) = f.get("key-file") {
        let pw_file = f.get("password-file").cloned().unwrap_or_else(|| format!("{file}.password.txt"));
        (read(file)?, read(&pw_file)?)
    } else if let Some(text) = env("APM_UPDATER_KEY") {
        (text, env("APM_UPDATER_KEY_PASSWORD").ok_or("APM_UPDATER_KEY_PASSWORD is not set")?)
    } else {
        let dir = default_secret_dir();
        (read(&dir.join("updater.key").to_string_lossy())?, read(&dir.join("updater.key.password.txt").to_string_lossy())?)
    };
    load_secret_key(&text, password.trim())
}

fn read(path: &str) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))
}

fn feeds(f: &HashMap<String, String>) -> Result<(), String> {
    let installer_path = Path::new(need(f, "installer")?);
    let version = need(f, "version")?.trim_start_matches('v');
    let out = PathBuf::from(need(f, "out")?);
    let platform = f.get("platform").map(String::as_str).unwrap_or("windows-x86_64");
    let file_name = installer_path.file_name().and_then(|n| n.to_str()).ok_or("bad installer path")?.to_string();
    if file_name.contains(' ') {
        return Err("the installer file name must not contain spaces (it is the URL in the feeds)".into());
    }
    let url = f.get("url").cloned().unwrap_or_else(|| github_download_url(version, &file_name));
    let installer = std::fs::read(installer_path).map_err(|e| format!("{}: {e}", installer_path.display()))?;

    let sk = read_key(f)?;
    let committed = public_key_line(COMMITTED_PUBLIC_KEY).ok_or("the committed public key is empty")?;
    if public_key_base64(&sk)? != committed {
        return Err("the updater key does not match crates/apm-core/src/update_key.pub; the app would refuse this signature".into());
    }

    let entry = PlatformEntry::new(platform, &url, &installer, &sk, &file_name, version)?;
    // Check the result exactly the way the app will before writing anything.
    let asset = apm_core_check(&entry, &installer)?;
    let existing = match f.get("merge") {
        Some(p) => Some(serde_json::from_str::<serde_json::Value>(&read(p)?).map_err(|e| e.to_string())?),
        None => None,
    };
    let manifest = update_manifest(version, std::slice::from_ref(&entry), existing.as_ref())?;

    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    std::fs::write(out.join("apm-update.json"), serde_json::to_string_pretty(&manifest).unwrap() + "\n").map_err(|e| e.to_string())?;
    std::fs::write(out.join(format!("{file_name}.minisig")), &asset).map_err(|e| e.to_string())?;
    if platform.starts_with("windows") {
        let date = f.get("release-date").cloned().unwrap_or_else(|| chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string());
        let yml = bridge_latest_yml(version, &file_name, &installer, &date);
        for name in channel_files(version) {
            std::fs::write(out.join(&name), &yml).map_err(|e| e.to_string())?;
            println!("wrote {}", out.join(&name).display());
        }
    }
    println!("wrote {}", out.join("apm-update.json").display());
    Ok(())
}

/// The signature verifies against the committed key (minisign's own verifier; apm-core uses
/// minisign-verify, and the crate's tests check the two agree).
fn apm_core_check(entry: &PlatformEntry, installer: &[u8]) -> Result<String, String> {
    let pk = minisign::PublicKey::from_base64(public_key_line(COMMITTED_PUBLIC_KEY).unwrap()).map_err(|e| e.to_string())?;
    let sig = minisign::SignatureBox::from_string(&entry.signature).map_err(|e| e.to_string())?;
    minisign::verify(&pk, &sig, std::io::Cursor::new(installer), true, false, false).map_err(|e| format!("signature self-check failed: {e}"))?;
    if sha256_hex(installer) != entry.sha256 {
        return Err("sha256 self-check failed".into());
    }
    Ok(entry.signature.clone())
}
