# AWS Profile Manager

A desktop application for Windows and macOS that manages AWS SAML profiles and refreshes temporary AWS credentials. It replaces the Python script that performed ADFS SAML login and wrote credentials to `~/.aws/credentials`.

## Features

- **Profile management**: Add, edit, delete profiles (IdP URL, account, label, auto-refresh).
- **Dashboard**: View profile status (Active/Expired), time remaining, expiration (PST), and refresh manually.
- **SAML login**: HTTP-based ADFS login (no browser automation); uses axios, tough-cookie, cheerio, and xml2js.
- **AWS credentials file**: Reads and writes `~/.aws/credentials` (INI format) — `%USERPROFILE%\.aws\credentials` on Windows.
- **OS credential store**: Stores usernames and passwords via keytar (Windows Credential Manager / macOS Keychain); never stores plaintext locally.
- **Auto refresh**: Background scheduler refreshes credentials before expiration; runs while app is in tray.
- **System tray**: Close window to minimize to tray; tray menu: Open App, Refresh All, Pause/Resume Auto Refresh, Exit.
- **Settings**: Open credentials file, manage saved credentials, default session duration, launch at startup, start minimized.

## Tech Stack

- Electron (main process)
- React + TypeScript + Vite (renderer)
- TailwindCSS (Discord-style dark theme)
- axios + tough-cookie + axios-cookiejar-support (HTTP session)
- cheerio (HTML parsing), xml2js (SAML), @aws-sdk/client-sts (AssumeRoleWithSAML)
- keytar (Windows Credential Manager / macOS Keychain)
- ini (credentials file)

## Development

```bash
npm install
npm run dev
```

This starts the Vite dev server and Electron; the app loads the renderer from `http://localhost:5173`.

## Build

```bash
npm run build
```

Compiles main/preload (TypeScript) and renderer (Vite). Then:

```bash
npm run dist
```

to create the installer for the platform you are on: the Windows installer (NSIS) on Windows, or
`.dmg` + `.zip` for both Intel and Apple Silicon on macOS. electron-builder cannot cross-build a
mac app from Windows or vice versa; the release workflow runs one job per platform.

## macOS

Download the `.dmg` for your Mac (`arm64` for Apple Silicon, `x64` for Intel) from the Releases
page and drag the app to Applications.

**Unsigned builds.** Until a Developer ID certificate is added to the repository secrets (see
the `build-mac` job in `.github/workflows/release.yml`), releases are unsigned, which means:

- The first launch is blocked by Gatekeeper. Right-click the app → **Open** → **Open**, or run
  `xattr -d com.apple.quarantine "/Applications/AWS Profile Manager.app"`.
- **Auto-update does not work.** macOS refuses to replace a running app with an update unless both
  are signed, so an unsigned install must download each release by hand. The in-app updater
  still *detects* new versions; installing one is what fails.

Where things live on a Mac:

| | |
|---|---|
| Profiles, settings, SSO sessions | `~/Library/Application Support/AWSProfileManager/` |
| IdP username/password | login Keychain, service `AWSProfileManager` |
| AWS credentials | `~/.aws/credentials` |
| Electron userData (partitions, CLI docs cache) | `~/Library/Application Support/AWS Profile Manager/` (packaged) or `.../aws-profile-manager/` (`npm run dev`) |

`scripts/reset-app-state.sh` wipes all of the above (except `~/.aws/credentials`) for testing
first-run flows; it is the mac counterpart of `scripts/reset-app-state.ps1`.

The embedded terminal offers your login shell (zsh) and Bash. Both start as login shells so
Homebrew's `/opt/homebrew/bin` and anything in `~/.zprofile` are on the `PATH` — a GUI app
otherwise inherits a bare `PATH` without `aws` on it.

## Tray icon

Place a 16x16 or 32x32 PNG at `resources/tray-icon.png` for the system tray icon. If missing, the tray may show a default or blank icon.

## Security

- Passwords are never sent to the renderer; all auth runs in the main process.
- Credentials are stored only in the OS credential store (keytar: Windows Credential Manager, macOS Keychain).
- Profile data (no secrets) is stored in `%APPDATA%\AWSProfileManager\profiles.json` on Windows and `~/Library/Application Support/AWSProfileManager/profiles.json` on macOS.

## 🚀 Creating a GitHub Release

**The tag must match the `version` in `package.json` at the commit you tag.** Bump and
*commit* first, then tag that commit:

```bash
git checkout main
git pull

npm version 1.2.4 --no-git-tag-version   # updates package.json AND package-lock.json
git commit -am "v1.2.4"
git push                                 # push the bump BEFORE tagging

git tag v1.2.4                           # tag now carries the bumped package.json
git push origin v1.2.4                   # this push triggers the Release workflow
```

Published as **Latest**, and offered to every user by the in-app updater.

## 🚀 Creating a GitHub pre-Release

Identical steps — the only difference is the hyphen in the version, which is what makes
the workflow mark the GitHub release as a pre-release instead of promoting it to "Latest".

```bash
git checkout main
git pull

npm version 1.2.4-rc.1 --no-git-tag-version   # updates package.json AND package-lock.json
git commit -am "v1.2.4-rc.1"
git push                                      # push the bump BEFORE tagging

git tag v1.2.4-rc.1                           # tag now carries the bumped package.json
git push origin v1.2.4-rc.1                   # this push triggers the Release workflow
```

Published as a **pre-release**. Only users who enable *Settings → Debug → Developer
options → allow pre-releases* are offered it; everyone else stays on the latest stable.

## Release notes

Write them in `docs/releasenotes/<version>.md` — e.g. `docs/releasenotes/1.2.4.md` for tag
`v1.2.4` — and commit the file with the version bump. The release workflow applies it to the
GitHub release automatically.

That same text is what the app shows in its **What's new** popup on the first launch after an
update, because the popup reads the GitHub release body. No notes file means an empty body, which
means no popup — the release still ships, the workflow just logs a warning.

See `docs/releasenotes/README.md` for details.

## Release workflow notes

Pushing the tag starts `.github/workflows/release.yml`, which runs four jobs:

1. **prepare** (Linux) — checks the tag against `package.json` and creates an empty draft release
   for the tag.
2. **build-windows** — builds the NSIS installer and uploads `.exe` + `latest.yml` to that draft.
3. **build-mac** — in parallel, builds `.dmg` + `.zip` for x64 and arm64 and uploads them plus
   `latest-mac.yml`. Signs and notarizes only if the `MAC_CSC_LINK` / `APPLE_*` secrets exist;
   otherwise it warns and ships an unsigned build (see the macOS section above).
4. **publish** — once *both* builds are up, applies the release notes and flips the draft to
   published, so nobody ever sees a release with one platform's installer missing.

The draft is created up front rather than by electron-builder because two builders finishing
at different times would otherwise race to create it and one of them would end up in a second,
stray draft.

The version check runs first and fails immediately if the tag and `package.json` disagree, so
a mismatch costs seconds instead of two full builds.

If a run fails partway, re-run it from the **Actions** tab with *Run workflow* and enter
the tag — no need to delete and re-push the tag.

### Why the tag and package.json version must agree

Two different things name the GitHub release, and they read from different places:

| | Names the release from |
|---|---|
| electron-builder (looks up the draft, uploads the installers) | `"v"` + `version` in `package.json` |
| the workflow's `gh release create` / `gh release edit` | the git tag you pushed |

If they disagree you get **two releases for one version**: electron-builder's draft holding
the installer under one name, and nothing under the tag you actually pushed — so the
publish step fails and you are left cleaning up by hand.

This is why the bump has to be committed *before* the tag is created. Tagging with the
bump still staged (`git add` without `git commit`) puts the tag on a commit that still
holds the old version, which is exactly the split above.

---

## 🔽 Download

You can always grab the latest prebuilt `.exe` (Windows) or `.dmg` (macOS) from the Releases page.

### Before Running the exe on Windows
Run this command in Powershell
```bash
Unblock-File "C:\users\name\downloads\AWS-Profile-Manager-Setup-x.x.x.exe"
```


---
