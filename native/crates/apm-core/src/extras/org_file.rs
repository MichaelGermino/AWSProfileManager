//! Export/import of the shareable organization configuration. Port of `orgConfigFile.ts`.
//!
//! Distinct from the backup: that captures one user's whole state including their profiles, this
//! captures only the org-wide settings a colleague would otherwise be told by hand. It is built
//! field by field from settings rather than by copying them, so a secret can never be included by
//! accident when new settings are added later — notably openWebUiApiKey, which sits right next to
//! the URL that IS exported. Parsing and sanitising are in [`crate::org_config`].

use serde_json::{json, Map, Value};

use super::{canceled, failure, json_dialog};
use crate::core::Core;
use crate::{js, json as jsjson, org_config};

impl Core {
    /// `buildOrgConfig`.
    pub fn build_org_config(&self, organization_name: Option<&str>) -> Map<String, Value> {
        let s = self.settings().get();
        let mut out = Map::new();
        out.insert("kind".into(), org_config::KIND.into());
        out.insert("version".into(), org_config::VERSION.into());
        let mut put = |key: &str, v: Option<&str>| {
            let t = js::trim(v.unwrap_or(""));
            if !t.is_empty() {
                out.insert(key.into(), t.into());
            }
        };
        let field = |k: &str| s.get(k).and_then(Value::as_str);
        put("organizationName", organization_name);
        put("idpEntryUrl", field("defaultIdpEntryUrl"));
        put("ssoStartUrl", field("defaultSsoStartUrl"));
        put("ssoRegion", field("defaultSsoRegion"));
        put("openWebUiApiUrl", field("openWebUiApiUrl"));
        put("openWebUiModel", field("openWebUiModel"));
        if let Some(Value::Object(names)) = s.get("accountDisplayNames") {
            if !names.is_empty() {
                out.insert("accountDisplayNames".into(), Value::Object(names.clone()));
            }
        }
        out
    }

    /// `exportOrgConfig`.
    pub fn export_org_config(&self, organization_name: Option<&str>) -> Value {
        let request = json_dialog(true, "Export organization configuration", Some("aws-profile-manager-org-config.json"));
        let Some(path) = self.host.dialog(request) else { return canceled() };
        let config = Value::Object(self.build_org_config(organization_name));
        match std::fs::write(&path, jsjson::stringify_pretty(&config)) {
            Ok(()) => json!({ "success": true, "path": path }),
            Err(e) => failure(e.to_string()),
        }
    }

    /// `importOrgConfig`: parse and clean the file; applying it is the window's next step.
    pub fn import_org_config(&self) -> Value {
        let Some(path) = self.host.dialog(json_dialog(false, "Open organization configuration", None)) else {
            return canceled();
        };
        let raw = std::fs::read_to_string(&path).map_err(|e| e.to_string());
        let parsed = raw.and_then(|r| serde_json::from_str::<Value>(&r).map_err(|e| e.to_string()));
        match parsed {
            Err(message) => failure(format!("Could not read the file: {message}")),
            Ok(v) => match org_config::parse(&v) {
                Some(config) => json!({ "success": true, "config": Value::Object(config) }),
                None => failure("That file is not an organization configuration exported from this app."),
            },
        }
    }
}
