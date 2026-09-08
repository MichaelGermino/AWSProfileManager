# v1.5.0 — IAM Identity Center, one-click console, and setup wizard

Adds support for AWS accounts reached through **IAM Identity Center** (federated to Entra ID)
alongside the existing SAML/ADFS profiles, one-click AWS console sign-in for any profile, and a
guided setup wizard with bulk account import.

Existing SAML profiles are unaffected — `authType` is absent on them and resolves to `saml`, so
nothing needs migrating.

---

## ✨ IAM Identity Center support

Profiles can now authenticate through IAM Identity Center instead of SAML. No Entra app
registration and no admin rights are required: the app registers itself with AWS's own OIDC
service using dynamic client registration.

- **Browser sign-in with PKCE**, then silent renewal. Once signed in, credentials refresh in the
  background with no browser and no prompts — a sign-in window only appears when the SSO session
  itself expires.
- **Bulk import**: sign in once, then pick which accounts and permission sets to create profiles
  for.
- **One session per organization**, shared by every profile in it. Eight profiles produce one
  sign-in prompt, not eight.
- Credentials land in `~/.aws/credentials` exactly as SAML profiles do, so the AWS CLI and any
  locally-developed apps are unaffected.

The scheduler **never** opens a browser. It attempts only the silent refresh path; when a session
needs a human it surfaces a prompt and waits, and this is not counted as a refresh failure.

## 🔐 One-click AWS console

A button on every profile (and a new **AWS Console** submenu in the tray) opens the AWS console
signed in as that role. Works identically for SAML and Identity Center profiles.

- Refreshes stale credentials first, so the button works even on an expired profile.
- **Several accounts open at once**: sign-in routes through AWS multi-session support
  automatically, so a second profile no longer evicts the first with "You must first log out".
- Lands on the profile's own region rather than `us-east-1`.
- Opens in your browser by default, with a picker for which installed browser to use. An in-app
  window per profile is available for keeping AWS sessions separate from everyday browsing.

## 🧭 Setup wizard

First launch now offers a guided setup, skippable at every step and re-runnable from
**Add profile → Add accounts…** or **Settings → General**.

- Asks which sign-in method you use **first**, then shows only the steps that apply — an Identity
  Center user is never asked for a master password or IdP credentials.
- **Bulk SAML import**, matching the Identity Center importer. Previously SAML profiles could only
  be created one at a time.
- Account names are looked up from AWS and used for profile names, credential section names, and
  the account display-name map.

### Organization configuration files

An admin can export a small `.json` from **Settings → General → Organization configuration** and
share it with their team; new users load it at the start of the wizard and everything is prefilled.

It carries the IdP URL, SSO start URL and region, AI assistant URL, and account display names.
It contains **no secrets** — no API keys, passwords, or credentials — and is built field by field
rather than copied from settings, so a secret cannot be included by accident.

## ⚡ Startup is roughly 8× faster

Cold start went from **~15.1s to ~1.9s** (measured on a packaged Windows build).

- The main window was signalled ready only by `ready-to-show`, which never fires for a window
  created hidden — so every launch waited out the full 15-second fallback timer.
- The splash screen enforced a 5-second minimum, adding dead time to every launch.
- The terminal screen is now mounted on first visit rather than at startup, cutting the initial
  renderer payload from ~945 kB to ~403 kB and deferring a PowerShell spawn.

`[startup]` timings are written to `startup-log.txt` in app data for diagnosing this in future.

## 🎨 Settings reorganized

The Settings page is now tabbed — **General**, **Credentials**, **Terminal & AI**, **Advanced** —
instead of one long scroll.

---

## 🐛 Fixes

- **Renaming a profile's credentials section no longer strands its credentials.** The section is
  moved in `~/.aws/credentials`, so `--profile <new name>` works immediately with no refresh.
- **Settings changes made elsewhere are no longer silently reverted.** The Settings page stayed
  mounted and saved a snapshot taken at app start, overwriting anything changed since.
- **Account display names apply immediately.** Profile labels resolve names at read time instead of
  from a snapshot taken when the role was chosen, so editing a name in Settings updates the list.
- **Profiles no longer show a live countdown for credentials that do not exist**, e.g. after
  restoring a config on a new machine. Status is cross-checked against `~/.aws/credentials`.
- **Restoring a config now removes credential sections** for profiles that no longer exist.
- The splash screen can be dragged.
- Secondary windows use the app icon rather than Electron's default.
- Master-password state now self-heals: if encrypted data exists but settings say otherwise, the
  app asks you to unlock instead of treating every stored secret as missing.

## 🔒 Security

- `npm audit` back to **0 vulnerabilities** (`@xmldom/xmldom`, `fast-uri`, `nanoid`). Lockfile-only;
  `package.json` and its load-bearing `overrides` block are untouched.
- SSO tokens are stored in a `safeStorage`-encrypted file rather than Keytar, which caps credential
  blobs at 2560 bytes — too small for SSO tokens, so they had silently failed to persist.
- Sign-in tokens for the AWS console are built and consumed entirely in the main process, never
  sent to the renderer and never logged.

## 📚 Documentation

- `docs/adr/003-identity-center-auth.md` — the authentication decisions and rejected alternatives
- `docs/identity-center-plan.md` — measured session durations and API findings
- `docs/setup-wizard-plan.md` — wizard design and the step dependency chain
- `scripts/reset-app-state.ps1` — reset to a first-run state for testing onboarding

---

## Upgrading

No action required. Existing profiles, settings, and stored credentials carry over.

New defaults apply to fresh installs only: the AWS console opens in your browser, and launch at
startup is enabled.
