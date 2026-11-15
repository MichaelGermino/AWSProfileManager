/**
 * Launches the 1.x Electron build in an isolated sandbox for the parity suite.
 *
 * Every launch gets its own temp root with app data, Electron userData, a credentials file and a
 * unique keychain service name, so tests never touch the developer's real profiles, credentials or
 * keychain. Requires `npm run build` first (the suite runs dist/, not the dev server).
 */
import { _electron as electron, type ElectronApplication, type Page } from '@playwright/test';
import { randomUUID } from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

// The repo is ESM ("type": "module"), so there is no __dirname.
export const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..', '..');
export const FIXTURES = path.join(REPO_ROOT, 'native', 'tests', 'fixtures');

export interface Sandbox {
  root: string;
  appData: string;
  userData: string;
  credentialsFile: string;
  keychainService: string;
}

export interface LaunchOptions {
  /** Folder under native/tests/fixtures/datadirs/ copied into app data before launch. */
  dataDir?: string;
  /** Extra files written into app data before launch (name -> JSON value or raw string). */
  files?: Record<string, unknown>;
  /** Initial ~/.aws/credentials content. */
  credentials?: string;
  /** Extra environment (e.g. APM_HOST_MAP, APM_TEST_CA_FILE). */
  env?: Record<string, string>;
}

export interface RunningApp {
  app: ElectronApplication;
  page: Page;
  sandbox: Sandbox;
  close(): Promise<void>;
}

function makeSandbox(): Sandbox {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'apm-parity-'));
  const sandbox: Sandbox = {
    root,
    appData: path.join(root, 'appdata'),
    userData: path.join(root, 'userdata'),
    credentialsFile: path.join(root, 'aws', 'credentials'),
    keychainService: `AWSProfileManager-test-${randomUUID().slice(0, 8)}`,
  };
  fs.mkdirSync(sandbox.appData, { recursive: true });
  fs.mkdirSync(sandbox.userData, { recursive: true });
  fs.mkdirSync(path.dirname(sandbox.credentialsFile), { recursive: true });
  return sandbox;
}

function seed(sandbox: Sandbox, opts: LaunchOptions): void {
  if (opts.dataDir) {
    fs.cpSync(path.join(FIXTURES, 'datadirs', opts.dataDir), sandbox.appData, { recursive: true });
  }
  for (const [name, value] of Object.entries(opts.files ?? {})) {
    const target = path.join(sandbox.appData, name);
    fs.mkdirSync(path.dirname(target), { recursive: true });
    fs.writeFileSync(target, typeof value === 'string' ? value : JSON.stringify(value, null, 2));
  }
  if (opts.credentials !== undefined) fs.writeFileSync(sandbox.credentialsFile, opts.credentials);
}

/** Main window = the one that loads the renderer, not the splash. */
async function mainWindow(app: ElectronApplication): Promise<Page> {
  const deadline = Date.now() + 30_000;
  while (Date.now() < deadline) {
    for (const w of app.windows()) {
      if (w.url().includes('renderer') || w.url().startsWith('http://localhost')) return w;
    }
    await app.waitForEvent('window', { timeout: 1_000 }).catch(() => undefined);
  }
  throw new Error('main window did not open');
}

/** Close the app and start it again on the same sandbox (persistence checks). */
export async function restartElectron(running: RunningApp, opts: LaunchOptions = {}): Promise<RunningApp> {
  await running.app.close().catch(() => undefined);
  return launchElectron({ ...opts, dataDir: undefined, files: undefined, credentials: undefined }, running.sandbox);
}

export async function launchElectron(opts: LaunchOptions = {}, existing?: Sandbox): Promise<RunningApp> {
  const sandbox = existing ?? makeSandbox();
  if (!existing) seed(sandbox, opts);
  const env: Record<string, string> = {
    ...(process.env as Record<string, string>),
    APM_TEST_MODE: '1',
    APM_APPDATA_DIR: sandbox.appData,
    APM_USERDATA_DIR: sandbox.userData,
    APM_AWS_CREDENTIALS_FILE: sandbox.credentialsFile,
    APM_KEYCHAIN_SERVICE: sandbox.keychainService,
    APM_SPLASH_MIN_MS: '0',
    // Never let the developer's .env AI settings leak into a test.
    AWS_TERMINAL_AI_API_URL: '',
    AWS_TERMINAL_AI_API_KEY: '',
    AWS_TERMINAL_AI_MODEL: '',
    ...opts.env,
  };
  delete env.NODE_ENV;
  delete env.ELECTRON_RUN_AS_NODE;

  // cwd = sandbox root, so dotenv in main finds no .env (the repo's .env holds real keys).
  const app = await electron.launch({ args: [REPO_ROOT], cwd: sandbox.root, env });
  const page = await mainWindow(app);
  await page.waitForFunction(() => typeof (window as unknown as { electron?: unknown }).electron === 'object');

  return {
    app,
    page,
    sandbox,
    async close() {
      await deleteTestKeychainEntries(app, sandbox.keychainService).catch(() => undefined);
      await app.close().catch(() => undefined);
      fs.rmSync(sandbox.root, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
    },
  };
}

/** keytar is built for Electron's ABI, so clean up from inside the main process. */
async function deleteTestKeychainEntries(app: ElectronApplication, service: string): Promise<void> {
  if (!service.startsWith('AWSProfileManager-test-')) return; // never touch the real service
  await app.evaluate(async (_electron, svc) => {
    const req = (process as unknown as { mainModule: NodeJS.Module }).mainModule.require;
    const keytar = req('keytar') as typeof import('keytar');
    for (const c of await keytar.findCredentials(svc)) await keytar.deletePassword(svc, c.account);
  }, service);
}

/** Keychain contents for the sandbox's service (account -> value), read inside main. */
export async function readKeychain(running: RunningApp): Promise<Record<string, string>> {
  return running.app.evaluate(async (_electron, svc) => {
    const req = (process as unknown as { mainModule: NodeJS.Module }).mainModule.require;
    const keytar = req('keytar') as typeof import('keytar');
    const out: Record<string, string> = {};
    for (const c of await keytar.findCredentials(svc)) out[c.account] = c.password;
    return out;
  }, running.sandbox.keychainService);
}
