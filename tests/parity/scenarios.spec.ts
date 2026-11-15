/**
 * Electron runner for the shared backend scenarios in native/tests/fixtures/scenarios/**.json.
 * Each scenario becomes one test. Calls go through window.electron (the real preload bridge),
 * mapped from channel names by src/shared/ipcContract.ts, so they exercise the same path as the UI.
 */
import { expect, test } from '@playwright/test';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { IPC_EVENTS, IPC_REQUESTS } from '../../src/shared/ipcContract';
import {
  FIXTURES,
  installEventRecorder,
  launchElectron,
  queueDialogs,
  readKeychain,
  restartElectron,
  seedKeychain,
  takeEvents,
  type RunningApp,
} from './support/electronApp';
import {
  match,
  materializeSecret,
  resolveRefs,
  withTokens,
  type FileExpectation,
  type Refs,
  type Scenario,
} from './support/scenario';
import { startFakes, TEST_CA_FILE, type Fakes, type FakesConfig } from '../fakes/fakes';

const SCENARIO_DIR = path.join(FIXTURES, 'scenarios');

function loadScenarios(dir: string): Array<{ file: string; scenario: Scenario }> {
  if (!fs.existsSync(dir)) return [];
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const full = path.join(dir, e.name);
    if (e.isDirectory()) return loadScenarios(full);
    if (!e.name.endsWith('.json')) return [];
    const parsed = JSON.parse(fs.readFileSync(full, 'utf8')) as Scenario | Scenario[];
    return (Array.isArray(parsed) ? parsed : [parsed]).map((scenario) => ({ file: full, scenario }));
  });
}

const methodByChannel = new Map(
  IPC_REQUESTS.filter((r) => r.method && (r.window ?? 'main') === 'main').map((r) => [r.channel, r.method!])
);
const recordedEvents = IPC_EVENTS.filter((e) => e.method && (e.window ?? 'main') === 'main').map((e) => ({
  channel: e.channel,
  method: e.method!,
}));

async function call(running: RunningApp, channel: string, args: unknown[]): Promise<unknown> {
  const method = methodByChannel.get(channel);
  if (!method) throw new Error(`no window.electron method for channel ${channel}`);
  const result = await running.page.evaluate(
    async ({ method, args }) => {
      const fn = (window as unknown as { electron: Record<string, (...a: unknown[]) => unknown> }).electron[method];
      return await fn(...args);
    },
    { method, args }
  );
  // Electron resolves void handlers with undefined, and structured clone keeps keys whose value is
  // undefined. The shared format (and Rust) is JSON: null for "nothing", such keys absent.
  return result === undefined ? null : JSON.parse(JSON.stringify(result));
}

function checkFile(label: string, filePath: string, exp: FileExpectation, refs: Refs): string[] {
  const exists = fs.existsSync(filePath);
  if ('absent' in exp) return exists ? [`${label}: expected file to be absent`] : [];
  if (!exists) return [`${label}: file missing`];
  const text = fs.readFileSync(filePath, 'utf8');
  if ('text' in exp) {
    // {EOL} = the platform line ending (the ini package writes os.EOL: CRLF on Windows).
    const want = exp.text.replaceAll('{EOL}', os.EOL);
    return text === want ? [] : [`${label}: text differs\n  expected ${JSON.stringify(want)}\n  actual   ${JSON.stringify(text)}`];
  }
  if ('contains' in exp || 'notContains' in exp) {
    const c = exp as { contains?: string[]; notContains?: string[] };
    return [
      ...(c.contains ?? []).filter((s) => !text.includes(s)).map((s) => `${label}: missing ${JSON.stringify(s)}`),
      ...(c.notContains ?? []).filter((s) => text.includes(s)).map((s) => `${label}: must not contain ${JSON.stringify(s)}`),
    ];
  }
  return match(JSON.parse(text), resolveRefs(exp.json, refs), label);
}

async function start(scenario: Scenario, fakes: Fakes | null): Promise<RunningApp> {
  const given = (fakes ? withTokens(scenario.given ?? {}, { FAKES: fakes.baseUrl }) : scenario.given ?? {}) as Scenario['given'] & object;
  const env = fakes ? { APM_HOST_MAP: JSON.stringify(fakes.hostMap()), APM_TEST_CA_FILE: TEST_CA_FILE } : undefined;
  let running = await launchElectron({ ...given, env });
  if (given.keychain) {
    // Keychain is only reachable from inside Electron; seed, then restart so the app starts with it.
    const entries = Object.fromEntries(Object.entries(given.keychain).map(([k, v]) => [k, materializeSecret(v)]));
    await seedKeychain(running, entries);
    running = await restartElectron(running, { env });
  }
  await installEventRecorder(running.page, recordedEvents);
  return running;
}

for (const { file, scenario } of loadScenarios(SCENARIO_DIR)) {
  const applies = !scenario.platforms || scenario.platforms.includes(process.platform as 'win32' | 'darwin');
  test(`${scenario.id} ${scenario.title}`, async () => {
    test.skip(!applies, `not for ${process.platform}`);
    test.info().annotations.push({ type: 'file', description: path.relative(SCENARIO_DIR, file) });
    const fakes = scenario.given?.fakes ? await startFakes(scenario.given.fakes as FakesConfig) : null;
    const env = fakes ? { APM_HOST_MAP: JSON.stringify(fakes.hostMap()), APM_TEST_CA_FILE: TEST_CA_FILE } : undefined;
    let running = await start(scenario, fakes);
    const refs: Refs = {};
    const tok = (v: unknown) => withTokens(v, { SANDBOX: running.sandbox.root, ...(fakes ? { FAKES: fakes.baseUrl } : {}) });
    try {
      for (const [i, step] of scenario.steps.entries()) {
        const label = `step ${i + 1}`;
        if ('restart' in step) {
          running = await restartElectron(running, { env });
          await installEventRecorder(running.page, recordedEvents);
          continue;
        }
        if ('dialogs' in step) {
          await queueDialogs(running, tok(step.dialogs) as Array<string | null>);
          continue;
        }
        if ('expectEvents' in step) {
          const got = await takeEvents(running.page);
          const problems = match(got, tok(resolveRefs(step.expectEvents, refs)), `${label} events`);
          expect(problems, problems.join('\n')).toEqual([]);
          continue;
        }
        const args = tok(resolveRefs(step.args ?? [], refs)) as unknown[];
        const result = await call(running, step.call, args);
        if (step.save) refs[step.save] = result;
        if ('expect' in step) {
          const problems = match(result, tok(resolveRefs(step.expect, refs)), `${label} ${step.call}`);
          expect(problems, problems.join('\n')).toEqual([]);
        }
      }
      const then = scenario.then ?? {};
      const problems: string[] = [];
      for (const [name, exp] of Object.entries(then.files ?? {})) {
        problems.push(...checkFile(name, path.join(running.sandbox.appData, name), tok(exp) as FileExpectation, refs));
      }
      for (const [name, exp] of Object.entries(then.sandboxFiles ?? {})) {
        problems.push(...checkFile(name, path.join(running.sandbox.root, name), tok(exp) as FileExpectation, refs));
      }
      if (then.credentials) problems.push(...checkFile('credentials', running.sandbox.credentialsFile, tok(then.credentials) as FileExpectation, refs));
      if (then.keychain) {
        const kc = await readKeychain(running);
        for (const [account, exp] of Object.entries(then.keychain)) {
          if (exp === '$absent') {
            if (account in kc) problems.push(`keychain ${account}: expected absent`);
          } else if (!(account in kc)) {
            problems.push(`keychain ${account}: missing`);
          } else {
            problems.push(...match(kc[account], resolveRefs(exp, refs), `keychain ${account}`));
          }
        }
      }
      for (const { service, expect: want } of then.fakeRequests ?? []) {
        const got = (fakes?.requests ?? []).filter((r) => r.service === service);
        problems.push(...match(got, tok(resolveRefs(want, refs)), `fake ${service} requests`));
      }
      expect(problems, problems.join('\n')).toEqual([]);
    } finally {
      await running.close();
      await fakes?.close();
    }
  });
}
