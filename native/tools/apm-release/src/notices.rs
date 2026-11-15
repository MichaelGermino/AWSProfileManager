//! THIRD_PARTY_NOTICES.txt for the installer: the licence texts of every crate in the release app
//! and of the bundled fonts and icons. Crates come from `cargo tree` (what is actually compiled
//! in for the target) and `cargo metadata` (where each crate's files are).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn workspace_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// `$CARGO` (set under `cargo run`), else the first `cargo` executable on PATH.
fn cargo_exe() -> PathBuf {
    if let Some(c) = std::env::var_os("CARGO").filter(|c| !c.is_empty()) {
        return c.into();
    }
    let names: &[&str] = if cfg!(windows) { &["cargo.exe"] } else { &["cargo"] };
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .flat_map(|dir| names.iter().map(move |n| dir.join(n)))
        .find(|p| p.is_file())
        // A PATH with a stray quote splits into one long entry; rustup's default location then.
        .or_else(|| {
            let home = std::env::var_os("CARGO_HOME").map(PathBuf::from).or_else(|| std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(|h| PathBuf::from(h).join(".cargo")))?;
            names.iter().map(|n| home.join("bin").join(n)).find(|p| p.is_file())
        })
        .unwrap_or_else(|| "cargo".into())
}

fn cargo(args: &[&str]) -> Result<String, String> {
    let out = Command::new(cargo_exe())
        .args(args)
        .current_dir(workspace_dir())
        .output()
        .map_err(|e| format!("{}: {e}", cargo_exe().display()))?;
    if !out.status.success() {
        return Err(format!("cargo {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr)));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// (name, version) of every crate compiled into the release app for `target`.
fn release_crates(target: &str) -> Result<BTreeSet<(String, String)>, String> {
    let tree = cargo(&["tree", "-p", "apm-app", "-e", "normal", "--no-default-features", "--target", target, "--prefix", "none", "-f", "{p}"])?;
    let mut set = BTreeSet::new();
    for line in tree.lines() {
        let mut parts = line.split_whitespace();
        let (Some(name), Some(version)) = (parts.next(), parts.next()) else { continue };
        let Some(version) = version.strip_prefix('v') else { continue };
        set.insert((name.to_string(), version.to_string()));
    }
    Ok(set)
}

fn is_licence_file(name: &str) -> bool {
    let n = name.to_ascii_uppercase();
    ["LICENSE", "LICENCE", "COPYING", "NOTICE", "UNLICENSE", "COPYRIGHT"].iter().any(|p| n.starts_with(p))
}

/// The bundled assets and their licence files, relative to the workspace.
const ASSETS: &[(&str, &str)] = &[
    ("Geist and Geist Mono fonts (Vercel)", "crates/apm-ui/assets/fonts/LICENSE-Geist.txt"),
    ("Lucide icons", "crates/apm-ui/assets/icons/LICENSE-LUCIDE"),
    ("Heroicons (profile icons)", "crates/apm-ui/assets/icons/profile/LICENSE-HEROICONS"),
];

pub fn run(f: &HashMap<String, String>) -> Result<(), String> {
    let out = f.get("out").ok_or("--out is required")?;
    let target = f.get("target").cloned().unwrap_or_else(|| "x86_64-pc-windows-msvc".to_string());
    let crates = release_crates(&target)?;
    let meta: Value = serde_json::from_str(&cargo(&["metadata", "--format-version", "1"])?).map_err(|e| e.to_string())?;
    let mut info: HashMap<(String, String), (String, PathBuf)> = HashMap::new();
    for p in meta["packages"].as_array().into_iter().flatten() {
        let key = (p["name"].as_str().unwrap_or("").to_string(), p["version"].as_str().unwrap_or("").to_string());
        let licence = p["license"].as_str().unwrap_or("").to_string();
        let dir = PathBuf::from(p["manifest_path"].as_str().unwrap_or("")).parent().map(Path::to_path_buf).unwrap_or_default();
        info.insert(key, (licence, dir));
    }

    // Identical texts are printed once, with every crate that ships them.
    let mut texts: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut summary = Vec::new();
    let mut without_file = Vec::new();
    for (name, version) in &crates {
        if name.starts_with("apm-") {
            continue; // this app (MIT)
        }
        let Some((licence, dir)) = info.get(&(name.clone(), version.clone())) else { continue };
        let id = format!("{name} {version}");
        summary.push(format!("{id:<48} {}", if licence.is_empty() { "(see licence file)" } else { licence }));
        let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
            .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_file() && p.file_name().and_then(|n| n.to_str()).is_some_and(is_licence_file)).collect())
            .unwrap_or_default();
        files.sort();
        if files.is_empty() {
            without_file.push(format!("{id} ({licence})"));
        }
        for file in files {
            if let Ok(text) = std::fs::read_to_string(&file) {
                let text = text.replace("\r\n", "\n").trim().to_string();
                texts.entry(text).or_default().push(id.clone());
            }
        }
    }

    let rule = "=".repeat(78);
    let mut doc = String::new();
    doc.push_str("AWS Profile Manager - third-party notices\n\n");
    doc.push_str("AWS Profile Manager is MIT licensed. It includes the software and assets listed below,\n");
    doc.push_str("each under its own licence, reproduced in full after the list.\n\n");
    doc.push_str(&format!("{rule}\nBundled assets\n{rule}\n\n"));
    for (what, _) in ASSETS {
        doc.push_str(&format!("{what}\n"));
    }
    doc.push_str(&format!("\n{rule}\nRust crates ({} crates)\n{rule}\n\n", summary.len()));
    for line in &summary {
        doc.push_str(line);
        doc.push('\n');
    }
    if !without_file.is_empty() {
        doc.push_str("\nThese crates ship no licence file; the standard text of their SPDX licence applies:\n");
        for c in &without_file {
            doc.push_str(&format!("  {c}\n"));
        }
    }
    for (what, rel) in ASSETS {
        let text = std::fs::read_to_string(workspace_dir().join(rel)).map_err(|e| format!("{rel}: {e}"))?;
        doc.push_str(&format!("\n{rule}\n{what}\n{rule}\n\n{}\n", text.replace("\r\n", "\n").trim()));
    }
    for (text, users) in &texts {
        doc.push_str(&format!("\n{rule}\nUsed by: {}\n{rule}\n\n{text}\n", users.join(", ")));
    }
    // Windows line endings: the file is opened from the install folder with Notepad.
    std::fs::write(out, doc.replace('\n', "\r\n")).map_err(|e| format!("{out}: {e}"))?;
    println!("wrote {out} ({} crates, {} distinct licence texts)", summary.len(), texts.len());
    Ok(())
}
