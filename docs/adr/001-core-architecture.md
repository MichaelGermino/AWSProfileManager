# ADR 001: Core architecture (IPC bridge + main-process services)

## Context

- Need a desktop app that manages AWS profiles and SAML-based credential refresh, with an embedded terminal and optional AI assistance.
- Credentials (IdP passwords, AWS keys, API keys) must never be exposed to the renderer. The AWS CLI is not invoked by the app; the user runs it in a terminal.
- Must work within Electron’s process model: main (Node) and renderer (sandboxed browser).

## Decision

- **Single main process** owns all profile/settings/credential storage, SAML/STS auth, PTY spawning, AI and AWS CLI docs fetching. Renderer has no Node, no direct file or credential access.
- **Preload script** exposes a narrow API via `contextBridge.exposeInMainWorld('electron', ...)`:
  - Methods that return promises call `ipcRenderer.invoke(channel, ...args)`.
  - Events (auth, terminal, scheduler, update) use `ipcRenderer.on(channel, callback)`.
- **Main** registers `ipcMain.handle` for each invoke channel and calls into **service modules** (profileStorage, awsAuthService, credentialStorage, ptyService, aiService, etc.). Main sends events to renderer via `webContents.send(channel, ...args)`.
- **No Redux or global store** in renderer; each page uses React state and fetches data via the preload API when needed (e.g. getProfiles on mount or when tab becomes visible).
- **Profiles** are stored only in app data (`profiles.json`), not read from `~/.aws/config`. Credentials file is written after assume-role and on profile delete; not used as source of profile list.

## Alternatives considered

- **Renderer with nodeIntegration**: Rejected; would expose Node and credentials risk.
- **Reading profiles from ~/.aws/config**: Rejected; app manages its own profile list and IdP/role metadata; config does not hold IdP URLs or refresh settings.
- **Running AWS CLI in a child_process**: Rejected; user runs `aws` in the embedded terminal; app only writes credentials to the standard file and optionally injects `--profile` when inserting commands.
- **Centralized state in renderer (e.g. Redux)**: Rejected in favor of simpler per-screen fetch; avoids sync issues with main-owned data.

## Consequences

- Clear boundary: anything sensitive or that touches the filesystem/Keytar/network for auth runs in main. Renderer stays UI-only.
- Adding a feature that needs credentials or new storage implies new IPC (and possibly new service) in main and new methods on the preload API.
- All IPC channel names and payloads are part of the contract; changes require coordinated main, preload, and renderer updates.
- No single source of truth in renderer for “current profiles”; screens that need the list call getProfiles() (and may refetch on visibility, e.g. terminal screen).
