# Tech stack

- **Electron**: ^41.5.0 (bundles Node 22). Upgraded from 33 in early 2026 to clear `npm audit` advisories.
- **Node** (host, for tooling): Node 18+ in practice; the bundled Electron Node is what runs main/preload at runtime.
- **Frontend**: React 18, react-router-dom 6. Vite ^7 for dev and production build. No separate state library (Redux, Zustand, etc.).
- **Module system**: project root is ESM (`"type": "module"`) so Vite 7's ESM-only Node API loads. `dist/main/` and `dist/preload/` get a generated `{"type":"commonjs"}` `package.json` (via `scripts/write-cjs-markers.mjs`) so the CJS-compiled main/preload still load. See CLAUDE.md.
- **Styling**: Tailwind CSS 3; PostCSS; Autoprefixer. Custom theme (e.g. discord-* classes) in Tailwind config.
- **Terminal**: node-pty ^1.1.0; xterm.js ^6.0.0; @xterm/addon-fit. One PTY per renderer WebContents; I/O via IPC.
- **AWS integration**: @aws-sdk/client-sts (AssumeRoleWithSAML). No AWS CLI binary execution; credentials written to INI file; user runs `aws` in the embedded terminal.
- **Persistence**: JSON files on disk (profiles, settings, ui-prefs, roles cache) under app data path. Keytar for IdP credentials. Renderer: one localStorage key for terminal layout. No SQLite.
- **Auth flow**: axios, axios-cookiejar-support, tough-cookie for SAML; xml2js + cheerio for parsing SAML and IdP HTML. ini for reading/writing ~/.aws/credentials.
- **AI (Terminal)**: Open WebUI–compatible API (chat/completions); URL and API key from settings; requests from main process only.
- **AWS CLI docs**: cheerio (main); fetch via Electron BrowserWindow (browserFetchService); cache in userData.
- **Build**: TypeScript 5 (tsconfig.main.json for main/preload/shared; Vite 7 for renderer). Vite config uses `@vitejs/plugin-react@^5` (paired with Vite 7; plugin-react 6+ requires Vite 8).
- **Packaging**: electron-builder ^26; output under `release/`; Windows target NSIS; icon from `resources/icon.ico`; publish to GitHub. Files included: `dist/**/*`, `resources/**/*`. `"npmRebuild": false` is set so packaging does not retry source rebuilds; native rebuilds are handled explicitly by `npm run rebuild-native` (`electron-rebuild --only=keytar`).
