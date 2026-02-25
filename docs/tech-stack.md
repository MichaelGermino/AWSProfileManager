# Tech stack

- **Electron**: ^33.2.0 (package.json).
- **Node**: No explicit engine in package.json; build and dev assume a Node that supports the used APIs (fs, path, require, ES modules where used). electron-builder and node-pty may impose practical minimums (e.g. Node 18+).
- **Frontend**: React 18, react-router-dom 6. Vite 5 for dev and production build. No separate state library (Redux, Zustand, etc.).
- **Styling**: Tailwind CSS 3; PostCSS; Autoprefixer. Custom theme (e.g. discord-* classes) in Tailwind config.
- **Terminal**: node-pty ^1.1.0; xterm.js ^6.0.0; @xterm/addon-fit. One PTY per renderer WebContents; I/O via IPC.
- **AWS integration**: @aws-sdk/client-sts (AssumeRoleWithSAML). No AWS CLI binary execution; credentials written to INI file; user runs `aws` in the embedded terminal.
- **Persistence**: JSON files on disk (profiles, settings, ui-prefs, roles cache) under app data path. Keytar for IdP credentials. Renderer: one localStorage key for terminal layout. No SQLite.
- **Auth flow**: axios, axios-cookiejar-support, tough-cookie for SAML; xml2js + cheerio for parsing SAML and IdP HTML. ini for reading/writing ~/.aws/credentials.
- **AI (Terminal)**: Open WebUI–compatible API (chat/completions); URL and API key from settings; requests from main process only.
- **AWS CLI docs**: cheerio (main); fetch via Electron BrowserWindow (browserFetchService); cache in userData.
- **Build**: TypeScript 5 (tsconfig.main.json for main/preload/shared; Vite for renderer). Vite config uses @vitejs/plugin-react.
- **Packaging**: electron-builder; output under `release/`; Windows target NSIS; icon from `resources/icon.ico`; publish to GitHub. Files included: `dist/**/*`, `resources/**/*`.
