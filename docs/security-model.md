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

## Identity Center (Entra-federated SSO)

- **What is stored:** the OIDC client registration (clientId/clientSecret, ~90 day life) and the SSO access + refresh tokens, in `%APPDATA%\AWSProfileManager\sso-sessions.json`, keyed by a hashed `sso:<hash(startUrl|region)>` name. Keyed by **org, not profile** — one session serves every profile in that org.
- **Not in Keytar, deliberately.** Windows Credential Manager caps a credential blob at 2560 bytes. SSO access and refresh tokens are commonly 1-2 KB each, so `keytar.setPassword` throws and the session silently never persists. The short IdP password stores fine, which is why this only bites the SSO path.
- **Encryption:** every value is encrypted with Electron `safeStorage` (DPAPI on Windows, bound to the OS user) and, when a master password is enabled and unlocked, wrapped in the app's `v1:` AES-256-GCM layer first. There is **no plaintext fallback**: if `safeStorage` is unavailable the session stays in memory for the run and is lost on exit. Creating or resetting a master password **deletes** SSO sessions rather than re-encrypting them, which costs one browser sign-in and avoids a migration path.
- **In-memory cache:** sessions are also held in memory for the process lifetime, so a persistence failure costs a re-login after restart rather than on every call.
- **Renderer exposure:** none. The renderer receives `{ signedIn, expiresAt, identity }` plus account and role *names*. Access and refresh tokens never leave main, and neither do the AWS credentials minted from them — those go straight to `~/.aws/credentials`.
- **The stored IdP password is not used and cannot be.** There is no supported way to replay it into the Entra sign-in; doing so would require an embedded webview scraping the Microsoft login form.
- **Wrong-identity risk:** in organizations issuing separate normal and privileged (SA) accounts, signing in as the wrong identity is easy and otherwise silent. Mitigated two ways: sign-in defaults to an isolated per-org `persist:` partition rather than the default browser's session, and the signed-in identity is displayed back in the profile form.
- **Consent:** AWS shows an "Allow AWSProfileManager to access your data" page once per client registration (~quarterly). The registered client name is user-visible, so it must stay recognizable.
