# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
npm run dev          # Build main, then run Vite + Electron concurrently (renderer at http://localhost:5173)
npm run build        # tsc -p tsconfig.main.json (main/preload/shared) + vite build (renderer)
npm run build:main   # Just compile main/preload/shared TypeScript
npm run dist         # Full build + electron-builder NSIS installer (output under release/)
npm run release      # Build + electron-builder --publish always (GitHub release)
```

There is no test suite, linter, or formatter wired up. TypeScript's `strict` + `noUnusedLocals` + `noUnusedParameters` is the only static check (`tsc -p tsconfig.main.json` on main; Vite handles renderer).

When editing main/preload code, you must run `npm run build:main` (or restart `npm run dev`) before changes take effect — the dev script only watches the renderer.

## Architecture

Electron desktop app with three TS roots:

- `src/main/` — Node-side: profile/settings storage, SAML auth, credential storage (Keytar), PTY shells, AI calls, AWS CLI docs scraping, auto-updater. Compiled to `dist/main/` via `tsconfig.main.json` (CommonJS).
- `src/preload/` — `contextBridge.exposeInMainWorld('electron', ...)` exposes a narrow IPC surface to the renderer. Compiled to `dist/preload/`.
- `src/renderer/` — React 18 + React Router (HashRouter) + Tailwind, built by Vite. `root: 'src/renderer'`, output `dist/renderer/`. `contextIsolation: true`, `nodeIntegration: false`.
- `src/shared/` — Types and helpers shared across main and renderer (e.g. `Profile`, `Settings`, `validateMasterPassword`).

### IPC bridge contract (load-bearing)

The renderer never imports Node modules and never holds secrets. Every cross-process call goes:

`renderer → window.electron.X → ipcRenderer.invoke('channel:foo', ...) → ipcMain.handle (in src/main/ipcHandlers.ts) → service in src/main/services/`

Events from main flow back via `webContents.send('channel:bar', ...)` → `ipcRenderer.on` in preload → callback registered by renderer (e.g. `onCredentialsRequired`, `onUpdateStatus`, `onTerminalData`).

Channel names and payload shapes are duplicated across **three files that must stay in sync**: `src/main/ipcHandlers.ts`, `src/preload/index.ts`, and the renderer call sites. Renaming or removing a channel without updating all three breaks the UI silently.

### Credential flow (security-critical)

- IdP username/password lives only in **Keytar** (`SERVICE_NAME = 'AWSProfileManager'`, account = profileId or `__default__`). The renderer collects them on a credentials-required modal, sends them via IPC, and never receives them back.
- Optional **master password**: when enabled, Keytar values are AES-256-GCM ciphertext (`v1:` prefix, PBKDF2-SHA256, 100k iters). The plaintext master password is held only in `sessionMasterPassword` in `credentialStorage.ts` for the running session — never persisted.
- AWS temporary credentials from STS `AssumeRoleWithSAML` are written directly to `~/.aws/credentials` (INI format, via the `ini` package) under `profile.credentialProfileName`, then dropped from memory. They are never sent to the renderer.
- Open WebUI API key is in `settings.json` (plaintext on disk). The AI service (`aiService.ts`) calls Open WebUI from main; the renderer only sees the boolean from `getOpenWebUiConfigStatus`.

### SAML refresh flow

`refreshProfile(profileId)` in `awsAuthService.ts`:
1. Load IdP creds from Keytar (or fire `auth:credentialsRequired` event to renderer if missing).
2. axios + tough-cookie → POST IdP login form → follow redirects to SAMLResponse.
3. Parse SAML assertion (xml2js, with regex fallback) for `Role` attribute pairs (`role_arn,principal_arn`).
4. If multiple roles or none cached, emit `auth:credentialsRequired` / role-picker flow; otherwise call `STSClient.AssumeRoleWithSAMLCommand`.
5. Write credentials to `~/.aws/credentials` via `credentialsFile.writeCredentialsForProfile`. Update `profile.expiration` in `profiles.json`.

`refreshScheduler.ts` runs every 60s and refreshes any auto-refresh profile whose `refreshIntervalMinutes` has elapsed since its last scheduled refresh, OR whose credentials expire within 15 minutes. After 2 consecutive failures it pauses auto-refresh and emits `auth:autoRefreshPausedForFailures`.

### Persistence locations

- **Profiles**: `%APPDATA%\AWSProfileManager\profiles.json` — `{ profiles: Profile[] }`. Migrates legacy `refreshIntervalHours` → `refreshIntervalMinutes` (×60, min 60). Returns `{ profiles: [] }` on missing/malformed file (no throws).
- **Settings**: `%APPDATA%\AWSProfileManager\settings.json` — see `Settings` in `src/shared/types.ts`.
- **UI prefs**: `ui-prefs.json` (sidebar collapsed, refresh-paused state).
- **Roles cache**: keyed by `idpEntryUrl`.
- **AWS CLI docs cache**: `userData/aws-cli-docs-cache/`, 24h TTL. HTML fetched via hidden `BrowserWindow` (system certs / Chromium stack), parsed with cheerio.
- **Renderer**: only `localStorage['terminal-layout']` for terminal panel sizes. No Redux/Zustand — every screen refetches via IPC on mount.
- **Path caveat**: `getAppDataPath()` uses `APPDATA || path.join(home, 'AppData', 'Roaming')` and credentials use `process.env.USERPROFILE`. Both are Windows-oriented. Mac/Linux are not verified targets.

### Terminal

`src/main/services/ptyService.ts` spawns one `node-pty` per renderer `WebContents`. Shell is PowerShell on Windows or `bashPath` from settings (Git Bash / WSL). The app **never** runs `aws` itself — "Insert into terminal" only writes a string to the PTY via `pty.write()`; the user runs it. There is no `child_process.exec/spawn` of `aws` anywhere in the codebase.

### Window lifecycle

- Single-instance lock via `app.requestSingleInstanceLock()`. Second launch focuses existing window.
- Splash window (5s minimum) → main window. `close` is preventDefaulted to hide-to-tray; only `before-quit` actually exits.
- Custom frameless title bar on Windows (`frame: !isWin`), with `window:minimize/maximize/close` IPC.

## Constraints (from docs/ai-constraints.md)

- **Do not** add credential or API-key handling in the renderer. New secrets go in `src/main/services/`, exposed via IPC that returns only non-secret results.
- **Do not** log credential values, API keys, or full auth/AI request/response bodies. Log profileId or channel name instead.
- **Do not** read profiles from `~/.aws/config` or `~/.aws/credentials`. The profile list is owned by `profiles.json`.
- **Do not** invoke the `aws` CLI from main (`child_process.exec/spawn`). The user runs `aws` in the embedded terminal.
- **Do not** add a plaintext fallback if Keytar fails — current behavior is silent no-op (keytar helpers return null).
- **Do not** rename Profile fields, IPC channels, or preload methods without updating main + preload + every renderer call site (and considering `configBackup` for Profile shape changes).
- New persistence: prefer `getAppDataPath()` + JSON. There is no SQLite.

## Further docs

`docs/` is the source of truth for architecture decisions:
- `architecture.md`, `domain-model.md`, `security-model.md`, `tech-stack.md`, `current-state.md`
- `ai-constraints.md` — read first for any non-trivial change
- `adr/001-core-architecture.md`, `adr/002-credentials-in-main-only.md`
