//! The core service object and `dispatch`, which mirrors the 1.x IPC contract
//! (`src/shared/ipcContract.ts`) channel for channel. The GPUI views call the typed methods; the
//! scenario runner calls `dispatch` with the same channel names and JSON arguments the Electron
//! suite passes to `window.electron`.

use std::sync::Arc;

use serde_json::{json, Value};

use crate::credentials_file::CredentialsFile;
use crate::dashboard;
use crate::env::{Clock, Paths, Platform, KEYCHAIN_SERVICE};
use crate::host::Host;
use crate::secrets::{Keychain, SecretStore};
use crate::store::{self, Obj};

/// How the core is set up. Tests point the paths and service names into a sandbox.
#[derive(Clone)]
pub struct CoreConfig {
    pub paths: Paths,
    pub platform: Platform,
    pub keychain_service: String,
    pub app_version: String,
    pub clock: Clock,
    pub http: crate::http::HttpConfig,
}

impl CoreConfig {
    pub fn for_user(app_version: &str) -> Self {
        CoreConfig {
            paths: Paths::default_for_user(),
            platform: Platform::current(),
            keychain_service: KEYCHAIN_SERVICE.to_string(),
            app_version: app_version.to_string(),
            clock: Clock::default(),
            http: crate::http::HttpConfig { user_agent: format!("AWSProfileManager/{app_version}"), ..Default::default() },
        }
    }
}

/// A failed call. `message` is what 1.x's handler threw (`err.message`).
#[derive(Debug, Clone, PartialEq)]
pub enum DispatchError {
    /// The channel exists in 1.x but is not ported yet.
    Unsupported(String),
    /// The handler failed with this message.
    Failed(String),
}

impl std::fmt::Display for DispatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DispatchError::Unsupported(c) => write!(f, "channel not ported yet: {c}"),
            DispatchError::Failed(m) => f.write_str(m),
        }
    }
}

pub type DispatchResult = Result<Value, DispatchError>;

pub struct Core {
    pub cfg: CoreConfig,
    pub host: Arc<dyn Host>,
    pub credentials: CredentialsFile,
    pub secrets: Arc<dyn SecretStore>,
    /// The master password for this session; never persisted.
    pub(crate) session_master_password: std::sync::Mutex<Option<String>>,
    pub(crate) refresh_state: crate::audit::RefreshBookkeeping,
    pub(crate) terminal: crate::terminal::TerminalState,
}

fn arg(args: &[Value], i: usize) -> &Value {
    args.get(i).unwrap_or(&Value::Null)
}

fn arg_str(args: &[Value], i: usize) -> &str {
    arg(args, i).as_str().unwrap_or("")
}

fn arg_obj(args: &[Value], i: usize) -> Obj {
    arg(args, i).as_object().cloned().unwrap_or_default()
}

fn string_list(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

fn objects(list: Vec<Obj>) -> Value {
    Value::Array(list.into_iter().map(Value::Object).collect())
}

impl Core {
    pub fn new(cfg: CoreConfig, host: Arc<dyn Host>) -> Self {
        let secrets = Arc::new(Keychain::new(&cfg.keychain_service));
        Self::with_secrets(cfg, host, secrets)
    }

    pub fn with_secrets(cfg: CoreConfig, host: Arc<dyn Host>, secrets: Arc<dyn SecretStore>) -> Self {
        let credentials = CredentialsFile { path: cfg.paths.credentials_file.clone(), platform: cfg.platform };
        Core {
            cfg,
            host,
            credentials,
            secrets,
            session_master_password: std::sync::Mutex::new(None),
            refresh_state: Default::default(),
            terminal: Default::default(),
        }
    }

    /// Ciphertexts of the stored Identity Center sessions (for the master-password checks).
    /// TODO(phase 3, Identity Center): read sso-sessions.json once the SSO store is ported.
    pub fn sso_session_blobs(&self) -> Vec<String> {
        Vec::new()
    }

    /// TODO(phase 3, Identity Center): delete sso-sessions.json entries.
    pub fn clear_all_sso_sessions(&self) {}

    /// TODO(phase 3, Identity Center): wipe the embedded sign-in browser data per org.
    pub fn clear_all_auth_partitions(&self) {}

    pub fn profiles(&self) -> store::Profiles<'_> {
        store::Profiles { paths: &self.cfg.paths, credentials: &self.credentials }
    }

    pub fn folders(&self) -> store::Folders<'_> {
        store::Folders { paths: &self.cfg.paths }
    }

    pub fn settings(&self) -> store::Settings<'_> {
        store::Settings { paths: &self.cfg.paths, platform: self.cfg.platform }
    }

    pub fn ui_prefs(&self) -> store::UiPrefs<'_> {
        store::UiPrefs { paths: &self.cfg.paths }
    }

    pub fn roles_cache(&self) -> store::RolesCache<'_> {
        store::RolesCache { paths: &self.cfg.paths }
    }

    pub fn dashboard_state(&self) -> Vec<Value> {
        let profiles = self.profiles().all();
        // A missing file is "no sections", which is what the check is for (fresh-machine restore).
        let sections = self.credentials.read();
        let names = match self.settings().get().get("accountDisplayNames") {
            Some(Value::Object(m)) => m.clone(),
            _ => serde_json::Map::new(),
        };
        dashboard::summaries(&profiles, Some(&sections), &names, self.cfg.clock.now_ms())
    }

    /// Run one IPC-contract channel.
    pub async fn dispatch(&self, channel: &str, args: &[Value]) -> DispatchResult {
        let ok = Ok(Value::Null);
        match channel {
            // Profiles
            "profiles:getAll" => Ok(objects(self.profiles().all())),
            "profiles:getById" => Ok(self.profiles().by_id(arg_str(args, 0)).map_or(Value::Null, Value::Object)),
            "profiles:save" => {
                self.profiles().save(arg_obj(args, 0));
                self.host.tray_changed();
                ok
            }
            "profiles:delete" => {
                self.profiles().delete(arg_str(args, 0));
                self.host.tray_changed();
                ok
            }
            "profiles:applyLayout" => {
                self.profiles().apply_layout(&string_list(arg(args, 0)), &arg_obj(args, 1));
                self.host.tray_changed();
                ok
            }
            "profiles:createMany" => {
                let list = arg(args, 0).as_array().cloned().unwrap_or_default();
                for p in &list {
                    self.profiles().save(p.as_object().cloned().unwrap_or_default());
                }
                self.host.tray_changed();
                Ok(json!({ "created": list.len() }))
            }

            // Folders
            "folders:getAll" => Ok(objects(self.folders().all())),
            "folders:save" => {
                let saved = self.folders().save(&arg_obj(args, 0), || uuid::Uuid::new_v4().to_string());
                self.host.tray_changed();
                Ok(Value::Object(saved))
            }
            "folders:delete" => {
                self.folders().delete(arg_str(args, 0), &self.profiles());
                self.host.tray_changed();
                ok
            }
            "folders:reorder" => {
                self.folders().reorder(&string_list(arg(args, 0)));
                self.host.tray_changed();
                ok
            }

            // Dashboard
            "dashboard:getState" => Ok(Value::Array(self.dashboard_state())),

            // Roles cache
            "roles:getCached" => Ok(self.roles_cache().get(arg_str(args, 0)).unwrap_or(Value::Null)),

            // Settings and app
            "settings:get" => Ok(Value::Object(self.settings().get_for_window())),
            "settings:save" => {
                let before = self.settings().get();
                let incoming = arg_obj(args, 0);
                self.settings().save_from_window(&incoming);
                self.host.apply_launch_at_startup(incoming.get("launchAtStartup").and_then(Value::as_bool).unwrap_or(false));
                if ["appIcon", "trayIconStyle", "trayStatusBadge"].iter().any(|k| before.get(*k) != incoming.get(*k)) {
                    self.host.app_icon_changed();
                }
                ok
            }
            "settings:getDefaultAccountDisplayNames" => Ok(self.settings().default_account_display_names()),
            "app:getVersion" => Ok(Value::String(self.cfg.app_version.clone())),
            "app:openExternal" => {
                self.host.open_external("url", arg_str(args, 0));
                ok
            }

            // UI prefs
            "ui:getSidebarCollapsed" => Ok(self.ui_prefs().sidebar_collapsed()),
            "ui:setSidebarCollapsed" => {
                self.ui_prefs().set_sidebar_collapsed(arg(args, 0).clone());
                ok
            }
            "ui:getCollapsedFolders" => Ok(Value::Array(self.ui_prefs().collapsed_folders())),
            "ui:setCollapsedFolders" => {
                self.ui_prefs().set_collapsed_folders(arg(args, 0));
                ok
            }

            // Saved credentials and the master password
            "credentials:getStatus" => Ok(self.credentials_status()),
            "credentials:getDefaultDisplay" => Ok(self.default_credentials_display()),
            "credentials:setDefault" => Ok(self.set_default_credentials(arg_str(args, 0), arg(args, 1).as_str())),
            "credentials:forgetDefault" => {
                self.delete_stored_credentials(crate::master::DEFAULT_CREDENTIALS_ID);
                ok
            }
            "credentials:getMasterPasswordStatus" => {
                let status = self.master_password_status();
                self.host.tray_changed();
                Ok(status)
            }
            "credentials:createMasterPassword" => {
                let r = self.create_master_password(arg_str(args, 0), arg_str(args, 1));
                self.host.tray_changed();
                Ok(r)
            }
            "credentials:unlock" => {
                let r = self.unlock(arg_str(args, 0));
                self.host.tray_changed();
                Ok(r)
            }
            "credentials:forgetAllAndResetMasterPassword" => {
                self.forget_all_and_reset();
                self.host.tray_changed();
                self.host.emit("credentials:masterPasswordReset", vec![]);
                ok
            }
            "credentials:getMasterPasswordEnabled" => {
                Ok(Value::Bool(self.settings().get().get("masterPasswordEnabled") == Some(&Value::Bool(true))))
            }
            "settings:openCredentialsFile" => {
                self.host.open_external("path", &self.cfg.paths.credentials_file.to_string_lossy());
                ok
            }

            // Auth audit log
            "logs:getAuthAuditEntries" => Ok(Value::Array(self.auth_audit_entries_for_viewer())),
            "logs:clearAuthAudit" => {
                self.clear_auth_audit();
                ok
            }
            "logs:openAuthViewer" => {
                self.host.open_log_viewer();
                ok
            }

            other => {
                if let Some(r) = self.dispatch_terminal(other, args) {
                    return r;
                }
                if let Some(r) = self.dispatch_auth(other, args).await {
                    return r;
                }
                if let Some(r) = self.dispatch_sso(other, args).await {
                    return r;
                }
                if let Some(r) = self.dispatch_assist(other, args).await {
                    return r;
                }
                if let Some(r) = self.dispatch_extras(other, args).await {
                    return r;
                }
                Err(DispatchError::Unsupported(other.to_string()))
            }
        }
    }
}
