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
