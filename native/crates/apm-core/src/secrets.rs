//! The OS keychain, laid out exactly as 1.x's keytar left it so 2.0 reads existing secrets:
//! service `AWSProfileManager`, one entry per account, value stored as UTF-8. On Windows keytar
//! names the credential `<service>/<account>` (keyring's default target would not find it).

use std::collections::BTreeSet;
use std::sync::Mutex;

pub trait SecretStore: Send + Sync {
    fn get(&self, account: &str) -> Option<String>;
    fn set(&self, account: &str, value: &str) -> Result<(), String>;
    fn delete(&self, account: &str);
}

/// The real keychain (Windows Credential Manager / macOS Keychain).
pub struct Keychain {
    service: String,
    /// Accounts this process wrote, so a test run can clean up after itself.
    touched: Mutex<BTreeSet<String>>,
}

impl Keychain {
    pub fn new(service: &str) -> Self {
        Keychain { service: service.to_string(), touched: Mutex::new(BTreeSet::new()) }
    }

    fn entry(&self, account: &str) -> Option<keyring::Entry> {
        if cfg!(windows) {
            keyring::Entry::new_with_target(&format!("{}/{account}", self.service), &self.service, account).ok()
        } else {
            keyring::Entry::new(&self.service, account).ok()
        }
    }

    pub fn touched(&self) -> Vec<String> {
        self.touched.lock().unwrap().iter().cloned().collect()
    }
}

impl SecretStore for Keychain {
    fn get(&self, account: &str) -> Option<String> {
        let bytes = self.entry(account)?.get_secret().ok()?;
        Some(String::from_utf8_lossy(&bytes).into_owned())
    }

    fn set(&self, account: &str, value: &str) -> Result<(), String> {
        self.touched.lock().unwrap().insert(account.to_string());
        let entry = self.entry(account).ok_or_else(|| "Credential storage is not available".to_string())?;
        entry.set_secret(value.as_bytes()).map_err(|e| e.to_string())
    }

    fn delete(&self, account: &str) {
        if let Some(e) = self.entry(account) {
            let _ = e.delete_credential();
        }
    }
}
