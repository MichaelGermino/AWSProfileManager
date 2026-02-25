# AI constraints (rules for future AI sessions)

## Do not change X without updating Y

- **Do not** change IPC channel names or payload shapes without updating: (1) `src/main/ipcHandlers.ts`, (2) `src/preload/index.ts` (invoke/on), (3) all renderer call sites. See **architecture.md** for the IPC list.
- **Do not** change the Profile type or `profiles.json` structure without considering: backup/restore (configBackup), replaceAllProfiles, and migration in profileStorage (e.g. refreshIntervalMinutes). See **domain-model.md**.
- **Do not** add new credential or API-key usage in the renderer. Any new secret must live in main and be accessed only via IPC that returns non-secret results. See **security-model.md** and **adr/002-credentials-in-main-only.md**.

## Credential and security

- **Do not** modify credential handling (Keytar, credentials file, auth flow) without reviewing **security-model.md** and **adr/002-credentials-in-main-only.md**. No sending passwords or API keys to the renderer; no logging them.
- **Do not** add a fallback that stores credentials in plaintext (e.g. in settings or a JSON file) if Keytar is unavailable, without an explicit security decision.

## IPC and architecture

- **Do not** refactor IPC structure (splitting or merging channels, changing preload API shape) without reviewing **architecture.md** and ensuring main, preload, and all renderer usages are updated together.
- **Do not** move profile or settings persistence to the renderer (e.g. localStorage for profiles). Persistence stays in main; renderer calls IPC to get/set.

## Required patterns for new features

- **New main-only data or secret**: Add a service in `src/main/services/`, register handler(s) in `ipcHandlers.ts`, expose only necessary methods/events in preload. Document new channels in architecture or a short comment.
- **New renderer screen or flow**: Use React state; fetch data via `window.electron.*`; do not assume a global store. If the screen needs fresh data when it becomes visible, refetch in a useEffect that depends on visibility (see TerminalScreen profiles refetch).
- **New persistence**: Prefer existing app data path (getAppDataPath()) and JSON unless there is a reason for a different store. Document in domain-model or architecture.

## Logging

- **Do not** log credentials (IdP password, AWS secret key, session token, API keys). Do not log full request/response bodies for auth or AI calls. Prefer logging only error messages or non-sensitive identifiers (e.g. profileId, channel name).
- Use `console.error` or `console.warn` in main for operational errors (e.g. PTY spawn failure); avoid logging in hot paths that could leak structure of sensitive data.

## Error handling

- **Storage reads**: On missing file or parse error, return empty/default (e.g. `{ profiles: [] }`) rather than throwing, unless the operation is critical and the UI must show an error. Document in architecture.md.
- **IPC handlers**: Let rejections propagate to the invoke caller so the renderer can handle them (e.g. try/catch or .catch on the promise). Do not swallow errors without at least logging in main.
- **Auth**: Surface failures to the user via existing auth events (e.g. credentials expired, refresh failed); do not crash the main process.

## What not to do

- **Do not** run the AWS CLI binary (child_process.exec/spawn of `aws`) unless a new ADR explicitly approves it. The app writes credentials and the user runs `aws` in the terminal.
- **Do not** read the profile list from `~/.aws/config` without an ADR; the app currently manages profiles only in `profiles.json`.
- **Do not** remove or rename preload API methods used by the renderer without updating all call sites (Profiles, Settings, TerminalScreen, etc.).
- **Do not** add dependencies that require native compilation (e.g. node-gyp) without checking CI and release build (e.g. Python version for node-pty/keytar); document in tech-stack or current-state if needed.
