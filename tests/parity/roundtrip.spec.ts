/**
 * Layer D (docs/native-rewrite-plan.md §5.1, §6): 1.x and 2.0 take turns on the SAME data.
 *
 * 1.x creates data (master password, saved sign-in, settings, folders, profiles, credentials file),
 * the 2.0 core reads it and changes it, then 1.x starts again and must see exactly what 2.0 left.
 * Both sides go through the IPC contract: 1.x through window.electron, 2.0 through
 * `apm_core::dispatch` (the `dispatch` example in native/crates/apm-core/examples).
 *
 * Needs `npm run build` and `cargo build -p apm-core --example dispatch` (from native/).
 */
import { expect, test } from '@playwright/test';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { IPC_REQUESTS } from '../../src/shared/ipcContract';
import { launchElectron, REPO_ROOT, type RunningApp } from './support/electronApp';

const DISPATCH = path.join(REPO_ROOT, 'native', 'target', 'debug', 'examples', process.platform === 'win32' ? 'dispatch.exe' : 'dispatch');
const methodOf = new Map(IPC_REQUESTS.map((r) => [r.channel, r.method]));

type Call = [channel: string, args: unknown[]];

async function electron(app: RunningApp, calls: Call[]): Promise<unknown[]> {
  const out: unknown[] = [];
  for (const [channel, args] of calls) {
    const method = methodOf.get(channel);
    if (!method) throw new Error(`no window method for ${channel}`);
    const value = await app.page.evaluate(
      ({ method, args }) => (window as unknown as { electron: Record<string, (...a: unknown[]) => unknown> }).electron[method](...args),
      { method, args }
    );
    out.push(value === undefined ? null : JSON.parse(JSON.stringify(value)));
  }
  return out;
}

function rust(app: RunningApp, calls: Call[]): unknown[] {
  const input = JSON.stringify({
    appData: app.sandbox.appData,
    credentialsFile: app.sandbox.credentialsFile,
    keychainService: app.sandbox.keychainService,
    calls,
  });
  const raw = execFileSync(DISPATCH, { input, encoding: 'utf8' });
  return (JSON.parse(raw) as Array<{ ok?: unknown; error?: string }>).map((r, i) => {
    if (r.error) throw new Error(`2.0 ${calls[i][0]} failed: ${r.error}`);
    return r.ok;
  });
}

/** What both sides must agree on. Dashboard countdowns move with time, so they are left out. */
const VIEW: Call[] = [
  ['profiles:getAll', []],
  ['folders:getAll', []],
  ['settings:get', []],
  ['ui:getCollapsedFolders', []],
  ['credentials:getDefaultDisplay', []],
];

function stable(dash: unknown): unknown {
  return (dash as Array<Record<string, unknown>>).map(({ timeRemainingSeconds: _t, ...rest }) => rest);
}

test.skip(!fs.existsSync(DISPATCH), 'build the 2.0 dispatch example first: cargo build -p apm-core --example dispatch');

test('1.x data -> 2.0 changes it -> 1.x reads exactly what 2.0 left', async () => {
  const MASTER = 'correct horse battery';
  let app = await launchElectron({
    dataDir: 'two-folders',
    credentials: '# managed by hand\n[dev]\naws_access_key_id=AKIA1\naws_secret_access_key=s1\naws_session_token=t1\n\n[other-tool]\nregion=eu-west-1\n',
    files: { 'settings.json': { setupCompleted: true, seenTourIds: ['v1.5-features-ga', 'v1.6-features'], lastChangelogVersionSeen: '9.9.9' } },
  });
  try {
    // ---- 1.x writes
    await electron(app, [
      ['credentials:createMasterPassword', [MASTER, MASTER]],
      ['credentials:setDefault', ['alice@example.test', 'idp-password-1']],
      ['folders:save', [{ name: 'Ops', color: '#22c55e' }]],
      ['ui:setCollapsedFolders', [['f1']]],
    ]);
    const [settings] = (await electron(app, [['settings:get', []]])) as Array<Record<string, unknown>>;
    await electron(app, [
      ['settings:save', [{ ...settings, theme: 'light', accountDisplayNames: { '111122223333': 'Shared' }, openWebUiApiKey: 'sk-roundtrip' }]],
    ]);
    const before = await electron(app, VIEW);
    const dashBefore = stable((await electron(app, [['dashboard:getState', []]]))[0]);
    // Quit 1.x but keep its sandbox (RunningApp.close() would delete it and its keychain entries).
    await app.app.close().catch(() => undefined);

    // ---- 2.0 reads what 1.x left: same view, same keychain (unlock with the 1.x verifier)
    const [status, unlock, ...rustBefore] = rust(app, [
      ['credentials:getMasterPasswordStatus', []],
      ['credentials:unlock', [MASTER]],
      ...VIEW,
    ]);
    expect(status).toEqual({ needsUnlock: true });
    expect(unlock).toEqual({ success: true });
    expect(rustBefore).toEqual(before);
    const [rustDash] = rust(app, [['dashboard:getState', []]]);
    expect(stable(rustDash)).toEqual(dashBefore);

    // ---- 2.0 changes things
    const profiles = before[0] as Array<Record<string, unknown>>;
    const ids = profiles.map((p) => p.id as string);
    const renamed = { ...profiles.find((p) => p.id === 'p1')!, credentialProfileName: 'dev-renamed', name: 'Renamed in 2.0' };
    const rustAfter = rust(app, [
      ['credentials:unlock', [MASTER]],
      ['profiles:applyLayout', [[...ids].reverse(), { p2: null }]],
      ['profiles:save', [renamed]],
      ['folders:reorder', [[...(before[1] as Array<{ id: string }>).map((f) => f.id)].reverse()]],
      ['credentials:setDefault', ['bob@example.test', null]],
      ['ui:setCollapsedFolders', [[]]],
      ...VIEW,
    ]).slice(6);

    // ---- 1.x starts again on 2.0's data
    app = await launchElectron({}, app.sandbox);
    const [status2, unlock2] = await electron(app, [
      ['credentials:getMasterPasswordStatus', []],
      ['credentials:unlock', [MASTER]],
    ]);
    expect(status2).toEqual({ needsUnlock: true });
    expect(unlock2).toEqual({ success: true });
    const after = await electron(app, VIEW);
    expect(after).toEqual(rustAfter);
    expect(after[4]).toEqual({ username: 'bob@example.test', hasPassword: true }); // password kept
    expect((after[2] as Record<string, unknown>).openWebUiApiKeySet).toBe(true); // key never lost

    // The credentials file: 2.0 moved the section and kept everything else byte for byte.
    const credentials = fs.readFileSync(app.sandbox.credentialsFile, 'utf8');
    expect(credentials).toContain('# managed by hand');
    expect(credentials).toContain('[dev-renamed]');
    expect(credentials).not.toContain('[dev]');
    expect(credentials).toContain('[other-tool]\nregion=eu-west-1');

    // Byte identity: 1.x rewriting the same data produces the files 2.0 wrote.
    const files = ['profiles.json', 'folders.json', 'ui-prefs.json'];
    const written = Object.fromEntries(files.map((f) => [f, fs.readFileSync(path.join(app.sandbox.appData, f), 'utf8')]));
    const order = (after[0] as Array<{ id: string }>).map((p) => p.id);
    await electron(app, [
      ['profiles:applyLayout', [order, {}]],
      ['folders:reorder', [(after[1] as Array<{ id: string }>).map((f) => f.id)]],
      ['ui:setCollapsedFolders', [[]]],
    ]);
    for (const f of files) expect(fs.readFileSync(path.join(app.sandbox.appData, f), 'utf8'), f).toBe(written[f]);
  } finally {
    await app.close();
  }
});
