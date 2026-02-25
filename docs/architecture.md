# Architecture

## High-level system overview

- **Desktop Electron app** for managing AWS CLI profiles and SAML-based credential refresh.
- **Single main window** (React SPA); optional **system tray**; **splash screen** on launch.
- **Main process** owns: profile/settings storage, SAML auth, credential storage (Keytar), PTY shells, AI/Open WebUI calls, AWS CLI docs scraping, auto-updater.
- **Renderer** is a Vite-built React app; communicates only via **preload IPC bridge** (`window.electron`). No Node in renderer; no direct `~/.aws` or credential access.

## Main responsibilities

- **Profile management**: CRUD and reorder profiles stored in app data (`profiles.json`). Profiles define IdP URL, role, refresh interval, credential section name; they are **not** read from `~/.aws/config`.
- **SAML auth**: Log in to IdP (axios + cookies), parse SAML assertion for roles, call AWS STS `AssumeRoleWithSAML`, write temporary credentials to `~/.aws/credentials` (INI).
- **Credential storage**: IdP username/password stored in **Keytar** (OS keychain); never sent to renderer. Renderer only receives “credentials required” / “refreshed” events.
- **Terminal**: Embedded PTY (node-pty) per window; shell is PowerShell (Windows) or `$SHELL` (Unix). User runs `aws` (or any command) manually; app does **not** invoke the AWS CLI binary.
- **Terminal UX**: Command Explorer (mock + scraped AWS CLI docs), Command Details panel, AI Assistant (Open WebUI) for generating CLI examples. Resizable/collapsible panels; layout persisted in renderer localStorage.
- **Auto-refresh**: Scheduler runs periodic refresh for profiles with `autoRefresh`; can pause; notifies when credentials required or expired.
- **Updates**: electron-updater; GitHub releases; optional install-and-restart.

## Core architectural pattern

- **IPC bridge + service layer**: Renderer → preload (`contextBridge.exposeInMainWorld('electron', ...)`) → `ipcRenderer.invoke` / `ipcRenderer.on` → main `ipcMain.handle` / `webContents.send` → **main-process services** (profileStorage, awsAuthService, credentialStorage, ptyService, etc.).
- **No shared state store** in renderer beyond React component state and one localStorage key (terminal layout). Each screen fetches what it needs via IPC (e.g. `getProfiles`, `getSettings`).
- **Main process is stateless** in the sense that it does not keep an in-memory copy of profiles; it reads from disk on each request (getProfiles, getProfileById). Scheduler and auth service hold minimal pending state (e.g. pending SAML by profileId).

## Electron main vs renderer

| Concern | Main | Renderer |
|--------|------|----------|
| Profile CRUD | profileStorage read/write JSON | Invokes IPC; displays forms/lists |
| Credentials (IdP) | Keytar get/set; never exposed | Receives “credentials required”; sends username/password via IPC |
| AWS credentials file | Read/write INI; path from USERPROFILE | Can request “open credentials file” (shell.openPath) |
| SAML / STS | axios + STS client; write to credentials file | Triggers refresh; selects role; no tokens in renderer |
| Terminal I/O | node-pty spawn, write, resize; send `terminal:data` / `terminal:error` | xterm.js; sends `terminal:write`, `terminal:resize` |
| Open WebUI / AI | aiService: URL + API key from settings; fetch in main | Sends prompt via IPC; receives command + explanation |
| AWS CLI docs | Parse/cache HTML; fetch via hidden BrowserWindow | Requests service list / commands via IPC; displays UI |
| Settings | settingsService, uiPrefsService (JSON on disk) | Get/save via IPC |
| Window/tray | Create window, tray, splash; single-instance lock | N/A |

## AWS CLI interactions

- **The app does not run the `aws` CLI.** No `child_process.exec`/`spawn` of `aws` in the codebase.
- User runs `aws` (and any other commands) **inside the embedded terminal** (PTY). The app can **insert text** into the terminal (e.g. “Insert into terminal” from Command Details or AI); optional `--profile &lt;credentialProfileName&gt;` is appended when a profile is selected in the terminal bar.
- **AWS CLI docs**: Fetched (hidden BrowserWindow) and parsed (cheerio) in main; cached on disk under `userData/aws-cli-docs-cache`. Used to show command list and syntax in the UI only.

## Profile data storage

- **Location**: `getAppDataPath()` = `%APPDATA%\AWSProfileManager` (Windows) or `path.join(home, 'AppData', 'Roaming', 'AWSProfileManager')` (same path logic used on all platforms in code; may be Windows-centric).
- **File**: `profiles.json` with shape `{ profiles: Profile[] }`. Written with `JSON.stringify(data, null, 2)`.
- **No SQLite.** No reading of `~/.aws/config` for the profile list. Profile list is entirely app-managed.

## State management

- **Renderer**: React `useState` / `useCallback` / `useEffect` per page. No Redux, Zustand, or global store. Terminal layout (panel sizes, collapsed) in `TerminalScreen` persisted to localStorage under `terminal-layout`.
- **Main**: No in-memory profile cache; read-from-disk on each IPC. Scheduler and auth keep small maps (e.g. pending SAML by profileId, PTY by webContentsId).

## Error handling strategy

- **Storage**: `readProfilesData` returns `{ profiles: [] }` on missing file or parse error. No throw. Same pattern in settings/uiPrefs/rolesCache where applicable.
- **Auth**: Errors surfaced to renderer via IPC (e.g. refresh failure, credentials expired). Auth service catches and sends events; does not crash main.
- **PTY**: Spawn failure sends `terminal:error` to renderer and logs to console. Resize errors ignored.
- **IPC**: Handlers are async where needed; exceptions propagate to invoke caller (renderer sees rejected promise). No global IPC error wrapper.
- **Keytar**: If require('keytar') fails, credential helpers return null; app continues without stored credentials.

## Security considerations

- **Credentials**: IdP username/password in Keytar only; never in renderer. Open WebUI API key in settings (main only); AI requests done in main.
- **Context isolation**: `contextIsolation: true`, `nodeIntegration: false`. Preload exposes a narrow API.
- **Credentials file**: Path from `process.env.USERPROFILE`; written only after successful STS assume-role. No credentials in logs (intent visible in code; no explicit audit of every log path).
- **Shell**: PTY inherits `process.env`. No user-controlled command string passed to spawn; user types in terminal. “Insert into terminal” only writes text; no execution by the app.
- See **security-model.md** for details.

## Performance considerations

- **Profile list**: Read from disk on every `profiles:getAll`; acceptable for small lists. No pagination.
- **Terminal**: One PTY per WebContents; output streamed via IPC. Resize observer in renderer drives `terminal:resize` IPC.
- **AWS CLI docs**: Cached 24h; fetch via hidden BrowserWindow to use system certs. Parsing (cheerio) in main.
- **AI**: One request at a time from UI; no queue or retry logic documented in aiService.
