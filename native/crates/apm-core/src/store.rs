//! The JSON stores in the app-data directory: profiles, folders, settings, UI prefs, roles cache.
//! Ports of `profileStorage.ts`, `folderStorage.ts`, `settingsService.ts`, `uiPrefsService.ts` and
//! `rolesCache.ts`.
//!
//! Records stay JSON objects (`serde_json::Map`, insertion-ordered) rather than structs: 1.x spreads
//! (`{...p}`) keep every key it does not know, and a 1.x build reinstalled over 2.0 data must find
//! them where it left them. Every call reads the file fresh, as 1.x does, so hand edits and other
//! writers are always seen.

use std::path::PathBuf;

use serde_json::{json, Map, Value};

use crate::credentials_file::CredentialsFile;
use crate::env::{Paths, Platform};
use crate::{js, json};

pub type Obj = Map<String, Value>;

fn ensure_dir(paths: &Paths) {
    let _ = std::fs::create_dir_all(&paths.app_data);
}

fn str_field<'a>(o: &'a Obj, key: &str) -> Option<&'a str> {
    o.get(key).and_then(Value::as_str)
}

// ---------------------------------------------------------------------------------------------
// Profiles (profiles.json: { "profiles": [...] })
// ---------------------------------------------------------------------------------------------

pub struct Profiles<'a> {
    pub paths: &'a Paths,
    pub credentials: &'a CredentialsFile,
}

/// `{...p, refreshIntervalMinutes: max(60, floor(minutes))}`; legacy `refreshIntervalHours`
/// converts (and stays), a missing interval is 60.
pub fn normalize_profile(raw: &Value) -> Obj {
    let mut p = raw.as_object().cloned().unwrap_or_default();
    let minutes = match p.get("refreshIntervalMinutes") {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(60.0),
        _ => match p.get("refreshIntervalHours") {
            Some(Value::Number(n)) => n.as_f64().unwrap_or(1.0) * 60.0,
            _ => 60.0,
        },
    };
    let clamped = minutes.floor().max(60.0);
    p.insert("refreshIntervalMinutes".into(), number(clamped));
    p
}

/// A JSON number the way JavaScript would hold it: integral values as integers.
pub fn number(f: f64) -> Value {
    if f.fract() == 0.0 && f.abs() < 9.007_199_254_740_992e15 {
        Value::from(f as i64)
    } else {
        serde_json::Number::from_f64(f).map(Value::Number).unwrap_or(Value::Null)
    }
}

/// The credentials-file section a profile owns: `credentialProfileName || name`, trimmed.
pub fn section_name_of(p: &Obj) -> String {
    let pick = str_field(p, "credentialProfileName").filter(|s| !s.is_empty()).or_else(|| str_field(p, "name"));
    js::trim(pick.unwrap_or("")).to_string()
}

impl Profiles<'_> {
    fn path(&self) -> PathBuf {
        self.paths.app_file("profiles.json")
    }

    pub fn all(&self) -> Vec<Obj> {
        ensure_dir(self.paths);
        let Some(data) = json::read_file(&self.path()) else { return Vec::new() };
        match data.get("profiles") {
            Some(Value::Array(items)) => items.iter().map(normalize_profile).collect(),
            _ => Vec::new(),
        }
    }

    fn write(&self, profiles: Vec<Obj>) {
        let value = json!({ "profiles": profiles.into_iter().map(Value::Object).collect::<Vec<_>>() });
        let _ = json::write_file(&self.path(), &value);
    }

    pub fn by_id(&self, id: &str) -> Option<Obj> {
        self.all().into_iter().find(|p| str_field(p, "id") == Some(id))
    }

    /// Insert or replace by id. Renaming the credentials section carries the live credentials
    /// across, unless another profile already owns the target section.
    pub fn save(&self, profile: Obj) {
        let mut all = self.all();
        let id = str_field(&profile, "id").map(str::to_string);
        match all.iter().position(|p| str_field(p, "id").map(str::to_string) == id) {
            Some(i) => {
                let from = section_name_of(&all[i]);
                let to = section_name_of(&profile);
                if !from.is_empty() && !to.is_empty() && from != to {
                    let claimed = all.iter().any(|p| str_field(p, "id").map(str::to_string) != id && section_name_of(p) == to);
                    if !claimed {
                        self.credentials.rename_section(&from, &to);
                    }
                }
                all[i] = profile;
            }
            None => all.push(profile),
        }
        self.write(all);
    }

    /// Remove a profile and its credentials-file section.
    pub fn delete(&self, id: &str) {
        let all = self.all();
        if let Some(p) = all.iter().find(|p| str_field(p, "id") == Some(id)) {
            let section = section_name_of(p);
            if !section.is_empty() {
                self.credentials.remove_section(&section);
            }
        }
        self.write(all.into_iter().filter(|p| str_field(p, "id") != Some(id)).collect());
    }

    /// One drag: new order and folder membership in ONE write. Unknown ids are ignored; profiles
    /// missing from the list keep their relative order at the end. In `folder_by_id`, `null` (or
    /// an empty id) means ungrouped and an absent key leaves the folder alone.
    pub fn apply_layout(&self, ordered_ids: &[String], folder_by_id: &Obj) {
        let mut all = self.all();
        for p in all.iter_mut() {
            let Some(id) = str_field(p, "id").map(str::to_string) else { continue };
            let Some(target) = folder_by_id.get(&id) else { continue };
            match target.as_str().filter(|s| !s.is_empty()) {
                Some(folder) => {
                    p.insert("folderId".into(), folder.into());
                }
                None => {
                    p.shift_remove("folderId");
                }
            }
        }
        let mut ordered: Vec<Obj> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for id in ordered_ids {
            if seen.contains(id) {
                continue;
            }
            // `new Map(profiles.map(p => [p.id, p]))`: the LAST profile with a duplicated id wins.
            if let Some(p) = all.iter().rev().find(|p| str_field(p, "id") == Some(id)) {
                ordered.push(p.clone());
                seen.insert(id.clone());
            }
        }
        let rest = all.into_iter().filter(|p| !str_field(p, "id").is_some_and(|id| seen.contains(id)));
        ordered.extend(rest);
        self.write(ordered);
    }

    /// Deleting a folder never deletes its profiles: they become ungrouped.
    pub fn clear_folder(&self, folder_id: &str) {
        let mut all = self.all();
        let mut changed = false;
        for p in all.iter_mut() {
            if str_field(p, "folderId") == Some(folder_id) {
                p.shift_remove("folderId");
                changed = true;
            }
        }
        if changed {
            self.write(all);
        }
    }

    /// Restore from backup: replace everything and drop credentials sections no profile owns.
    pub fn replace_all(&self, profiles: &[Value]) {
        ensure_dir(self.paths);
        let normalized: Vec<Obj> = profiles.iter().map(normalize_profile).collect();
        let kept: std::collections::HashSet<String> = normalized.iter().map(section_name_of).filter(|s| !s.is_empty()).collect();
        for p in self.all() {
            let section = section_name_of(&p);
            if !section.is_empty() && !kept.contains(&section) {
                self.credentials.remove_section(&section);
            }
        }
        self.write(normalized);
    }
}

// ---------------------------------------------------------------------------------------------
// Folders (folders.json: { "folders": [...] }; array order is display order)
// ---------------------------------------------------------------------------------------------

pub struct Folders<'a> {
    pub paths: &'a Paths,
}

const MAX_FOLDER_NAME: usize = 40;

/// Drop anything that is not a usable folder; keep only the known keys.
fn normalize_folder(raw: &Value) -> Option<Obj> {
    let f = raw.as_object()?;
    let id = str_field(f, "id").filter(|id| !js::trim(id).is_empty())?;
    let mut out = Obj::new();
    out.insert("id".into(), id.into());
    out.insert("name".into(), str_field(f, "name").unwrap_or("").into());
    if let Some(c) = str_field(f, "color") {
        out.insert("color".into(), c.into());
    }
    if let Some(i) = str_field(f, "iconName") {
        out.insert("iconName".into(), i.into());
    }
    Some(out)
}

/// Trimmed and cut to 40 UTF-16 units, like `trim().slice(0, 40)`.
fn clean_folder_name(name: Option<&str>) -> String {
    let t = js::trim(name.unwrap_or(""));
    let mut units = 0;
    let mut end = t.len();
    for (i, c) in t.char_indices() {
        if units + c.len_utf16() > MAX_FOLDER_NAME {
            end = i;
            break;
        }
        units += c.len_utf16();
    }
    t[..end].to_string()
}

impl Folders<'_> {
    fn path(&self) -> PathBuf {
        self.paths.app_file("folders.json")
    }

    pub fn all(&self) -> Vec<Obj> {
        ensure_dir(self.paths);
        let Some(data) = json::read_file(&self.path()) else { return Vec::new() };
        match data.get("folders") {
            Some(Value::Array(items)) => items.iter().filter_map(normalize_folder).collect(),
            _ => Vec::new(),
        }
    }

    fn write(&self, folders: Vec<Obj>) {
        let value = json!({ "folders": folders.into_iter().map(Value::Object).collect::<Vec<_>>() });
        let _ = json::write_file(&self.path(), &value);
    }

    /// Create (no id, or an unknown id) or update. A blank name becomes "New folder" on create
    /// and keeps the existing name on update. New folders go first so they are on screen.
    pub fn save(&self, input: &Obj, new_id: impl FnOnce() -> String) -> Obj {
        let mut all = self.all();
        let input_id = str_field(input, "id").filter(|s| !s.is_empty());
        let name = clean_folder_name(str_field(input, "name"));
        let optional = |out: &mut Obj| {
            for key in ["color", "iconName"] {
                if let Some(v) = input.get(key) {
                    out.insert(key.into(), v.clone());
                }
            }
        };
        if let Some(i) = input_id.and_then(|id| all.iter().position(|f| str_field(f, "id") == Some(id))) {
            let mut updated = all[i].clone();
            if !name.is_empty() {
                updated.insert("name".into(), name.into());
            }
            optional(&mut updated);
            all[i] = updated.clone();
            self.write(all);
            return updated;
        }
        let mut created = Obj::new();
        created.insert("id".into(), input.get("id").filter(|v| !v.is_null()).cloned().unwrap_or_else(|| Value::String(new_id())));
        created.insert("name".into(), if name.is_empty() { "New folder".into() } else { name.into() });
        optional(&mut created);
        all.insert(0, created.clone());
        self.write(all);
        created
    }

    /// Profiles first (the order that fails safe), then the folder.
    pub fn delete(&self, id: &str, profiles: &Profiles) {
        profiles.clear_folder(id);
        let all = self.all();
        self.write(all.into_iter().filter(|f| str_field(f, "id") != Some(id)).collect());
    }

    /// Unknown ids are dropped, missing ones appended.
    pub fn reorder(&self, ordered_ids: &[String]) {
        let all = self.all();
        let mut ordered: Vec<Obj> = Vec::new();
        for id in ordered_ids {
            // `byId.get(fid)` on a Map built from the folders: the last duplicate wins.
            if let Some(f) = all.iter().rev().find(|f| str_field(f, "id") == Some(id.as_str())) {
                ordered.push(f.clone());
            }
        }
        let seen: std::collections::HashSet<String> = ordered.iter().filter_map(|f| str_field(f, "id").map(str::to_string)).collect();
        let rest = all.into_iter().filter(|f| !str_field(f, "id").is_some_and(|id| seen.contains(id)));
        ordered.extend(rest);
        self.write(ordered);
    }

    pub fn replace_all(&self, folders: &[Value]) {
        self.write(folders.iter().filter_map(normalize_folder).collect());
    }
}

// ---------------------------------------------------------------------------------------------
// Settings (settings.json)
// ---------------------------------------------------------------------------------------------

pub struct Settings<'a> {
    pub paths: &'a Paths,
    pub platform: Platform,
}

pub fn default_settings() -> Obj {
    let v = json!({
        "defaultSessionDurationHours": 1,
        "defaultIdpEntryUrl": "",
        "launchAtStartup": true,
        "startMinimizedToTray": false,
        "accountDisplayNames": {},
        "accountDisplayNamesDefault": {},
        "openWebUiApiUrl": "",
        "openWebUiApiKey": "",
        "openWebUiModel": "",
        "terminalShell": "powershell",
        "bashPath": "",
        "authLoggingEnabled": true
    });
    v.as_object().unwrap().clone()
}

/// Git for Windows' bash.exe in the standard install locations (`getDetectedGitBashPath`).
pub fn detect_git_bash(platform: Platform) -> Option<String> {
    if platform != Platform::Windows {
        return None;
    }
    let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into());
    let pf86 = std::env::var("ProgramFiles(x86)").unwrap_or_else(|_| r"C:\Program Files (x86)".into());
    [pf, pf86]
        .into_iter()
        .map(|base| PathBuf::from(base).join("Git").join("bin").join("bash.exe"))
        .find(|p| p.exists())
        .map(|p| p.to_string_lossy().into_owned())
}

impl Settings<'_> {
    fn path(&self) -> PathBuf {
        self.paths.app_file("settings.json")
    }

    /// Defaults merged with the file, legacy keys migrated, Git Bash detected on Windows.
    pub fn get(&self) -> Obj {
        ensure_dir(self.paths);
        let mut merged = default_settings();
        if let Some(Value::Object(data)) = json::read_file(&self.path()) {
            for (k, v) in &data {
                merged.insert(k.clone(), v.clone());
            }
            let defaults = data
                .get("accountDisplayNamesDefault")
                .filter(|v| !v.is_null())
                .or_else(|| data.get("defaultAccountDisplayNames").filter(|v| !v.is_null()))
                .cloned()
                .unwrap_or_else(|| json!({}));
            merged.insert("accountDisplayNamesDefault".into(), defaults.clone());
            match data.get("accountDisplayNames") {
                None => {
                    // A spread of the defaults: an object copy (a non-object spreads to {}).
                    let copy = if defaults.is_object() { defaults } else { json!({}) };
                    merged.insert("accountDisplayNames".into(), copy);
                }
                Some(v) => {
                    merged.insert("accountDisplayNames".into(), v.clone());
                }
            }
        }
        // 'externalPrivate' was removed in 1.x.
        if str_field(&merged, "consoleBrowserMode") == Some("externalPrivate") {
            merged.insert("consoleBrowserMode".into(), "external".into());
        }
        if self.platform == Platform::Windows && js::trim(str_field(&merged, "bashPath").unwrap_or("")).is_empty() {
            if let Some(detected) = detect_git_bash(self.platform) {
                merged.insert("bashPath".into(), detected.into());
            }
        }
        merged
    }

    /// What the window may see: no API key, `openWebUiApiKeySet` instead.
    pub fn get_for_window(&self) -> Obj {
        let mut s = self.get();
        let key = s.shift_remove("openWebUiApiKey");
        let set = key.as_ref().and_then(Value::as_str).is_some_and(|k| !js::trim(k).is_empty());
        s.insert("openWebUiApiKeySet".into(), set.into());
        s
    }

    /// Save from the window: an absent key keeps the stored one; '' removes it.
    pub fn save_from_window(&self, incoming: &Obj) {
        let mut clean = incoming.clone();
        clean.shift_remove("openWebUiApiKeySet");
        if !clean.contains_key("openWebUiApiKey") {
            if let Some(stored) = str_field(&self.get(), "openWebUiApiKey").filter(|k| !k.is_empty()) {
                clean.insert("openWebUiApiKey".into(), stored.into());
            }
        }
        self.save(&clean);
    }

    /// On Windows an empty bash path forces PowerShell (there is no system bash).
    pub fn save(&self, settings: &Obj) {
        ensure_dir(self.paths);
        let mut to_write = settings.clone();
        if self.platform == Platform::Windows && js::trim(str_field(&to_write, "bashPath").unwrap_or("")).is_empty() {
            to_write.insert("terminalShell".into(), "powershell".into());
        }
        let _ = json::write_file(&self.path(), &Value::Object(to_write));
    }

    pub fn default_account_display_names(&self) -> Value {
        match self.get().get("accountDisplayNamesDefault") {
            Some(Value::Object(m)) => Value::Object(m.clone()),
            _ => json!({}),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// UI prefs (ui-prefs.json)
// ---------------------------------------------------------------------------------------------

pub struct UiPrefs<'a> {
    pub paths: &'a Paths,
}

impl UiPrefs<'_> {
    fn path(&self) -> PathBuf {
        self.paths.app_file("ui-prefs.json")
    }

    pub fn get(&self) -> Obj {
        ensure_dir(self.paths);
        let mut prefs = json!({
            "sidebarCollapsed": false,
            "refreshPaused": false,
            "refreshPausedDueToFailures": false,
            "collapsedFolderIds": []
        })
        .as_object()
        .unwrap()
        .clone();
        if let Some(Value::Object(data)) = json::read_file(&self.path()) {
            for (k, v) in data {
                prefs.insert(k, v);
            }
        }
        prefs
    }

    fn set(&self, key: &str, value: Value) {
        let mut prefs = self.get();
        prefs.insert(key.into(), value);
        ensure_dir(self.paths);
        let _ = json::write_file(&self.path(), &Value::Object(prefs));
    }

    /// `prefs.x ?? false` style read: a JSON null or absent value is the default.
    fn get_value(&self, key: &str) -> Value {
        self.get().get(key).cloned().unwrap_or(Value::Null)
    }

    pub fn sidebar_collapsed(&self) -> Value {
        self.get_value("sidebarCollapsed")
    }

    /// The Terminal page's pane sizes and collapsed panes (2.0 only). 1.x kept them in the
    /// window's localStorage; it keeps keys it does not know when it writes this file.
    pub fn terminal_layout(&self) -> Value {
        self.get_value("terminalLayout")
    }

    pub fn set_terminal_layout(&self, v: Value) {
        self.set("terminalLayout", v);
    }

    pub fn set_sidebar_collapsed(&self, v: Value) {
        self.set("sidebarCollapsed", v);
    }

    pub fn refresh_paused(&self) -> bool {
        self.get_value("refreshPaused").as_bool().unwrap_or(false)
    }

    pub fn set_refresh_paused(&self, v: bool) {
        self.set("refreshPaused", v.into());
    }

    pub fn refresh_paused_due_to_failures(&self) -> bool {
        self.get_value("refreshPausedDueToFailures").as_bool().unwrap_or(false)
    }

    pub fn set_refresh_paused_due_to_failures(&self, v: bool) {
        self.set("refreshPausedDueToFailures", v.into());
    }

    pub fn collapsed_folders(&self) -> Vec<Value> {
        match self.get_value("collapsedFolderIds") {
            Value::Array(ids) => ids.into_iter().filter(Value::is_string).collect(),
            _ => Vec::new(),
        }
    }

    pub fn set_collapsed_folders(&self, ids: &Value) {
        let clean: Vec<Value> = match ids {
            Value::Array(ids) => ids.iter().filter(|v| v.is_string()).cloned().collect(),
            _ => Vec::new(),
        };
        self.set("collapsedFolderIds", Value::Array(clean));
    }
}

// ---------------------------------------------------------------------------------------------
// Roles cache (rolesCache.json: { idpEntryUrl: { roles, fetchedAt } })
// ---------------------------------------------------------------------------------------------

pub struct RolesCache<'a> {
    pub paths: &'a Paths,
}

impl RolesCache<'_> {
    fn path(&self) -> PathBuf {
        self.paths.app_file("rolesCache.json")
    }

    fn read(&self) -> Obj {
        match json::read_file(&self.path()) {
            Some(Value::Object(m)) => m,
            _ => Obj::new(),
        }
    }

    pub fn get(&self, idp_entry_url: &str) -> Option<Value> {
        if js::trim(idp_entry_url).is_empty() {
            return None;
        }
        self.read().get(idp_entry_url).and_then(|e| e.get("roles")).filter(|r| !r.is_null()).cloned()
    }

    pub fn set(&self, idp_entry_url: &str, roles: Value, now_ms: i64) {
        if js::trim(idp_entry_url).is_empty() {
            return;
        }
        let mut cache = self.read();
        cache.insert(idp_entry_url.into(), json!({ "roles": roles, "fetchedAt": now_ms }));
        let _ = json::write_file(&self.path(), &Value::Object(cache));
    }
}
