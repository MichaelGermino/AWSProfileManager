//! `~/.aws/credentials` on disk. Port of `credentialsFile.ts`; the editing rules live in
//! [`crate::credentials_ini`].

use std::path::PathBuf;

use crate::credentials_ini::{self, Section};
use crate::env::Platform;
use crate::js;

pub struct CredentialsFile {
    pub path: PathBuf,
    pub platform: Platform,
}

/// The keys the app writes for a profile.
pub struct ProfileCredentials<'a> {
    pub access_key_id: &'a str,
    pub secret_access_key: &'a str,
    pub session_token: &'a str,
    pub region: Option<&'a str>,
    pub output: Option<&'a str>,
}

impl CredentialsFile {
    fn read_text(&self) -> String {
        std::fs::read_to_string(&self.path).unwrap_or_default()
    }

    /// Parsed sections. An unreadable file reads as empty, like a missing one.
    pub fn read(&self) -> Vec<(String, Section)> {
        credentials_ini::parse(&self.read_text())
    }

    /// Write one profile's section; every other byte of the file stays as it was.
    pub fn write_profile(&self, name: &str, c: &ProfileCredentials) -> std::io::Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let values = vec![
            ("aws_access_key_id".to_string(), c.access_key_id.to_string()),
            ("aws_secret_access_key".to_string(), c.secret_access_key.to_string()),
            ("aws_session_token".to_string(), c.session_token.to_string()),
            ("region".to_string(), c.region.filter(|r| !r.is_empty()).unwrap_or("us-west-2").to_string()),
            ("output".to_string(), c.output.filter(|o| !o.is_empty()).unwrap_or("json").to_string()),
        ];
        let next = credentials_ini::set_section(&self.read_text(), name, &values, self.platform.eol());
        std::fs::write(&self.path, next)
    }

    /// Move a section to a new name, keeping its credentials. False when there was nothing to move.
    pub fn rename_section(&self, old_name: &str, new_name: &str) -> bool {
        let (from, to) = (js::trim(old_name), js::trim(new_name));
        if from.is_empty() || to.is_empty() || from == to || !self.path.exists() {
            return false;
        }
        match credentials_ini::rename_section(&self.read_text(), from, to, self.platform.eol()) {
            Some(next) => std::fs::write(&self.path, next).is_ok(),
            None => false,
        }
    }

    /// Remove a section (profile deleted). No-op if the file or the section is missing.
    pub fn remove_section(&self, name: &str) {
        if js::trim(name).is_empty() || !self.path.exists() {
            return;
        }
        if let Some(next) = credentials_ini::remove_section(&self.read_text(), name, self.platform.eol()) {
            let _ = std::fs::write(&self.path, next);
        }
    }
}
