# AWS Profile Manager 2.0 — Rust + Tauri rewrite plan

Status: draft, 2026-11-14. Owner: Michael.

## 1. Goals and non-goals

**Goals**

1. Same features, same behaviour, same data. A user who updates from 1.x keeps every profile, folder,
   setting, saved IdP credential, master password and (where technically possible) SSO session.
2. Minimal memory. Target budgets (Windows, measured as the summed working set of the whole process
   tree, including WebView2 children):
   - Hidden in tray: **≤ 25 MB** (no webview alive, Rust process only).
   - Window open, Profiles screen, idle: **≤ 120 MB**.
   - Terminal open with one shell: **≤ 150 MB**.
   - Baseline today: ~400 MB (to be measured precisely in Phase 0).
3. Proven parity. A test suite written against the Electron app first, then run unchanged against the
   Tauri app. 2.0 ships only when the suite is green on both.
4. Seamless upgrade. Windows users move from 1.x to 2.0 through the existing in-app updater.
5. A refreshed UI (unslop pass) that no longer reads as AI-generated, without changing features.

**Non-goals for 2.0**

- New features. Anything new waits for 2.1.
- Changing the data model, file formats, IPC semantics, or the security model.
- Linux support (cheap to add later with Tauri, but not in scope).

## 2. Guiding principles

- **One variable at a time.** Port first with the UI untouched, prove parity, then redesign. A test
  failure must point at one cause.
- **Tests before code.** The parity suite is built and green on Electron before any Rust is written
  for that feature.
- **Never silently lose data.** Migration only adds files; it never rewrites or deletes a 1.x file.
  A user can reinstall 1.x after 2.0 and find everything as it was.
- **Same contracts.** File paths, JSON shapes, keychain service/account names, the `v1:` ciphertext
  format and the `~/.aws/credentials` layout stay byte-compatible.
- **Security model unchanged.** Secrets live only in the backend (now Rust). The renderer never
  receives credentials, SSO tokens or API keys. Everything in `docs/ai-constraints.md` still applies.

## 3. Architecture of 2.0

### 3.1 Repository layout

Same repo, new branch `tauri-2.0`. Electron code stays on `main` (and keeps shipping 1.x fixes) until
cutover.

```
src/renderer/        React UI — reused, unchanged in Phase 2
src/shared/          TS helpers used by the renderer — unchanged
src/preload/         removed at cutover; replaced by src/renderer/api/tauriBridge.ts
src-tauri/           new Rust backend (Cargo workspace)
  src/main.rs
  src/commands/      one module per IPC area (profiles, auth, sso, terminal, ai, ...)
  src/services/      ports of src/main/services/*
  src/platform/      windows.rs / macos.rs (paths, shortcuts, browsers, tray icon tinting)
  tauri.conf.json
tests/
  fixtures/          shared JSON test vectors (consumed by vitest AND cargo test)
  fakes/             fake IdP / STS / OIDC / SSO portal / Open WebUI / GitHub server (Node)
  parity/            Playwright suite that runs against either app
  perf/              memory and startup measurement scripts
```

### 3.2 The bridge: keep `window.electron` as the contract

The renderer calls `window.electron.<area>.<method>(...)` today. In 2.0 a small TS module
(`tauriBridge.ts`) installs an object with **the same shape** on `window.electron`, implemented with
Tauri `invoke()` for requests and `Channel`/`listen()` for events.

Why: the renderer does not change at all in Phase 2, and the parity suite can call
`window.electron.*` through `page.evaluate` against both apps. The bridge is generated from one
channel table (`src/shared/ipcContract.ts`, written in Phase 1) so channel names and payload types
exist in one place instead of three.

Inventory to cover (from `src/main/ipcHandlers.ts`): **~87 request channels** and **14 events**
(`ai:chat-chunk`, `app:iconChanged`, `auth:credentialsExpired`, `auth:credentialsRefreshed`,
`auth:credentialsRequired`, `auth:networkUnavailable`, `auth:refreshAllRequired`,
`auth:refreshStarted`, `auth:ssoLoginRequired`, `credentials:masterPasswordReset`, `notify`,
`sso:loginProgress`, `terminal:data`, `terminal:error`), plus `update:*` status pushes.

### 3.3 Module-by-module port map

| 1.x (TypeScript)                                   | 2.0 (Rust)                                                                 | Notes |
|----------------------------------------------------|----------------------------------------------------------------------------|-------|
| `appPaths.ts`                                      | `platform::paths`                                                          | Must return the identical paths: `%APPDATA%\AWSProfileManager`, `~/Library/Application Support/AWSProfileManager`. Not Tauri's `app_data_dir` (named after identifier). |
| `profileStorage.ts`, `folderStorage.ts`, `uiPrefsService.ts`, `settingsService.ts`, `rolesCache.ts` | `services::store` with `serde_json`                  | Preserve unknown fields (`#[serde(flatten)] extra: Map`) so a 1.x reinstall loses nothing. Same migrations (`refreshIntervalHours` → minutes). Atomic writes (temp + rename). |
| `credentialStorage.ts` (keytar + `v1:` AES-GCM)    | `keyring` crate + `aes-gcm` + `pbkdf2`                                     | Same service `AWSProfileManager`, same accounts (profileId, `__default__`). Same `v1:` format, PBKDF2-SHA256 100k. Cross-language test vectors required. Keep "silent no-op, no plaintext fallback". |
| `ssoTokenStore.ts` (Electron `safeStorage`)        | `services::sso_store` — DPAPI (`windows` crate) / Keychain-held key on macOS | Electron `safeStorage` does not exist in Tauri. See §6.3 for migration of existing `sso-sessions.json`. |
| `credentialsFile.ts` (`ini`)                       | own minimal INI writer or `rust-ini`                                       | Must produce the same output as the `ini` package for the same input. Golden-file tests. Other profiles in the file untouched. |
| `awsAuthService.ts` (axios + tough-cookie + cheerio + xml2js + STS SDK) | `reqwest` (cookie store, redirects) + `scraper` + `quick-xml` + `aws-sdk-sts` or signed-less HTTP call | `AssumeRoleWithSAML` is unauthenticated, so a plain HTTPS POST to STS is enough and avoids the SDK's size. Keep regex fallback for SAML parsing. |
| `identityCenterService.ts` (node:https)            | `reqwest` + `oauth2`-style PKCE helpers + `tokio` loopback server          | Same pinned-port rule, `scopes` param, coalesced interactive logins, non-interactive scheduler path. |
| `enterpriseTls.ts`                                 | `reqwest` with `rustls-platform-verifier` (or `native-tls`)                | OS trust store is used directly — most of the Node workarounds disappear. Keep the 120 s connect timeout and the `AWSProfileManager/<version>` User-Agent on every client. |
| `refreshScheduler.ts`, `refreshFailureCounters.ts` | `services::scheduler` on `tokio::time::interval`                           | 60 s tick, 15 min expiry window, 2-strike pause. Scheduler must never open a browser. |
| `ptyService.ts` (node-pty)                         | `portable-pty` (ConPTY on Windows)                                         | One PTY per window. Data to the renderer over a Tauri `Channel`. Login shells on macOS (`-l`). |
| `aiService.ts` (fetch + SSE)                       | `reqwest` streaming + SSE parser, abort via `CancellationToken`            | Keep `tool_choice: 'none'`, history trim, empty-content error, JSON fallback when not streamed. |
| `awsCliDocsService.ts`, `browserFetchService.ts` (hidden BrowserWindow + cheerio) | `reqwest` + `scraper`                     | The hidden window existed for system certs; Rust TLS already uses the OS store. Same cache dir and 24 h TTL. |
| `consoleSignIn.ts`, `externalBrowser.ts`           | `services::console` + `platform::browsers`                                 | Same browser keys. Never launch URLs through `cmd /c start`; use `CreateProcess` argv / `open -a`. |
| `autoUpdater.ts` (electron-updater)                | `tauri-plugin-updater`                                                     | Pre-release toggle kept (two endpoints: stable / prerelease `latest.json`). |
| `changelogService.ts`, `tourService.ts`            | ports, same settings keys                                                  | `FORCE_WHATS_NEW` / `--force-whats-new` kept. |
| `configBackup.ts`, `orgConfigFile.ts`              | ports                                                                      | Backup v2 format byte-compatible; v1 restore still supported. |
| `authAuditLog.ts`, `authLogViewer.ts`              | port + second webview window                                               | |
| `networkStatus.ts`, `networkErrorClassifier.ts`    | ports                                                                      | Classifier uses error kinds, so it needs a new mapping from `reqwest` errors; fixture-driven. |
| `backgroundMedia.ts` (`appmedia://` with Range)    | `register_asynchronous_uri_scheme_protocol("appmedia")`                    | Must answer `Range` with `206` + `Content-Range` (fragmented MP4 issue). Dedicated test. |
| `appIcon.ts`, `shortcutIcon.ts`                    | `services::icons` + `windows` crate `IShellLinkW`                          | Runtime window/tray icon via `set_icon`. Shortcut repointing keeps the same rules. |
| `tray.ts`, `trayHud.ts`                            | Tauri `TrayIconBuilder` + HUD webview window + `tauri-plugin-positioner`   | Status badge drawn with the `image` crate. Monochrome tinting rules per platform. |
| `main.ts` window lifecycle                         | `main.rs` + `tauri-plugin-single-instance`, `tauri-plugin-autostart`       | Splash window, single instance, hide-to-tray, login item (packaged builds only). |
| `devFlags.ts`                                      | port                                                                       | |
| Embedded SSO `BrowserWindow` on `persist:sso-<hash>` | `WebviewWindowBuilder` with per-org `data_directory` (Windows) / `data_store_identifier` (macOS 14+) | Isolated, persistent per org. On macOS < 14 fall back to `ssoBrowserMode = 'external'`. Cookies from 1.x partitions do not migrate; one re-sign-in per org. |

Tauri plugins expected: `single-instance`, `autostart`, `updater`, `dialog`, `opener`, `process`,
`positioner`, `log`. Keep the capability allowlist (`capabilities/*.json`) minimal — the renderer gets
only the commands the bridge needs.

### 3.4 Window and title bar

- Windows: `decorations: false`, renderer draws the bar; replace `-webkit-app-region: drag` with
  `data-tauri-drag-region`.
- macOS: `titleBarStyle: "Overlay"`, `hiddenTitle: true`, traffic lights kept; renderer pads left.
  `hasCustomTitleBar()` stays the single switch.
- Splash: separate small window, same 5 s floor and readiness rule (show main on first of
  "page loaded" / "first paint" signal, never wait on one signal alone).

## 4. Memory strategy

Tauri alone removes Node and the bundled Chromium. The bigger wins come from how the app lives in the
tray:

1. **Destroy the webview when hidden to tray.** Close the main window (keep the Rust process, tray and
   scheduler alive) and recreate it on "Open App". In tray the app is then a ~15–25 MB Rust process.
   Cost: ~300–600 ms to reopen. *Decided — see §12.*
2. **Lazy terminal.** Today the Terminal screen mounts at app start and spawns its PTY. In 2.0 the PTY
   spawns on first visit to the Terminal screen. *Decided — see §12.*
3. **Lazy routes.** Code-split `Settings`, `TerminalScreen`, the wizard and the AI panel. `xterm`,
   `react-markdown` and `@dnd-kit` load only when used.
4. **Background video.** Paused and unloaded when the window is hidden or minimized.
5. **One HTTP client.** A single shared `reqwest::Client` (connection pool, TLS config) instead of
   one per service.
6. **Release profile.** `opt-level = "s"`, `lto = true`, `codegen-units = 1`, `strip = true`,
   `panic = "abort"`.

Each item is measured by the perf suite (§5.7), not assumed.

## 5. Test strategy — build on Electron, replay on Tauri

There is no test suite today. Phase 1 creates one and runs it against the shipping Electron app until
it is green. The same suite then becomes the acceptance gate for the Tauri app.

### 5.1 Layers

| Layer | What it proves | Runs against | Tooling |
|-------|----------------|--------------|---------|
| A. Logic vectors | Pure functions give identical output | Electron TS services (vitest) **and** Rust (cargo test) | Shared JSON fixtures in `tests/fixtures/` |
| B. Bridge contract | Every `window.electron.*` call returns the same shapes and has the same side effects on disk/keychain | Both apps | Playwright `page.evaluate` |
| C. UI flows | A user can do everything they can do today | Both apps | Playwright, role/testid locators |
| D. Data compatibility | 2.0 reads 1.x data; 1.x still reads data after 2.0 ran | Both apps | Golden data dirs |
| E. Platform behaviour | Tray, autostart, single instance, shortcuts, updater, proxy | Both apps | Scripted where possible, checklist otherwise |
| F. Visual | Screens look as intended | Both apps | Playwright screenshots (Phase 2: must match Electron; Phase 4: new baselines) |
| G. Performance | Memory and startup budgets | Both apps | `tests/perf/` |

### 5.2 How one suite drives both apps

- **Electron:** Playwright `_electron.launch()`.
- **Tauri (Windows):** launch the built exe with
  `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=<port>` and attach with
  `chromium.connectOverCDP()`. All windows (main, splash, HUD, log viewer, SSO) appear as CDP targets.
- A `driver` fixture hides the difference: `app.mainWindow()`, `app.windows()`, `app.restart()`,
  `app.dataDir`, `app.kill()`. Tests never import Electron or Tauri APIs directly.
- **macOS Tauri:** WKWebView has no WebDriver/CDP. Layers A, B (via a Rust-side command test harness)
  and D run on macOS CI; layer C runs on Windows only, and macOS gets the manual checklist in §5.6.

### 5.3 Test hooks (added to Electron in Phase 1, mirrored in Rust)

The app needs a few seams so tests are hermetic. In Electron they are honoured only when
`APM_TEST_MODE=1` **and** the build is unpackaged or a dedicated test build; in Rust they are behind a
`test-hooks` cargo feature that release builds do not enable.

- `APM_APPDATA_DIR` — override the app data root.
- `APM_AWS_CREDENTIALS_FILE` — override `~/.aws/credentials`.
- `APM_KEYCHAIN_SERVICE` — keychain service name suffix (e.g. `AWSProfileManager-test-<run>`), cleaned
  up after each run.
- `APM_ENDPOINTS` — JSON map overriding STS, OIDC, SSO portal, GitHub releases, AWS CLI docs and
  console federation base URLs, pointing them at the fakes.
- `APM_DIALOG_RESPONSES` — queued answers for native open/save dialogs (backup, restore, video,
  bash path, org config).
- `APM_NOW` / clock offset — lets scheduler tests fast-forward instead of waiting 60 s ticks.
- `APM_DISABLE_UPDATER`, `APM_SPLASH_MIN_MS=0`.

### 5.4 Fakes (`tests/fakes/`, one Node process)

- **IdP:** HTML login form, cookie-based session, redirects, final page with a `SAMLResponse`
  (single role, multiple roles, no roles, bad password, MFA-ish interstitial if the real IdP has one).
- **STS:** `AssumeRoleWithSAML` XML responses, success and error codes, slow responses.
- **OIDC + SSO portal:** RegisterClient, authorize + PKCE redirect to loopback, CreateToken (auth code
  and refresh), ListAccounts, ListAccountRoles, GetRoleCredentials, expiry and refresh failure.
- **Open WebUI:** `/api/models`, `/api/chat/completions` streaming SSE, non-streamed JSON fallback,
  `tool_calls` with null content, 403 for `User-Agent: node`.
- **GitHub:** releases API for changelog, update feeds (`latest.yml` for Electron, `latest.json` for
  Tauri).
- **AWS CLI docs:** a few static service/command pages.
- **TLS:** the fakes serve HTTPS with a test CA; a proxy-mode job installs that CA into the OS store to
  prove enterprise-TLS behaviour (no `rejectUnauthorized: false` anywhere).

### 5.5 Feature inventory to cover

Every row becomes at least one test (B and/or C). This list is the parity checklist.

**Profiles**
- Create SAML profile, create Identity Center profile, edit (top Save/Cancel, cancel-on-leave), delete.
- Create many (import), reorder by drag, search/filter, drag while filtered.
- Folders: create, rename, color, delete (profiles survive), collapse state persisted, move via
  right-click menu, drag into folder, dangling `folderId` renders ungrouped, `applyLayout` single write.
- Profile icon picker, avatar.
- Dashboard state: expiration display, role display text.

**Auth**
- SAML refresh: stored creds, missing creds → credentials modal, wrong password, role picker with
  multiple roles, cached role, STS error, network down (`auth:networkUnavailable`).
- Credentials written to the credentials file under `credentialProfileName`; other profiles preserved.
- Refresh all, refresh auto-refresh profiles, events fired in order.
- Scheduler: interval elapsed, 15-min expiry window, 2 failures → paused + event, pause/resume persisted.
- Identity Center: sign-in (embedded and external modes), session reuse across profiles in one org,
  silent refresh, refresh-token expiry → `ssoLoginRequired` (not a failure), sign out, list accounts,
  detect region, coalesced concurrent logins.
- Console sign-in in each supported browser key, multi-session enable.

**Credentials and security**
- Default credentials set/forget/display.
- Master password: create, unlock, wrong password, enabled status, forget all + reset, values are `v1:`
  ciphertext in the keychain when enabled, plaintext master password never on disk.
- No secrets in renderer: assert IPC responses never contain test passwords/tokens (scan payloads).
- No secrets in logs: scan the auth audit log and app logs for the test secrets.

**Terminal and AI**
- Start PowerShell / system shell, start Git Bash (Windows path gating), resize, write, data events.
- Insert command into terminal from AI reply and from command explorer (writes only, never runs).
- AWS CLI command explorer: service list, command details, cache TTL.
- AI: config status, models, streaming chat with progressive chunks, stop mid-stream keeps partial
  text, follow-up uses whole thread, empty reply shows error with `finish_reason`.

**Settings and app**
- Every settings field round-trips (generate from the `Settings` type so new fields are not missed).
- Appearance: app icon change applies live to window and tray, background video choose/clear and
  seekable playback (Range), monochrome tray.
- Launch at startup, browser list, bash path picker, open credentials file.
- Backup (v2) and restore (v1 and v2), org config export/import.
- Auth audit log: entries, clear, viewer window.
- What's new: changelog once per version, tour offered from the modal, skip banks the tour, tour with
  missing anchors under dev flag, return to starting route; developer options (7 taps).
- Setup wizard: every step and both auth types, AI step, master password step, org config step.
- Updates: check now, status, download, install and restart (against the fake feed), pre-release toggle.
- Window: minimize/maximize/close (close hides to tray), single instance focuses existing window,
  splash then main, sidebar collapsed persisted.
- Tray: menu items, HUD window, status badge colors (amber / red / gray rules).

### 5.6 Manual checklist (things automation cannot reach well)

Real corporate network behind a TLS-inspecting proxy; real IdP and real Identity Center org; Windows
Start-menu/taskbar shortcut icons after an update; Windows light/dark taskbar tray tinting; macOS menu
bar template icon, Dock re-open, login shell `PATH`, Intel and Apple Silicon; screen readers and
keyboard-only navigation.

### 5.7 Performance suite

- Script launches the app, waits for idle, samples the whole process tree's working set / private
  bytes (Windows: `Get-Process` tree incl. `msedgewebview2.exe`; macOS: `footprint`/`ps`) for: tray
  only, Profiles idle, Terminal open, after 30 min of scheduler activity (leak check).
- Startup time to interactive.
- Recorded for Electron in Phase 0 as the baseline; enforced as budgets (§1) in CI for Tauri.

## 6. Data and upgrade compatibility

### 6.1 Files that must stay byte-compatible

`profiles.json`, `settings.json`, `folders.json`, `ui-prefs.json`, roles cache, auth audit log,
`aws-cli-docs-cache/`, `background/`, backup files (v1/v2), org config files, `~/.aws/credentials`.
Golden fixtures for each, plus round-trip tests: "1.x writes → 2.0 reads → 2.0 writes → 1.x reads".

### 6.2 Keychain

Same service and account names, same `v1:` envelope. Cross-language vectors: TS encrypts → Rust
decrypts, and the reverse, including wrong-password failure. On macOS, check the Keychain ACL: items
created by the Electron app may prompt "allow access" for the new binary once; document it in the
release notes or pre-authorize during migration.

### 6.3 SSO sessions (`sso-sessions.json`)

1.x encrypts each value with Electron `safeStorage`:
- Windows: AES-256-GCM (`v10` prefix) with a key stored DPAPI-protected in Electron's `Local State`
  file (in Electron's `userData`, which is named after `productName`: `%APPDATA%\AWS Profile Manager`).
- macOS: AES-128-CBC with a key derived from the "AWS Profile Manager Safe Storage" Keychain item.

Plan: 2.0 implements a **read-only** decoder for that format, decrypts once, and writes the sessions to
a new `sso-sessions.v2.json` using its own protection (Windows DPAPI directly; macOS a random key held
in the Keychain), keeping the `v1:` master-password layer. The 1.x file is left untouched. If decoding
fails for any reason, the user signs in to each SSO org once (about 1 s per org) — logged, never fatal.

### 6.4 Things that cannot migrate

Embedded SSO browser cookies (1.x partitions) — one re-sign-in per org. Electron `localStorage`
(`terminal-layout` panel sizes) — read from the old Chromium LevelDB is not worth it; reset to default.

### 6.5 Installer, identity, autostart, shortcuts

- Keep identifier `com.awsprofilemanager.app` and product name "AWS Profile Manager".
- Tauri NSIS installer with a pre-install hook that silently runs the 1.x uninstaller (found via the
  uninstall registry key) **without** its "delete app data" option, removes the old login item, and
  removes old shortcuts. App data is never touched by the installer.
- Autostart: if 1.x had launch-at-startup on, 2.0 re-registers it under its own entry.

## 7. Update path from 1.x

### 7.1 Windows (automatic)

electron-updater downloads whatever `latest.yml` names, checks its sha512 and runs it silently.

1. **No extra 1.x release.** The test hooks live in the Electron code on the branch for local testing
   only; they are never released. The bridge goes straight from the current 1.x (1.6.x).
2. **Bridge release 2.0.0:** the GitHub release contains the Tauri NSIS installer **plus** a
   hand-generated `latest.yml` (version 2.0.0, file name, sha512, size) so 1.x clients take it as a
   normal update. A CI step generates this `latest.yml` from the Tauri artifact.
3. Ship the bridge to the **pre-release channel first** (users with "allow pre-release" on), then
   promote to stable.
4. From 2.0.0 on, the Tauri updater reads `latest.json` (signed with the Tauri updater key — new
   GitHub secret). Keep publishing a `latest.yml` pointing at the newest 2.x for at least a few months,
   for stragglers still on 1.x.

### 7.2 macOS

1.x mac builds are unsigned, so Squirrel.Mac cannot auto-update them today; mac users already download
by hand. They download the 2.0 dmg. After that, the Tauri updater verifies its own signature (not
Apple's), so auto-update should work even for unsigned builds — verify on a clean Mac early (Phase 2
spike), including Gatekeeper behaviour on the replaced bundle.

### 7.3 Rollback

Because migration is additive, a user can reinstall the latest 1.x from GitHub and keep working.
Document it in the 2.0 release notes and the README.

## 8. Build, CI and release

- New workflow `release-tauri.yml`: `prepare` (draft release, as today) → `build-windows` (NSIS) ‖
  `build-mac` (x64 + arm64 dmg and `.app.tar.gz` for the updater) → `bridge-feed` (generate
  `latest.yml`) → `publish`.
- Secrets: `TAURI_SIGNING_PRIVATE_KEY` (+ password), existing mac signing secrets (optional, as today).
- CI test jobs: `cargo test` + `cargo clippy -D warnings` + `cargo fmt --check` on both OSes; vitest
  (layer A, TS side); Playwright parity suite against the Electron build and the Tauri build on
  Windows; perf budgets on Windows.
- Icons: replace `npm run icons` (which runs under Electron) with a Node script using Playwright's
  Chromium to rasterize `designs.browser.js`. Output files stay committed.
- Drop: `keytar`/`node-pty` rebuild scripts, CJS marker script, the npm `overrides` block (most of it
  exists for electron-builder).

## 9. UI refresh (unslop) — Phase 4

Runs on the Tauri app after parity is proven, using the unslop skill's process.

1. **Screens in scope:** Profiles (incl. folders, context menus, empty state), Settings (all tabs),
   Terminal (top bar, command explorer, details panel, AI assistant), Setup wizard (all 9 steps),
   modals (credentials, role picker, master password, changelog), feature tour card, splash, tray HUD,
   auth log viewer, title bar.
2. **Audit** every screen against the skill's slop signs; one line per sign with its file.
3. **Direction** (product, character, world, moments, signature) written once for the whole app and
   **approved by Michael before any edit**. The product is a credential tool people keep open next to a
   terminal all day: the result should be calm and exact, with the chrome quiet and colour reserved for
   status (expiry, refresh state).
4. **Nine passes in order:** spacing, color, icons, copy, type, layout, details, visuals, motion —
   each across all screens before the next. App screens get no generated images; the "moment" comes
   from the product (e.g. the expiry/status display), built in code.
5. **Constraints that keep tests and features intact:**
   - Never remove or rename `data-tour` or `data-testid` attributes, ARIA roles or accessible names that
     tests rely on. The parity suite locates by role/testid only, so it must stay green through the
     redesign.
   - Keep the selectable app icons, background video and monochrome tray as features; the chrome must
     stay legible over a user's video.
   - Remove `animate-pulse`/glows except on things that are actually live (e.g. a refresh in progress).
6. **After:** regenerate visual baselines (layer F), walk every tour under the dev flag, write the 2.0
   tour (`TOURS` entry with a new id) and release notes.

## 10. Phases, deliverables and exit criteria

| Phase | Work | Exit criteria |
|-------|------|---------------|
| **0. Baseline** (≈1 week) | Measure Electron memory/startup per scenario. Write `src/shared/ipcContract.ts` from the 87 channels + 14 events. Spikes: Tauri hello-world with WebView2 CDP attach from Playwright; `portable-pty` + xterm over a Channel; decode an Electron `safeStorage` value in Rust; Tauri updater on an unsigned Mac. | Baseline numbers recorded. All four spikes work or have a fallback decided. |
| **1. Parity suite on Electron** (≈3–4 weeks) | Test hooks in Electron (committed on the branch, local testing only, never released). Fakes server. Layers A–D + F + G against Electron. Feature inventory §5.5 fully covered. | Suite green on Electron in CI, on Windows; layers A/D also on macOS. |
| **2. Tauri port** (≈6–10 weeks) | `src-tauri` backend module by module in this order: paths + stores → keychain + master password → credentials file → SAML auth + STS → scheduler → Identity Center → tray/window lifecycle → terminal → AI → CLI docs → icons/shortcuts/background media → updater → migration. `tauriBridge.ts`. Renderer unchanged except drag regions and lazy routes. | Same suite green on Tauri (Windows); Rust tests green on macOS; memory budgets met. |
| **3. Migration and packaging** (≈2 weeks) | NSIS hooks, SSO session migration, bridge `latest.yml`, release workflow, upgrade tests on clean VMs from 1.5 / 1.6 / 1.7 with real data dirs. | Upgrade from each 1.x version keeps all data; 1.x reinstall after 2.0 still works. |
| **4. Unslop** (≈2–3 weeks) | §9. | Direction approved; checklist passes; parity suite still green; new visual baselines. |
| **5. Beta → release** | 2.0.0-beta on the pre-release channel, then stable. Monitor issues. | No data-loss or auth regressions reported during beta. |

Estimates assume part-time work with Claude Code doing most of the implementation; the Rust backend is
roughly 7k lines of TS to port (~8–10k lines of Rust).

## 11. Risks

| Risk | Mitigation |
|------|------------|
| SAML scraping behaves differently in `reqwest` (cookies, redirects, form encoding) against a real IdP | Fake IdP mirrors the real flow; manual test against the real IdP early in Phase 2. |
| Corporate TLS proxy breaks Rust TLS | `rustls-platform-verifier` uses the OS store; proxy-mode CI job with a custom root; manual test on the corporate network. |
| WebView2 Conditional Access blocks embedded SSO | `ssoBrowserMode = 'external'` stays. |
| macOS Keychain prompts for the new binary | Detect on first run, explain in release notes. |
| `safeStorage` decode fails | Fallback = one SSO re-sign-in per org. Never fatal. |
| Memory target missed because of WebView2 overhead | Destroy-on-hide (§4.1) keeps the tray state small regardless; perf suite catches regressions. |
| Bridge update fails mid-install on Windows | Old app is only uninstalled after new files are in place; installer logs; 1.x reinstall path. |
| Parity drift: a 1.x fix lands on `main` during the port | Every 1.x fix gets a parity test first; the test then fails on Tauri until ported. |

## 12. Decisions (resolved 2026-11-14)

1. **Order:** port with the UI unchanged first, then unslop.
2. **Hidden to tray destroys the window.** The Rust process, tray and scheduler stay alive; the window
   is recreated on "Open App".
3. **Lazy PTY:** the shell spawns on first visit to the Terminal screen.
4. **Minimum macOS for embedded SSO: 14.** Older macOS uses the external browser.
5. **No 1.7.0 release.** Test hooks are added to the Electron code on the branch and committed, but
   used only for local testing on the development PC. The bridge release goes straight from 1.6.x.
   No pre-flight data check.
