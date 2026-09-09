# AWS Profile Manager

A desktop application for Windows that manages AWS SAML profiles and refreshes temporary AWS credentials. It replaces the Python script that performed ADFS SAML login and wrote credentials to `~/.aws/credentials`.

## Features

- **Profile management**: Add, edit, delete profiles (IdP URL, account, label, auto-refresh).
- **Dashboard**: View profile status (Active/Expired), time remaining, expiration (PST), and refresh manually.
- **SAML login**: HTTP-based ADFS login (no browser automation); uses axios, tough-cookie, cheerio, and xml2js.
- **AWS credentials file**: Reads and writes `%USERPROFILE%\.aws\credentials` (INI format).
- **Windows Credential Manager**: Stores usernames and passwords via keytar; never stores plaintext locally.
- **Auto refresh**: Background scheduler refreshes credentials before expiration; runs while app is in tray.
- **System tray**: Close window to minimize to tray; tray menu: Open App, Refresh All, Pause/Resume Auto Refresh, Exit.
- **Settings**: Open credentials file, manage saved credentials, default session duration, launch at startup, start minimized.

## Tech Stack

- Electron (main process)
- React + TypeScript + Vite (renderer)
- TailwindCSS (Discord-style dark theme)
- axios + tough-cookie + axios-cookiejar-support (HTTP session)
- cheerio (HTML parsing), xml2js (SAML), @aws-sdk/client-sts (AssumeRoleWithSAML)
- keytar (Windows Credential Manager)
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

to create the Windows installer (NSIS).

## Tray icon

Place a 16x16 or 32x32 PNG at `resources/tray-icon.png` for the system tray icon. If missing, the tray may show a default or blank icon.

## Security

- Passwords are never sent to the renderer; all auth runs in the main process.
- Credentials are stored only in Windows Credential Manager (keytar).
- Profile data (no secrets) is stored in `%APPDATA%\AWSProfileManager\profiles.json`.

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

## Release workflow notes

Pushing the tag starts `.github/workflows/release.yml`, which builds the NSIS installer,
uploads `.exe` + `latest.yml` to a draft release, then publishes it.

Its first step checks the tag against `package.json` and fails immediately if they
disagree, so a mismatch costs seconds instead of a full Windows build.

If a run fails partway, re-run it from the **Actions** tab with *Run workflow* and enter
the tag — no need to delete and re-push the tag.

### Why the tag and package.json version must agree

Two different things name the GitHub release, and they read from different places:

| | Names the release from |
|---|---|
| electron-builder (creates the draft, uploads the `.exe`) | `"v"` + `version` in `package.json` |
| the workflow's `gh release edit` (publishes it) | the git tag you pushed |

If they disagree you get **two releases for one version**: electron-builder's draft holding
the installer under one name, and nothing under the tag you actually pushed — so the
publish step fails and you are left cleaning up by hand.

This is why the bump has to be committed *before* the tag is created. Tagging with the
bump still staged (`git add` without `git commit`) puts the tag on a commit that still
holds the old version, which is exactly the split above.

---

## 🔽 Download

You can always grab the latest prebuilt `.exe` from the Releases page.

### Before Running the exe on Windows
Run this command in Powershell
```bash
Unblock-File "C:\users\name\downloads\AWS-Profile-Manager-Setup-x.x.x.exe"
```


---
