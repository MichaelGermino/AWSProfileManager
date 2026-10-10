import { app } from 'electron';
import os from 'os';
import path from 'path';

/**
 * Where this app keeps its own files, per platform. The ONE place that knows.
 *
 * Four services used to each carry a private copy of the Windows formula
 * (`%APPDATA%\AWSProfileManager`), which on macOS resolved to a literal
 * `~/AppData/Roaming/AWSProfileManager` — a Windows path transplanted onto a Mac.
 *
 * - Windows: `%APPDATA%\AWSProfileManager`, byte-for-byte what every shipped build has used, so
 *   existing installs keep finding their profiles. Deliberately NOT `app.getPath('userData')`,
 *   which is named after `productName` and so differs between `npm run dev` and a packaged build.
 * - macOS: `~/Library/Application Support/AWSProfileManager`.
 * - Linux: `$XDG_CONFIG_HOME/AWSProfileManager` (Electron's `appData` on Linux).
 */
const APP_NAME = 'AWSProfileManager';

export function getAppDataPath(): string {
  if (process.platform === 'win32') {
    const base = process.env.APPDATA || path.join(app.getPath('home'), 'AppData', 'Roaming');
    return path.join(base, APP_NAME);
  }
  return path.join(app.getPath('appData'), APP_NAME);
}

/**
 * The user's home directory, for `~/.aws`. `os.homedir()` already prefers `USERPROFILE` on
 * Windows and `HOME` elsewhere; the explicit `USERPROFILE` check keeps the Windows result
 * identical to the old `process.env.USERPROFILE || ''` even in the odd shell where the two differ.
 */
export function getHomeDir(): string {
  if (process.platform === 'win32' && process.env.USERPROFILE) return process.env.USERPROFILE;
  return os.homedir();
}

/** `~/.aws/credentials` — the file the AWS CLI reads on every platform. */
export function getAwsCredentialsPath(): string {
  return path.join(getHomeDir(), '.aws', 'credentials');
}
