//! User-supplied background video. Port of the choose/clear parts of `backgroundMedia.ts`.
//!
//! The file is COPIED into app storage rather than referenced where the user picked it. A path
//! into Downloads or a network share would break the moment the file moved, and the background
//! would silently vanish with nothing in the UI to explain why.
//!
//! 1.x kept the copy in Electron's userData (`%APPDATA%\aws-profile-manager\background`); 2.0 has
//! no Electron userData and keeps it in the app-data directory (`<app data>/background`). The
//! stored `backgroundVideoPath` is absolute, so 1.x reinstalled over 2.0 still finds the file, and
//! 2.0 still plays a video 1.x copied until the user picks a new one.
//!
//! Serving the file to the player (1.x `appmedia://` with HTTP range support, which fragmented MP4
//! needs for its full duration) is the UI's job; it plays the stored `backgroundVideoPath`.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::{canceled, failure};
use crate::core::Core;
use crate::host::DialogRequest;

const FOLDER_NAME: &str = "background";
const ALLOWED_EXTENSIONS: &[&str] = &["mp4", "webm"];

impl Core {
    fn background_dir(&self) -> PathBuf {
        self.cfg.paths.app_data.join(FOLDER_NAME)
    }

    /// `chooseBackgroundVideo`: pick a video, copy it into app storage, and record it in settings.
    /// Replaces any previous one.
    pub fn choose_background_video(&self) -> Value {
        let request = DialogRequest {
            save: false,
            title: "Choose a background video".into(),
            default_name: None,
            filters: vec![("Video".into(), ALLOWED_EXTENSIONS.iter().map(|e| e.to_string()).collect())],
        };
        let Some(source) = self.host.dialog(request) else { return canceled() };
        let source = PathBuf::from(source);
        let ext = source.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        if !ALLOWED_EXTENSIONS.contains(&ext.as_str()) {
            return failure(format!("Unsupported format \".{ext}\". Use {}.", ALLOWED_EXTENSIONS.join(" or ")));
        }
        match self.copy_background_video(&source, &ext) {
            Ok(v) => v,
            Err(e) => failure(e.to_string()),
        }
    }

    fn copy_background_video(&self, source: &Path, ext: &str) -> std::io::Result<Value> {
        let dir = self.background_dir();
        std::fs::create_dir_all(&dir)?;

        // Clear previous files first, so switching mp4 -> webm doesn't leave the old one orphaned.
        for entry in std::fs::read_dir(&dir)?.flatten() {
            // A file held open by the still-playing video can refuse to delete; the new name wins
            // anyway, so this is not worth failing the whole operation over.
            let _ = std::fs::remove_file(entry.path());
        }

        let updated_at = self.cfg.clock.now_ms();
        let destination = dir.join(format!("video-{updated_at}.{ext}"));
        std::fs::copy(source, &destination)?;

        let file_name = source.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let mut settings = self.settings().get();
        settings.insert("backgroundVideoPath".into(), destination.to_string_lossy().into_owned().into());
        settings.insert("backgroundVideoName".into(), file_name.clone().into());
        settings.insert("backgroundVideoUpdatedAt".into(), updated_at.into());
        self.settings().save(&settings);

        Ok(json!({ "success": true, "fileName": file_name, "updatedAt": updated_at }))
    }

    /// `clearBackgroundVideo`: forget the custom video and delete the copy. The bundled default
    /// (if any) takes over again.
    pub fn clear_background_video(&self) {
        let stored = self.settings().get().get("backgroundVideoPath").and_then(Value::as_str).map(str::to_string);
        if let Some(stored) = stored.filter(|s| !s.is_empty()) {
            // Best effort — the settings entry below is what actually stops it being used.
            let _ = std::fs::remove_file(stored);
        }
        let mut settings = self.settings().get();
        for k in ["backgroundVideoPath", "backgroundVideoName", "backgroundVideoUpdatedAt"] {
            settings.shift_remove(k);
        }
        self.settings().save(&settings);
    }
}
