# Current state

## What is complete

- Profile CRUD and reorder; persistence in profiles.json; migration of refreshIntervalHours to refreshIntervalMinutes.
- SAML login flow: IdP form submit, cookie handling, SAML assertion parse, role extraction, STS AssumeRoleWithSAML, write to ~/.aws/credentials.
- Keytar-backed storage for IdP credentials; “credentials required” / “refreshed” / “expired” flow; default credentials (username/password) for refresh-all.
- Dashboard: state derived from profiles (active/expired/never, expiry display); refresh-all and per-profile refresh.
- Scheduler: interval and expiry-based refresh; pause state; tray and IPC for paused changed.
- Roles cache by IdP URL; fetch roles with or without stored credentials.
- Settings: persistence (settings.json, ui-prefs); open credentials file (shell.openPath); Open WebUI URL/key/model (main-only for key).
- Config backup/restore: export/import JSON (settings + profiles) via file dialog.
- Terminal: one PTY per WebContents; xterm.js; fit addon; write/resize via IPC; insert-command (and optional --profile) from Command Details / AI.
- Command Explorer: mock tree + scraped AWS CLI docs (service list + per-service commands); cache in userData; fetch via hidden BrowserWindow.
- Command Details panel: syntax, options, examples; “Insert into terminal” and “Ask AI” (when configured).
- AI Assistant: Open WebUI chat/completions from main; renderer sends prompt; receives command + explanation; insert into terminal.
- Terminal layout: resizable left/center/right panels; collapsible Command Explorer and AI Assistant; resizable and collapsible Command Details; layout persisted in localStorage.
- Auto-updater: electron-updater; GitHub; install and restart.
- Single-instance lock; tray; splash screen; Windows icon and frameless option.

## Partially implemented

- **App data path**: getAppDataPath() uses APPDATA or `home/AppData/Roaming`; same logic used on all platforms. On Mac/Linux this yields a path under “AppData/Roaming”, which is Windows-specific. Works on Windows; on other platforms path may be non-standard or confusing.
- **Credentials path**: Relies on USERPROFILE for `.aws/credentials`. On non-Windows, USERPROFILE may be unset; behavior not explicitly handled.

## Known technical debt

- No TODO/FIXME/HACK comments found in src. No explicit “technical debt” section in code.
- Profile list and settings are read from disk on every IPC request; no caching layer (acceptable for current scale).
- Terminal: one PTY per WebContents; if multiple windows/tabs use terminal, each has a PTY (by design; no shared terminal).

## TODO areas in code

- None found (grep for TODO, FIXME, HACK, XXX in src returned no matches).

## Areas that need refactoring

- **Cross-platform paths**: App data and credentials paths are Windows-oriented (APPDATA, USERPROFILE). Refactor to use `app.getPath('userData')` for app data and a consistent home directory for `.aws` would improve portability. Unclear if Mac/Linux are supported targets.
- **Error handling**: Some catch blocks return empty data or ignore; no unified error reporting to user (e.g. toast or inline message) for storage failures.

## Areas that must not be refactored casually

- **IPC surface**: Changing channel names or payloads requires coordinated preload and renderer updates. Adding new channels is safe; renaming or removing breaks existing callers.
- **Credential flow**: Keytar keys (SERVICE_NAME, profileId, profileId_username, __default__) and the contract that renderer never receives passwords. Any change must preserve “credentials only in main” and be reviewed against security-model.md.
- **Profile storage shape**: Profile type and profiles.json format are shared with backup/restore and migration (refreshIntervalMinutes). Changing shape requires migration or versioning.
- **Preload API**: Renderer depends on `window.electron` method names and signatures. Removing or renaming breaks the UI.
- **Auth events**: Main sends auth:credentialsRequired, auth:credentialsRefreshed, auth:refreshStarted, auth:credentialsExpired, auth:refreshAllRequired. Renderer and tray depend on these. Changing event names or payloads requires coordinated updates.
