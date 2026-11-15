import { app } from 'electron';

/**
 * Seams for the parity test suite (docs/native-rewrite-plan.md §5.3). Everything here is inert
 * unless APM_TEST_MODE=1 AND the app is unpackaged, so an installed build ignores these variables.
 * The Rust app mirrors the same variables behind its `test-hooks` cargo feature.
 *
 * - APM_APPDATA_DIR           replaces %APPDATA%\AWSProfileManager (profiles, settings, folders, ...)
 * - APM_USERDATA_DIR          replaces Electron's userData (Local State, partitions, caches)
 * - APM_AWS_CREDENTIALS_FILE  replaces ~/.aws/credentials
 * - APM_KEYCHAIN_SERVICE      replaces the keychain service name 'AWSProfileManager'
 * - APM_HOST_MAP              JSON {"oidc.us-east-1.amazonaws.com": "https://127.0.0.1:9443/oidc", ...}
 *                             sends requests for a real host to a fake server base URL instead
 * - APM_TEST_CA_FILE          PEM file trusted in addition to the OS store (the fakes' test CA)
 * - APM_SPLASH_MIN_MS         splash floor (tests use 0)
 */
export const TEST_MODE = process.env.APM_TEST_MODE === '1' && !app.isPackaged;

function testEnv(name: string): string | undefined {
  if (!TEST_MODE) return undefined;
  const v = process.env[name];
  return v && v.trim() ? v.trim() : undefined;
}

export const testAppDataDir = (): string | undefined => testEnv('APM_APPDATA_DIR');
export const testCredentialsFile = (): string | undefined => testEnv('APM_AWS_CREDENTIALS_FILE');
export const testKeychainService = (): string | undefined => testEnv('APM_KEYCHAIN_SERVICE');
export const testCaFile = (): string | undefined => testEnv('APM_TEST_CA_FILE');

export function testSplashMinMs(): number | undefined {
  const v = testEnv('APM_SPLASH_MIN_MS');
  return v === undefined ? undefined : Number(v);
}

/** Must run before app.requestSingleInstanceLock(): the lock is keyed on userData. */
export function applyTestUserData(): void {
  const dir = testEnv('APM_USERDATA_DIR');
  if (dir) app.setPath('userData', dir);
}

let hostMap: Record<string, string> | null = null;
function getHostMap(): Record<string, string> {
  if (hostMap) return hostMap;
  const raw = testEnv('APM_HOST_MAP');
  try {
    hostMap = raw ? (JSON.parse(raw) as Record<string, string>) : {};
  } catch {
    hostMap = {};
  }
  return hostMap;
}

/** Fake base URL for a real host, or undefined (always undefined outside test mode). */
export function testBaseUrlForHost(host: string): string | undefined {
  return getHostMap()[host.toLowerCase()];
}

/** For https.request({ host, path }) callers: where the request should really go. */
export function remapHostPath(host: string, path: string): { hostname: string; port?: number; path: string } {
  const base = testBaseUrlForHost(host);
  if (!base) return { hostname: host, path };
  const u = new URL(base);
  return {
    hostname: u.hostname,
    port: u.port ? Number(u.port) : undefined,
    path: u.pathname.replace(/\/$/, '') + path,
  };
}

/** For URL-string callers (axios, BrowserWindow.loadURL, shell.openExternal). */
export function remapUrl(url: string): string {
  if (!TEST_MODE) return url;
  try {
    const u = new URL(url);
    const base = testBaseUrlForHost(u.host);
    if (!base) return url;
    return base.replace(/\/$/, '') + u.pathname + u.search + u.hash;
  } catch {
    return url;
  }
}

// The parity suite reaches main-process modules (keytar) through electronApp.evaluate, whose code
// cannot `require` on its own. Expose this module's require, in test mode only.
if (TEST_MODE) {
  (globalThis as unknown as { __apmTestRequire?: NodeRequire }).__apmTestRequire = require;
}
