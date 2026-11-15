# AWS Profile Manager 2.0 — native Rust rewrite plan (GPUI)

Status: draft, revised 2026-11-14 (switched from Tauri to GPUI). Owner: Michael.

The plan is a fully native Rust app with a GPUI interface (an earlier draft used Tauri).

## 1. Goals and non-goals

**Goals**

1. **Same features, same behaviour, same data.** A user who updates from 1.x keeps every profile,
   folder, setting, saved IdP credential, master password and (where technically possible) SSO
   session. Every 1.x feature exists in 2.0. This is the top priority.
2. **Minimal memory.** No Electron, no Node, no webview in normal use. Targets (Windows, summed
   working set of the whole process tree):
   - Hidden in tray: **≤ 25 MB**.
   - Window open, Profiles screen, idle: **≤ 80 MB**.
   - Terminal open with one shell: **≤ 100 MB** (shell processes excluded).
   - Baseline (1.6.1 installed build, 2026-11-14, after hours running): **461 MB working set /
     372 MB private** across 5 Electron processes (main 138, GPU 93, utility 58, renderers 99 + 73),
     plus ~34 MB for the terminal shell and winpty.
3. **Proven parity.** Behaviour is captured by tests against the Electron app first. The same
   scenarios then run against the Rust backend (automatically, shared data files) and as GPUI UI tests
   (one translated test per Electron UI test, tracked by ID). 2.0 ships only when every scenario
   passes on both.
4. **Seamless upgrade.** Windows users move from 1.x to 2.0 through the existing in-app updater.
5. **A refreshed UI** (unslop) that no longer reads as AI-generated, without changing features.

**Non-goals for 2.0**

- New features. Anything new waits for 2.1.
- Changing the data model, file formats or the security model.
- Linux support (GPUI supports it; not in scope).

## 2. Guiding principles

- **Tests before code.** A feature's parity tests exist and pass on Electron before its Rust port starts.
- **Backend first, UI-agnostic.** All behaviour lives in a library crate with no UI dependency. The
  GPUI app is a thin layer over it. This also isolates the project from GPUI API churn.
- **Never silently lose data.** Migration only adds files; it never rewrites or deletes a 1.x file.
  Reinstalling 1.x after 2.0 must still work.
- **Same contracts.** File paths, JSON shapes, keychain service/account names, the `v1:` ciphertext
  format and the `~/.aws/credentials` layout stay byte-compatible.
- **Security model unchanged.** Secrets stay in the backend. The UI layer receives only non-secret
  results, exactly as the renderer does today. Everything in `docs/ai-constraints.md` still applies.
- **Pin everything.** GPUI is not stable on crates.io; it is used as a git dependency pinned to one
  Zed commit and upgraded deliberately, never by accident.

## 3. Architecture of 2.0

### 3.1 Why GPUI, and what it costs

GPUI is the GPU-rendered Rust UI framework from the Zed editor (Apache-2.0). Discord is rebuilding its
desktop client on it (CTO confirmation, November 2026). Compared with a webview approach it removes
the browser engine entirely, which is where most of the remaining memory goes.

The cost: the React UI (≈13k lines) is rewritten in Rust, and features the browser gave for free
(terminal emulator, markdown, drag and drop, video, an embedded web page for SSO sign-in,
accessibility) need native replacements. Each one is listed in §3.4 with its plan.

### 3.2 Repository layout

Same repo, branch `native-2.0`. Electron code stays and keeps working until
cutover, because the parity tests run against it.

```
native/                    Cargo workspace
  crates/
    apm-core/              ALL behaviour: storage, keychain, crypto, SAML, STS, Identity Center,
                           scheduler, credentials file, AI client, CLI docs, backup, org config,
                           audit log, update checks. No UI dependency. Exposes a typed API plus
                           `dispatch(channel, json) -> json` mirroring the 1.x IPC contract (for tests).
    apm-platform/          OS integration: paths, tray, single instance, autostart, shortcuts,
                           browsers, dialogs, notifications, DPAPI/Keychain, icon tinting.
    apm-terminal/          portable-pty + alacritty_terminal state machine (no rendering).
    apm-sso-webview/       Embedded sign-in window (wry), only alive during an interactive sign-in.
    apm-ui/                GPUI views: shell, Profiles, Settings, Terminal, wizard, modals, tour,
                           splash, tray HUD, log viewer. Depends on the crates above.
    apm-app/               main(): wires everything, owns the event loop.
  tests/fixtures/          shared JSON vectors and scenarios (also read by the Electron harness)
src/                       1.x Electron app (unchanged until cutover)
tests/
  parity/                  Playwright suite against Electron (UI flows + backend scenarios)
  fakes/                   fake IdP / STS / OIDC / SSO portal / Open WebUI / GitHub (Node)
  perf/                    memory and startup measurement
```

### 3.3 Backend port map (`apm-core`, `apm-platform`)

| 1.x (TypeScript)                                   | 2.0 (Rust)                                                    | Notes |
|----------------------------------------------------|---------------------------------------------------------------|-------|
| `appPaths.ts`                                      | `apm_platform::paths`                                         | Identical paths: `%APPDATA%\AWSProfileManager`, `~/Library/Application Support/AWSProfileManager`. |
| `profileStorage.ts`, `folderStorage.ts`, `uiPrefsService.ts`, `settingsService.ts`, `rolesCache.ts` | `apm_core::store` (`serde_json`) | Preserve unknown fields (`#[serde(flatten)]`) so a 1.x reinstall loses nothing. Same migrations. Atomic writes. `ui-prefs.json` also takes the terminal panel sizes that lived in `localStorage`. |
| `credentialStorage.ts` (keytar + `v1:` AES-GCM)    | `keyring` + `aes-gcm` + `pbkdf2`                              | Same service `AWSProfileManager`, same accounts, same `v1:` envelope, PBKDF2-SHA256 100k. Cross-language vectors. No plaintext fallback. |
| `ssoTokenStore.ts` (Electron `safeStorage`)        | `apm_core::sso_store` with DPAPI / Keychain-held key          | Migration of existing sessions in §6.3. |
| `credentialsFile.ts` (`ini`)                       | own INI writer                                                | Byte-identical output to the `ini` package; golden files. |
| `awsAuthService.ts`                                | `reqwest` (cookie store) + `scraper` + `quick-xml`; STS via plain HTTPS POST | `AssumeRoleWithSAML` is unauthenticated, no SDK needed. Regex fallback kept. |
| `identityCenterService.ts`                         | `reqwest` + PKCE + `tokio` loopback server                    | Pinned port, `scopes` param, coalesced logins, scheduler never interactive. |
| `enterpriseTls.ts`                                 | `reqwest` + `rustls-platform-verifier`                        | OS trust store directly. 120 s connect timeout and `AWSProfileManager/<version>` User-Agent on the one shared client. |
| `refreshScheduler.ts`, `refreshFailureCounters.ts` | `apm_core::scheduler` (`tokio`)                               | 60 s tick, 15 min window, 2-strike pause. |
| `ptyService.ts` (node-pty)                         | `apm_terminal` (`portable-pty`, ConPTY)                       | Shell spawns on first visit to the Terminal screen. |
| `aiService.ts`                                     | `reqwest` streaming + SSE parser, `CancellationToken`         | `tool_choice: 'none'`, history trim, empty-reply error, JSON fallback. |
| `awsCliDocsService.ts`, `browserFetchService.ts`   | `reqwest` + `scraper`                                         | Same cache dir and 24 h TTL. |
| `consoleSignIn.ts`, `externalBrowser.ts`           | `apm_core::console` + `apm_platform::browsers`                | Same browser keys; never `cmd /c start`. |
| `autoUpdater.ts` (electron-updater)                | `apm_core::updater` (own)                                     | §7. GitHub releases, ed25519 signature (`minisign-verify`), pre-release toggle. |
| `changelogService.ts`, `tourService.ts`, `devFlags.ts` | ports                                                     | Same settings keys; `FORCE_WHATS_NEW` kept. |
| `configBackup.ts`, `orgConfigFile.ts`              | ports                                                         | Backup v2 byte-compatible; v1 restore supported. |
| `authAuditLog.ts`, `networkStatus.ts`, `networkErrorClassifier.ts` | ports                                         | Classifier gets a fixture-driven mapping from `reqwest` errors. |
| `appIcon.ts`, `shortcutIcon.ts`                    | `apm_platform::icons` (`windows` crate: `IShellLinkW`, `WM_SETICON`) | Runtime window + tray icon change; shortcut repointing rules kept. |
| `tray.ts`, `trayHud.ts`                            | `tray-icon` + `muda` crates; HUD is a small GPUI popup window | Badge drawn with `image`; monochrome tinting per platform. |
| `main.ts` lifecycle                                | `apm_app` + `single-instance` (named mutex / socket), `auto-launch` | Splash (GPUI window, 5 s floor), hide-to-tray destroys windows. |
| Native dialogs (`dialog.showOpenDialog` etc.)      | `rfd`                                                         | |
| `src/shared/*.ts` (profileGrouping, ssoOrg, masterPassword, orgConfig, terminalProfile) | `apm_core` modules | Renderer-side logic is ported too; vectors shared with the TS versions. |

### 3.4 UI port map (`apm-ui`, GPUI)

| 1.x (React)                                        | 2.0 (GPUI) | Plan / risk |
|----------------------------------------------------|------------|-------------|
| App shell, sidebar, custom title bar, routing      | GPUI root view with client-side title bar (Windows draws min/max/close; macOS traffic lights) | Zed does exactly this; low risk. |
| Profiles page, folders, search, context menus, `FloatingMenu` | GPUI views, `uniform_list`, popover/menu elements | Medium. |
| Drag & drop reorder (dnd-kit)                      | GPUI `on_drag` / `on_drop` with custom drop indicators | Medium; same rules (drag while filtered, folder drop targets). |
| Settings (all tabs), developer options (7 taps)    | GPUI forms | Large but straightforward. Text input via gpui-component or own `TextInput`. |
| Setup wizard (9 steps)                             | GPUI views | Medium. |
| Modals (credentials, role picker, master password, changelog) | GPUI overlay views | Low. |
| Embedded terminal (xterm.js)                       | `apm_terminal` (alacritty_terminal grid) rendered by a GPUI element | **High.** Zed's terminal view is GPL and cannot be copied into this MIT app; write our own renderer on the Apache-2.0 `alacritty_terminal` crate. Must handle colours, cursor, selection, copy/paste, resize, scrollback, IME. |
| AI assistant panel + markdown (react-markdown, remark-gfm) | `pulldown-cmark` → GPUI styled text; code blocks with "Insert into terminal" | Medium. GFM tables and lists must render; `isRunnable` rule kept. |
| Command explorer / details panel                   | GPUI views | Low–medium. |
| Feature tour (SVG spotlight, `data-tour` anchors)  | GPUI overlay drawing a dimmer with a rounded hole around an element's bounds | Medium. Anchors become element IDs registered at paint time; same rules (navigate then wait for anchor, direction-aware skip, swallow clicks, return to start route). |
| Background video (`appmedia://`, `<video>`)        | Platform decoder → frames → GPUI image: Media Foundation (Windows), AVFoundation (macOS) | **High.** No video in GPUI. Must loop, honour fragmented MP4 duration, pause when hidden. Phase 0 spike decides the approach. |
| Embedded SSO sign-in (`BrowserWindow`, `persist:sso-<hash>`) | `apm_sso_webview`: separate native window hosting `wry` (WebView2 / WKWebView) with per-org data directory / `data_store_identifier` | Medium. Only alive during sign-in, so no idle memory cost. macOS < 14 uses the external browser. |
| Auth log viewer window, tray HUD, splash           | Small GPUI windows | Low. |
| Toast notifications (`notify` event)               | GPUI toast view | Low. |
| Tooltips, icons, avatars, icon picker              | GPUI + an SVG icon set (lucide SVGs) | Low. |
| Accessibility (Chromium gives it for free)         | GPUI AccessKit integration | **High / uncertain.** AccessKit support in GPUI is new and incomplete. Phase 0 checks what works with Windows Narrator and macOS VoiceOver. |

`gpui-component` (longbridge, Apache-2.0) offers inputs, tables, lists, popovers and a markdown view.
Its compatibility with a given GPUI commit is disputed in the community, so Phase 0 decides whether to
use it (pinned) or build the handful of widgets we need ourselves.

## 4. Memory strategy

1. **No browser engine in normal use.** GPUI draws directly with DirectX 11 / Metal.
2. **Destroy windows when hidden to tray.** Only the tray, scheduler and backend remain.
3. **Lazy terminal.** PTY spawns on first visit to the Terminal screen.
4. **Webview only for SSO sign-in**, created for the sign-in and dropped right after.
5. **Video decoding only while visible**; frames are not cached beyond what playback needs.
6. **One shared `reqwest::Client`** and one `tokio` runtime.
7. **Release profile:** `opt-level = "s"`, `lto = true`, `codegen-units = 1`, `strip = true`,
   `panic = "abort"`.

Every item is verified by the perf suite (§5.7).

## 5. Test strategy

There is no test suite today. Electron is the reference: the suite is written and green against
Electron first. Because the new UI is native, UI tests cannot be shared directly; behaviour is shared
instead, in three ways.

### 5.1 Layers

| Layer | What it proves | Electron side | Rust side | How parity is enforced |
|-------|----------------|---------------|-----------|------------------------|
| A. Logic vectors | Pure functions give identical output | vitest on TS modules | `cargo test` in `apm-core` | Same JSON files in `native/tests/fixtures/vectors/` |
| B. Backend scenarios | Every IPC call: same results, same events, same files and keychain state | Playwright calls `window.electron.*` via `page.evaluate` | `cargo test` calls `apm_core::dispatch(channel, args)` | Same scenario files in `native/tests/fixtures/scenarios/` run by both runners — no translation needed |
| C. UI flows | A user can do everything they can do today | Playwright `getByRole` (role + accessible name) | Windows UI Automation against the built app (same role + name contract; proven in spike (e)); GPUI `TestAppContext` only for widget-level unit tests | **Same flow files** in `native/tests/fixtures/flows/` run by both apps — no translation step. macOS GPUI: VoiceOver/AX API runner later, or Windows-only for UI flows |
| D. Data compatibility | 2.0 reads 1.x data; 1.x still reads it after 2.0 ran | Electron writes golden data dirs | Rust reads/writes them | Round-trip tests |
| E. Platform behaviour | Tray, autostart, single instance, shortcuts, updater, proxy | scripted / checklist | scripted / checklist | Checklist §5.6 |
| F. Visual | Screens match the approved design | — | GPUI screenshots of each view | Baselines from the approved design (§9) |
| G. Performance | Memory and startup budgets | baseline numbers | enforced budgets | `tests/perf/` |

Because flows locate everything by role and accessible name, they are also an accessibility
spec: every control a flow touches must be named for screen readers in both apps. Phase 1 adds
the missing names to the 1.x React UI (invisible change); the GPUI UI must expose the same names.

### 5.2 Scenario files (layer B)

A scenario is data, so one file drives both apps:

```json
{
  "id": "BE-FOLDERS-003",
  "title": "Deleting a folder ungroups its profiles and never deletes them",
  "given": { "dataDir": "fixtures/datadirs/two-folders" },
  "steps": [
    { "call": "folders:delete", "args": ["f1"], "expect": null },
    { "call": "profiles:getAll", "expect": { "match": "profiles-ungrouped.json" } }
  ],
  "then": { "files": { "folders.json": "folders-after-delete.json" } }
}
```

Runners: `tests/parity/scenario-runner.ts` (Electron, via `window.electron`) and
`native/crates/apm-core/tests/scenarios.rs` (Rust, via `dispatch`). Events are asserted the same way
(`expectEvents: [["auth:refreshStarted", "p1"], ...]`). `src/shared/ipcContract.ts` lists all 87
request channels and 18 events; a coverage script fails if any channel has no scenario.

### 5.3 Test hooks

Added to the Electron code on the branch (local testing only, never released) and mirrored in Rust
behind a `test-hooks` cargo feature that release builds do not enable:

`APM_APPDATA_DIR`, `APM_AWS_CREDENTIALS_FILE`, `APM_KEYCHAIN_SERVICE` (per-run service name, cleaned
up), `APM_ENDPOINTS` (STS, OIDC, SSO portal, GitHub, CLI docs, console federation → fakes),
`APM_DIALOG_RESPONSES` (queued answers for file dialogs), `APM_NOW` (clock offset for the scheduler),
`APM_DISABLE_UPDATER`, `APM_SPLASH_MIN_MS=0`.

### 5.4 Fakes (`tests/fakes/`, one Node process, HTTPS with a test CA)

IdP (form login, cookies, redirects, SAMLResponse with one / many / no roles, bad password); STS
(`AssumeRoleWithSAML` success/errors/slow); OIDC + SSO portal (RegisterClient, authorize + PKCE,
CreateToken, ListAccounts, ListAccountRoles, GetRoleCredentials, expiry); Open WebUI (models,
streaming SSE, JSON fallback, `tool_calls` with null content, 403 for `User-Agent: node`); GitHub
(release notes, update feeds); AWS CLI docs pages. A proxy-mode run installs the test CA into the OS
store to prove enterprise-TLS behaviour.

### 5.5 Feature inventory

Every row gets at least one B scenario and/or C flow. This list is the parity checklist.

**Profiles** — create SAML / Identity Center, edit (top Save/Cancel, cancel-on-leave), delete, create
many, reorder by drag, search, drag while filtered; folders: create, rename, colour, icon, delete
(profiles survive), collapse persisted, move via right-click, drag into folder, dangling `folderId`
renders ungrouped, `applyLayout` single write; icon picker, avatar; dashboard expiry and role text.

**Auth** — SAML refresh with stored creds, missing creds → credentials modal, wrong password, role
picker, cached role, STS error, network down; credentials file written under `credentialProfileName`
with other profiles preserved; refresh all, auto-refresh profiles, event order; scheduler interval,
15-minute window, 2 failures → paused + event, pause/resume persisted; Identity Center sign-in
(embedded and external), session reuse per org, silent refresh, refresh-token expiry →
`ssoLoginRequired` (not a failure), sign out, list accounts, detect region, coalesced logins; console
sign-in per browser key, multi-session enable.

**Credentials and security** — default credentials set/forget/display; master password create,
unlock, wrong password, status, forget all + reset, keychain values are `v1:` when enabled; no secret
ever reaches the UI layer (assert on every API result); no secret in any log.

**Terminal and AI** — PowerShell / system shell / Git Bash (Windows path gating), resize, input,
output, colours, selection, copy/paste, scrollback; insert command from AI reply and command explorer
(writes, never runs); CLI command explorer and cache TTL; AI config status, models, streaming, stop
keeps partial text, follow-ups send the whole thread, empty reply shows `finish_reason`.

**Settings and app** — every settings field round-trips (generated from the `Settings` type);
appearance: app icon change applies live to window, tray and shortcuts; background video choose,
clear, seamless loop, fragmented MP4 full duration, pause when hidden; monochrome tray; launch at
startup; browser list; bash path picker; open credentials file; backup v2, restore v1/v2; org config
export/import; auth audit log entries, clear, viewer window; what's new: changelog once per version,
tour offered from the modal, skip banks the tour, newest unseen tour wins, missing anchors under dev
flag, return to start route, developer options; setup wizard all steps and both auth types; updates
check / download / install / pre-release toggle; window minimize / maximize / close-to-tray, single
instance focuses existing window, splash then main, sidebar collapsed persisted; tray menu, HUD,
badge colours (amber / red / gray rules); keyboard navigation and focus order.

### 5.6 Manual checklist

Real corporate network behind a TLS-inspecting proxy; real IdP and Identity Center org; Windows
Start-menu and taskbar shortcut icons after an update; light/dark taskbar tray tinting; macOS menu-bar
template icon, Dock re-open, login-shell `PATH`, Intel and Apple Silicon; screen readers (Narrator,
VoiceOver) and keyboard-only use; high-DPI and multi-monitor.

### 5.7 Performance suite

`tests/perf/measure-memory.ps1` (done) sums working set and private bytes over a process tree. Phase 1
adds scenario drivers: tray only, Profiles idle, Terminal open, 30 minutes of scheduler activity
(leak check), plus startup time to interactive. Budgets in §1 are enforced on the Rust app.

## 6. Data and upgrade compatibility

### 6.1 Byte-compatible files

`profiles.json`, `settings.json`, `folders.json`, `ui-prefs.json`, roles cache, auth audit log,
`aws-cli-docs-cache/`, `background/`, backups (v1/v2), org config files, `~/.aws/credentials`. Golden
fixtures and round trips: 1.x writes → 2.0 reads → 2.0 writes → 1.x reads.

### 6.2 Keychain

Same service and account names, same `v1:` envelope; TS↔Rust vectors both directions including the
wrong-password case. macOS may ask once to allow the new binary to read items created by the Electron
app; covered in release notes.

### 6.3 SSO sessions

1.x encrypts `sso-sessions.json` values with Electron `safeStorage` (Windows: AES-256-GCM `v10` with a
DPAPI-protected key in Electron's `Local State` under `%APPDATA%\aws-profile-manager` — Electron's
userData is named after the package name; verified by spike (g); macOS:
AES-128-CBC with a key from the "AWS Profile Manager Safe Storage" Keychain item). 2.0 decodes this
once (read only), writes `sso-sessions.v2.json` with its own protection plus the `v1:` layer, and
leaves the old file untouched. If decoding fails, the user signs in once per org; never fatal.

### 6.4 Not migrated

Embedded-browser cookies from 1.x (one SSO re-sign-in per org); terminal panel sizes from
`localStorage` (reset to default).

### 6.5 Installer, identity, autostart, shortcuts

NSIS installer built with `cargo-packager` (or a hand-written NSIS script if it falls short), same
product name and identifier. A pre-install step silently runs the 1.x uninstaller without touching app
data, removes the old login item and old shortcuts. macOS: `.app` in a dmg, plus a `.tar.gz` for the
updater.

## 7. Update path

### 7.1 From 1.x (Windows, automatic)

electron-updater downloads whatever `latest.yml` names, checks the sha512 and runs it with `/S`. The
2.0.0 GitHub release carries the native NSIS installer plus a generated `latest.yml` (version, file,
sha512, size), so 1.6.x clients take it as a normal update. Ship it to the pre-release channel first,
then stable. Keep publishing a `latest.yml` for the newest 2.x for a few months for stragglers. No
extra 1.x release is needed.

### 7.2 2.0 onwards (own updater in `apm-core`)

Hourly check of a `latest.json` (stable and pre-release feeds) on GitHub releases; download; verify an
ed25519 signature (key pair generated once, public key compiled in) and sha256; Windows runs the NSIS
installer silently and exits; macOS replaces the `.app` bundle and relaunches. Same UI states as today
(available, downloading %, downloaded, installing, error, no update).

### 7.3 macOS

1.x mac builds are unsigned, so mac users already update by hand; they download the 2.0 dmg. The own
updater verifies its own signature, so auto-update works for unsigned builds from 2.0 on (to verify
on Michael's Mac in Phase 0).

### 7.4 Rollback

Migration is additive, so reinstalling the latest 1.x keeps working. Stated in the release notes.

## 8. Build, CI and release

- Workflow: `prepare` (draft release) → `build-windows` ‖ `build-mac` (x64 + arm64) → `feeds` (generate
  `latest.json` and the bridge `latest.yml`, sign) → `publish`.
- Secrets: updater ed25519 private key; existing optional mac signing secrets.
- CI: `cargo test`, `cargo clippy -D warnings`, `cargo fmt --check` on Windows and macOS; vitest for TS
  vectors; Playwright parity suite on Electron (Windows); GPUI UI tests (both OSes); perf budgets.
- `cargo deny` for licences (the app is MIT: no GPL crates, in particular nothing from Zed's GPL
  crates) and advisories.
- Icons: `npm run icons` replaced by a Rust or Playwright script; outputs stay committed.
- At cutover: delete `src/`, `package.json` Electron deps, rebuild scripts, CJS marker script,
  `overrides` block.

## 9. UI refresh (unslop)

Because the UI is rebuilt anyway, the design is settled **before** the GPUI build, so it is built
once in its final form instead of replicated and then redesigned.

1. Run the unslop process on the current React UI in Electron, on the branch: audit every screen
   (Profiles, Settings, Terminal and AI panel, wizard, modals, tour card, splash, tray HUD, log
   viewer, title bar) against the slop signs.
2. Write the direction (product, character, world, moments, signature) and get Michael's approval.
   Expected shape: calm and exact, quiet chrome, colour reserved for status (expiry, refresh state).
3. Apply the nine passes (spacing, color, icons, copy, type, layout, details, visuals, motion) to the
   React UI. The Electron parity suite must stay green: tests locate by role, test ID and `data-tour`,
   never by class or copy that the redesign may change.
4. Extract the result as design tokens (colours, spacing scale, radii, type sizes, motion durations)
   into `native/crates/apm-ui/src/theme.rs`. The refreshed Electron screens become the visual reference
   for GPUI (layer F baselines).
5. Write the 2.0 release notes and a new tour id for 2.0.

## 10. Phases

| Phase | Work | Exit criteria |
|-------|------|---------------|
| **0. Baseline and spikes** (≈2 weeks) | Done: IPC contract (`src/shared/ipcContract.ts` + `scripts/check-ipc-contract.mjs`), memory script and 1.6.1 baseline. Spikes: (a) pinned GPUI window with custom title bar, tray via `tray-icon`, destroy-on-hide, memory measured; (b) terminal: `portable-pty` + `alacritty_terminal` grid rendered in GPUI (colours, input, resize); (c) background video frames into GPUI on Windows (Media Foundation); (d) `wry` sign-in window with a per-org data directory; (e) AccessKit / Narrator check; (f) gpui-component builds against the pinned GPUI; (g) decode a 1.x `safeStorage` value in Rust; (h) on the Mac: GPUI build, video via AVFoundation, unsigned self-update. | Every spike works or has an agreed fallback. Memory of spike (a) confirms the budgets are reachable. |
| **1. Parity suite on Electron** (≈4 weeks) | Test hooks, fakes, scenario runner, UI flows with IDs, vectors, golden data dirs, perf drivers. | Full §5.5 inventory green on Electron. |
| **2. Unslop on Electron** (≈2–3 weeks) | §9. | Direction approved; suite still green; tokens extracted. |
| **3. `apm-core` + `apm-platform`** (≈6–8 weeks) | Port in order: paths + stores → keychain + master password → credentials file → SAML + STS → scheduler → Identity Center → console → AI → CLI docs → backup / org config / audit → updater → migration. | All vectors and scenarios green in Rust on Windows and macOS. |
| **4. `apm-ui`** (≈10–14 weeks) | Shell + title bar + tray + windows → Profiles + folders + drag/drop → modals → Settings → wizard → terminal → AI panel + markdown → command explorer → tour → background video → SSO webview → log viewer, HUD, splash. | Every UI flow ID has a passing GPUI test; visual baselines match; memory budgets met. |
| **5. Migration and packaging** (≈2 weeks) | Installer, 1.x uninstall step, SSO migration, feeds, upgrade tests on clean VMs from 1.5 / 1.6 with real data. | Upgrade keeps all data; 1.x reinstall still works. |
| **6. Beta → release** | 2.0.0-beta on the pre-release channel, then stable. | No data-loss or auth regressions during beta. |

Rough total: 7–9 months part-time with Claude Code doing most of the implementation. The UI phase is
the largest and least predictable part.

### 10.1 Phase 0 results (Windows, 2026-11-14)

Memory is reported three ways by `tests/perf/measure-memory.ps1`: **Task Manager** (private working
set, the default "Memory" column), working set (includes shared system DLLs) and private bytes
(committed). Spikes live in `spikes/`.

| Spike | Result |
|-------|--------|
| (a) GPUI shell + tray (`gpui-pre` 0.3.12, `gpui-component` 0.9.1, `tray-icon`) | Works. Window open: **16 MB** Task Manager (57 MB working set), 1 process. Closing the window needs `QuitMode::Explicit` (default quits on Windows). After close + `K32EmptyWorkingSet`: **under 2 MB** Task Manager. Private bytes stay ~84 MB (committed, paged out). Exe 8.6 MB. |
| (b) Terminal (`portable-pty` + `alacritty_terminal` 0.26) | Works. PowerShell with PSReadLine colours, 256/true colour, cursor, input, resize, DSR replies. Automated round-trip test passes. Still to build: selection, copy/paste, scrollback UI, IME. |
| (c) Background video (Media Foundation → BGRA → `RenderImage`) | Works and loops with **every frame**, including a fragmented MP4 whose header declares a wrong duration (8.4 s for 10 s) — the 1.x bug cannot occur because playback loops on end of stream. GPUI text renders over it. **Too expensive as built**: ~140 MB and ~70% of one core for 720p30 (CPU colour conversion + a new texture per frame). Next: decode at window size, reuse one buffer, hardware decode; possibly a small GPUI patch for an external D3D11 texture (macOS GPUI already has `surface()`). |
| (d) Embedded SSO (`wry` 0.57 in a GPUI window) | Works, with a per-org data directory. Adds ~74 MB (WebView2 processes) only while the sign-in window is open; they exit within 4 s of closing. |
| (e) Accessibility (AccessKit in `gpui-pre`) | Works. Windows UI Automation (what Narrator uses) sees roles, names and patterns (button/Invoke, switch/Toggle, spinner/RangeValue, list/items). A Toggle performed through UI Automation changed state. This also gives a way to drive the real GPUI app in end-to-end tests (`spikes/gpui-shell/uia-probe.ps1`). |
| (f) `gpui-component` against the pinned GPUI | Builds and runs (`Root`, `TitleBar`); it pins `gpui-pre =0.3.12`, so the pair is one coherent version. |
| (g) 1.x `safeStorage` decode in Rust | Works: DPAPI key from `%APPDATA%\aws-profile-manager\Local State`, AES-256-GCM `v10`. The decrypted value carries the `v1:` master-password layer as expected. |
| (h) macOS (GPUI build, AVFoundation video, unsigned self-update) | Pending — on Michael's Mac. |

Other findings: GPUI's first **debug** build stalled (> 30 min) while release builds take ~3–4 min, so
use release (or `opt-level = 1` dev) builds; one transient rustc crash (access violation) was cured by
a rebuild. `PrintWindow(PW_RENDERFULLCONTENT)` captures GPUI windows for visual tests.

**Comparison:** 1.6.1 Electron measured at the same time (window open, terminal open, after days
running): 91 MB Task Manager / 497 MB working set / 389 MB private over 9 processes.

**Fresh-start Electron baseline** (`tests/perf/electron-memory.spec.ts`, 2026-11-14, 15 s settle,
median of 5 samples; includes the background PowerShell that 1.x starts at launch):

| State | Task Manager | Working set | Private | Processes |
|-------|-------------:|------------:|--------:|----------:|
| Profiles idle | 206 MB | 618 MB | 384 MB | 9 |
| Terminal open | 189 MB | 604 MB | 367 MB | 9 |
| Hidden to tray | 185 MB | 600 MB | 394 MB | 9 |

GPUI spike for comparison: 16 MB with the window open, under 2 MB in the tray, 1 process.

### 10.2 Phase 1 status (2026-11-14): complete

- Backend scenarios: all 87 request channels and 18 events covered; 112+ scenarios against fake
  IdP, STS, Identity Center (PKCE), Open WebUI, GitHub, AWS federation and CLI docs.
- Logic vectors: 111 (crypto, SAML roles, credentials INI editing, org config, grouping, ...).
- UI flows: 43 shared flows by role + accessible name (profiles, folders, SAML and Identity Center
  profile creation, credentials and role dialogs, master password and unlock, release notes and
  tour, wizard, AI assistant, terminal tools, settings, backup/restore, appearance).
- Bugs found and fixed in 1.x on this branch (each pinned by tests): API key sent to the renderer,
  credentials file re-serialised (comments/line endings lost, dotted names broken), SAML role
  parsing quirk, credential prompt every minute, emoji-length master passwords, stale kept-alive
  socket failures (console fallback, idle SSO calls), master-password confirm unusable from the
  keyboard, drag-and-drop reordering impossible from the keyboard. Every React control now has an
  accessible name.
- Layer D (1.x data -> 2.0 -> 1.x round trips) runs in Phase 3 once the Rust store exists; the
  byte-format rules it checks are already pinned by scenarios and vectors.

### 10.3 Phase 2 status (2026-11-14): in progress

Direction (approved): calm, exact, a little technical. Graphite neutrals; primary buttons are
off-white on dark and near-black on light; teal only for selection and focus; green / amber / red
only for credential state. Type is IBM Plex Sans with Plex Mono for figures and meta. Signature: a
status dot plus a mono countdown (`47m left`, `2d 3h left`, amber under 15 minutes), defined once in
`src/renderer/components/CredentialStatus.tsx`.

- **Light and dark themes** (added at Michael's request). Settings → Appearance → Theme: System
  (default, follows the OS live), Light, Dark. Colours are CSS variables in
  `src/renderer/theme.css`; `nativeTheme.themeSource` follows the setting so the splash, tray HUD and
  auth log viewer (`prefers-color-scheme`) and the window background match. The terminal has a palette
  per theme and switches live. Pinned by BE-SETTINGS-010 and UI-APPEARANCE-003; flows gained the
  `pressed` / `unpressed` states (UIA TogglePattern). GPUI scope: `theme.rs` carries both palettes and
  follows the OS appearance when set to System.
- Component vocabulary in `src/renderer/index.css` (`.btn-*`, `.input`, `.label`, `.card`,
  `.nav-item`, `.segmented`, `.status-dot`, `.countdown`); the GPUI port mirrors the same set.
- Profiles: one-line rows (56px) without nested cards, folders as plain sections, a subtitle with
  real counts, one primary action. Countdown fixes: never-refreshed profiles read "Not signed in"
  instead of "— left", long sessions show days, expiry drops the seconds and names the zone (PT).
- Wizard: SAML users who skip the master password are no longer sent to a credentials step that
  cannot save; the skip label says what happens.

**2.0 layout and look (decided 2026-11-14, from the mockups at
https://claude.ai/artifact/5BG8wFHPVNfGpZj55XnUe8).** The first pass above polished 1.x; the GPUI
build targets this instead.

- Look: "direction B, refined 1.x". Neutral surfaces, the 1.x blurple (#5865f2 dark, #4752c4 light)
  only on the main button and the selection, Geist and Geist Mono, 10px controls / 12px cards.
  Chosen over a graphite / Plex / teal look because that one was too close to Parallax.
- Profiles: every profile visible; each row keeps one-click actions: an icon-only console button
  (sign-in icon instead when the profile is expired or never signed in) and Refresh, both with
  tooltips; move / edit / delete in a per-row menu and the context menu. Clicking the row body opens
  an optional side panel: session timeline, copyable `AWS_PROFILE` / `--profile` snippets, sign-in
  details, recent activity.
- Two appearance settings besides Theme: **Style** (Modern, or Classic = 1.x colours) and
  **Profile list** (Compact rows, or Cards = the 1.x card list with all buttons). Classic + Cards is
  the "1.x mode". Only the Profiles list has two layouts; everything else is shared, so the extra
  cost is small and RAM is unaffected (one layout renders at a time).
- Also in the mockups, to confirm per feature before building: Ctrl+K command palette, tray menu
  with every profile's status and one-click actions, Settings as one page with section nav, Terminal
  full width with tabs per profile and the assistant / command explorer as toggled panels.

### 10.4 Phase 3 status (2026-11-14): in progress

Rust workspace in `native/` (`apm-core`). Run `cargo test -p apm-core` from `native/`.

- Vectors (layer A): all 90 cases green in Rust (`tests/vectors.rs`).
- Scenarios (layer B, `tests/scenarios.rs`): 44 of 117 green, none failing; the rest report as
  pending until their part is ported. Phase 3 is done when none are pending.
- Ported: credentials-file editor, v1 crypto, SAML role parsing, org config, SSO org helpers,
  grouping, terminal profile, master-password rules, network classifier; the JSON stores (written
  byte for byte as `JSON.stringify(x, null, 2)`), dashboard summaries, keychain (keytar-compatible
  both ways, checked against keytar) and master password / default credentials.
- Next: HTTP client with the host map and test CA, the fakes from Rust (spawn
  `tests/fakes/fakes.ts`), SAML + STS refresh, scheduler, Identity Center (with the SSO session
  store), console, AI, CLI docs, backup / org config / audit log, tour and changelog, updater.

## 11. Risks

| Risk | Mitigation |
|------|------------|
| GPUI API churn; not stable on crates.io | Git dependency pinned to one commit; upgrade in dedicated commits; UI kept thin over `apm-core`. |
| Terminal emulator quality (input, IME, selection, performance) | Built on `alacritty_terminal`; dedicated spike; terminal flows in the parity suite; manual daily-use check. |
| Background video in a native UI | Spike in Phase 0 on both OSes; frames decoded only while visible. |
| Accessibility regression vs Chromium | Phase 0 AccessKit check; keyboard navigation flows in the suite; screen-reader items on the manual checklist. |
| gpui-component incompatibility | Decide in Phase 0; fallback is our own small widget set. |
| Licence contamination (Zed GPL crates) | `cargo deny` licence policy in CI; clean-room implementation of terminal view and editor-like widgets. |
| SAML scraping differences in `reqwest` | Fake IdP mirrors the real flow; early manual test against the real IdP. |
| Corporate TLS proxy | `rustls-platform-verifier`; proxy-mode test; manual test on the corporate network. |
| WebView2 Conditional Access blocks embedded SSO | `ssoBrowserMode = 'external'` stays. |
| `safeStorage` decode fails | One SSO re-sign-in per org; never fatal. |
| Bridge update fails mid-install | Old app removed only after new files are in place; 1.x reinstall path. |
| Parity drift while 1.x keeps changing | Every 1.x fix lands with a scenario/flow first; it then fails on Rust until ported. |

## 12. Decisions

Resolved 2026-11-14:

1. **UI technology: GPUI** (native Rust), not Tauri. Backend in a UI-agnostic crate.
2. **Hidden to tray destroys windows**; backend, tray and scheduler stay alive.
3. **Lazy PTY:** spawns on first visit to the Terminal screen.
4. **macOS 14 minimum for embedded SSO**; older versions use the external browser.
5. **No 1.7.0 release.** Test hooks live on the branch for local testing only; the bridge update
   goes straight from 1.6.x.
6. **Unslop runs before the GPUI build** (§9), replacing the earlier "port first, then redesign"
   order, because the UI is rewritten anyway. Confirmed by Michael 2026-11-14.
