# Security model

## Credentials in memory

- **IdP username/password**: Retrieved from Keytar only in main process when needed for SAML login. Passed to awsAuthService (submitCredentials, etc.); not stored in plaintext in any app-owned file. Not sent to renderer; renderer only sends username/password to main via IPC on user input.
- **AWS temporary credentials**: Obtained by main via STS AssumeRoleWithSAML; written directly to `~/.aws/credentials` (INI). Not held in app memory longer than needed to write the file. Not sent to renderer.
- **Open WebUI API key**: Read from settings (main only) in aiService; used in Authorization header for fetch. Never exposed via IPC to renderer. getAiConfigStatus returns only whether URL+key are configured (boolean).

## What is persisted vs not persisted

- **Persisted**: profiles.json (metadata, no passwords); settings.json (includes openWebUiApiUrl, openWebUiApiKey, openWebUiModel—API key is persisted in settings file on disk); ui-prefs; roles cache; Keytar (OS keychain) for IdP credentials; ~/.aws/credentials (standard AWS format). Terminal layout in renderer localStorage.
- **Not persisted in app**: STS tokens in memory (discarded after writing to credentials file). No logging of credentials (see below).

## Subprocess spawning

- **PTY**: node-pty spawns shell (powershell.exe or $SHELL) with args `[]`; no user-controlled command line. Environment is `{ ...process.env }`; cwd is HOME/USERPROFILE or process.cwd(). So the app does not pass user input as a command string to spawn; user types in the terminal. Risk: whatever the user types is executed by the shell (normal terminal risk); the app’s “insert into terminal” only writes text and does not execute it.
- **No other subprocesses** in the codebase that run user-controlled commands (no exec('aws ' + userInput), etc.).

## Shell injection

- **Insert into terminal**: insertCommandToTerminal writes a string to the PTY with pty.write(). The string is not passed to a shell invocation by the app; the terminal (shell) will interpret it when the user runs the command. So the app does not do “run this string as shell command” in a new process; it only injects text. Mitigation for injection is same as for any terminal: user is responsible for what they run. App does not sanitize the inserted string.
- **No eval or exec of user input** in main or renderer.

## Sensitive logs

- **Code intent**: No place in the codebase intentionally logs IdP passwords, AWS secret keys, or session tokens. Auth service and credential code paths do not log credential values.
- **Risk**: General-purpose logging (e.g. console.error(err) on auth failure) could include error messages; no audit of every error path for possible credential leakage. Keytar failures are silent (return null). Recommended: avoid logging request/response bodies in auth or AI calls.

## TLS trust extension (corporate proxy support)

- **What:** `src/main/services/enterpriseTls.ts` runs first in `app.whenReady()` and extends Node's TLS trust to include OS-provisioned CAs (Windows CryptoAPI / macOS Keychain) via `tls.getCACertificates('system')`. Implemented by monkey-patching `tls.createSecureContext` (covers axios, AWS SDK, electron-updater, undici) plus an undici global dispatcher with a 120s connect timeout for slow proxies.
- **Why:** corporate machines running TLS-inspection proxies (Zscaler, Netskope, etc.) re-sign every outbound HTTPS with a private CA. That CA is in the OS trust store but not Node's bundled Mozilla list, so without this extension every fetch fails with `UNABLE_TO_GET_ISSUER_CERT_LOCALLY` / `SELF_SIGNED_CERT_IN_CHAIN`.
- **Trust model:** verification stays on. We only ADD to the trust set, we do not weaken it. We do NOT use `rejectUnauthorized: false` or `NODE_TLS_REJECT_UNAUTHORIZED=0`. Anything the user's OS already trusts is now also trusted by Node — same trust boundary as Chrome/Edge/Firefox on the same machine.
- **Risk:** if the OS trust store is compromised (malicious root CA installed by malware or a misconfigured AD policy), the app would honor that trust. This is the same risk model as every native browser on the machine; no worse.

## Documented risks

- **Settings file**: openWebUiApiKey is stored in plaintext in settings.json under app data. Anyone with access to the app data directory can read it.
- **~/.aws/credentials**: Standard AWS file; readable by the user and any process with access to the user’s home directory. App does not add extra protection.
- **USERPROFILE / paths**: credentialsFile and credentialStorage use `process.env.USERPROFILE` for `.aws/credentials`. On non-Windows, USERPROFILE may be unset; path could be `'/.aws/credentials'` or similar. Behavior on Mac/Linux not verified.
- **Keytar**: If Keytar is unavailable (e.g. missing native module), credentials are not stored; app continues without storing passwords. No fallback to plaintext storage in the codebase.
