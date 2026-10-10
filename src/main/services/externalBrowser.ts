import { spawn } from 'child_process';
import { existsSync } from 'fs';
import os from 'os';
import path from 'path';
import { shell } from 'electron';

/**
 * Opening a URL in a specific installed browser.
 *
 * Private/incognito windows are deliberately NOT offered. The AWS console keeps one session per
 * browser session context, and multi-session opt-in lives in a cookie a private window discards —
 * so a private window can only ever hold one AWS account, which defeats the point.
 *
 * ⚠️ Always spawn the browser executable DIRECTLY with an args array. Going through
 * `cmd /c start` makes cmd treat '&' as a command separator, silently truncating a URL at its
 * first query parameter. A direct spawn also avoids ShellExecute's URL length limit.
 */

interface BrowserDef {
  key: string;
  name: string;
  /** Windows: candidate .exe paths. macOS: candidate .app bundle paths (launched via `open -a`). */
  paths: string[];
}

/** A browser found on this machine, for the Settings picker. */
export interface InstalledBrowser {
  key: string;
  name: string;
}

/**
 * macOS bundles. Keys match the Windows list so a settings.consoleBrowser value means the same
 * browser on both. ~/Applications is where a non-admin install lands.
 */
function macApp(name: string): string[] {
  return [path.join('/Applications', name), path.join(os.homedir(), 'Applications', name)];
}

const MAC_BROWSERS: BrowserDef[] = [
  { key: 'chrome', name: 'Google Chrome', paths: macApp('Google Chrome.app') },
  { key: 'edge', name: 'Microsoft Edge', paths: macApp('Microsoft Edge.app') },
  { key: 'firefox', name: 'Mozilla Firefox', paths: macApp('Firefox.app') },
  { key: 'safari', name: 'Safari', paths: ['/Applications/Safari.app'] },
];

const WINDOWS_BROWSERS: BrowserDef[] = [
  {
    key: 'edge',
    name: 'Microsoft Edge',
    paths: [
      'C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe',
      'C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe',
    ],
  },
  {
    key: 'chrome',
    name: 'Google Chrome',
    paths: [
      'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe',
      'C:\\Program Files (x86)\\Google\\Chrome\\Application\\chrome.exe',
      `${process.env.LOCALAPPDATA ?? ''}\\Google\\Chrome\\Application\\chrome.exe`,
    ],
  },
  {
    key: 'firefox',
    name: 'Mozilla Firefox',
    paths: [
      'C:\\Program Files\\Mozilla Firefox\\firefox.exe',
      'C:\\Program Files (x86)\\Mozilla Firefox\\firefox.exe',
    ],
  },
];

const BROWSERS: BrowserDef[] =
  process.platform === 'win32' ? WINDOWS_BROWSERS : process.platform === 'darwin' ? MAC_BROWSERS : [];

function resolve(b: BrowserDef): (BrowserDef & { exe: string }) | null {
  const exe = b.paths.find((p) => p && existsSync(p));
  return exe ? { ...b, exe } : null;
}

/** Browsers actually installed, in preference order. Empty on Linux. */
export function listInstalledBrowsers(): InstalledBrowser[] {
  return BROWSERS.map(resolve)
    .filter((b): b is BrowserDef & { exe: string } => b !== null)
    .map(({ key, name }) => ({ key, name }));
}

/** The requested browser if installed, else the first one that is. */
function findBrowser(preferredKey?: string): (BrowserDef & { exe: string }) | null {
  if (preferredKey && preferredKey !== 'default') {
    const wanted = BROWSERS.find((b) => b.key === preferredKey);
    const hit = wanted ? resolve(wanted) : null;
    if (hit) return hit;
    // Chosen browser was uninstalled since; fall through rather than failing outright.
  }
  for (const b of BROWSERS) {
    const hit = resolve(b);
    if (hit) return hit;
  }
  return null;
}

/**
 * Launch a specific browser with a URL. Windows runs the executable directly; macOS goes
 * through `open -a <bundle> <url>`, which is how a .app is launched from outside Finder and
 * passes the URL as a plain argv entry — no shell, so '&' in the query string is safe.
 */
function launch(browser: BrowserDef & { exe: string }, url: string): void {
  const child =
    process.platform === 'darwin'
      ? spawn('open', ['-a', browser.exe, url], { detached: true, stdio: 'ignore' })
      : spawn(browser.exe, [url], { detached: true, stdio: 'ignore' });
  child.unref();
}

/**
 * Open `url` in a browser, optionally private.
 *
 * Spawning the executable directly also sidesteps a length limit: `shell.openExternal` goes
 * through ShellExecute, which truncates long URLs, and the multi-session console URL runs to
 * ~3300 characters. A direct spawn passes it as an argv entry (32 KB limit), so it survives.
 *
 * Falls back to the default browser when no known browser is installed, so the action still does
 * something rather than silently failing.
 */
export function openInBrowser(
  url: string,
  options: { browserKey?: string } = {}
): { ok: true; via: string } | { ok: false; error: string } {
  // 'default' means "whatever the OS is set to", which shell.openExternal handles.
  const browser = options.browserKey && options.browserKey !== 'default'
    ? findBrowser(options.browserKey)
    : null;

  if (!browser) {
    void shell.openExternal(url);
    return { ok: true, via: 'default browser' };
  }

  try {
    launch(browser, url);
    return { ok: true, via: browser.name };
  } catch (err) {
    return { ok: false, error: err instanceof Error ? err.message : String(err) };
  }
}
