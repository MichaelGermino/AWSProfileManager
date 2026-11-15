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
  installExternalOpenCapture,
  takeExternalOpens,
  launchElectron,
  queueDialogs,
  readKeychain,
  readStream,
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
/** The running 1.x version, for release-notes scenarios ({APP_VERSION}). */
const APP_VERSION = (JSON.parse(fs.readFileSync(path.join(FIXTURES, '..', '..', '..', 'package.json'), 'utf8')) as { version: string }).version;

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
  const outcome = await running.page.evaluate(
    async ({ method, args }) => {
      const fn = (window as unknown as { electron: Record<string, (...a: unknown[]) => unknown> }).electron[method];
      try {
        return { ok: true as const, value: await fn(...args) };
      } catch (err) {
        return { ok: false as const, error: err instanceof Error ? err.message : String(err) };
      }
    },
    { method, args }
  );
  if (!outcome.ok) {
    // Electron wraps handler errors: "Error invoking remote method 'x': Error: <message>".
    return { failed: outcome.error.replace(/^Error invoking remote method '[^']+': (?:\w*Error: )?/, '') };
  }
  // Electron resolves void handlers with undefined, and structured clone keeps keys whose value is
  // undefined. The shared format (and Rust) is JSON: null for "nothing", such keys absent.
  const result = outcome.value;
  return { value: result === undefined ? null : JSON.parse(JSON.stringify(result)) };
}

type CallOutcome = { value: unknown } | { failed: string };

function checkOutcome(outcome: CallOutcome, step: { expect?: unknown; expectError?: unknown }, label: string, tok: (v: unknown) => unknown, refs: Refs): string[] {
  if ('failed' in outcome) {
    if (!('expectError' in step)) return [`${label}: call failed: ${outcome.failed}`];
    return match(outcome.failed, tok(resolveRefs(step.expectError, refs)), `${label} error`);
  }
  if ('expectError' in step) return [`${label}: expected the call to fail, got ${JSON.stringify(outcome.value)}`];
  if (!('expect' in step)) return [];
  return match(outcome.value, tok(resolveRefs(step.expect, refs)), label);
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
  await installExternalOpenCapture(running.app);
  return running;
}

for (const { file, scenario } of loadScenarios(SCENARIO_DIR)) {
  const applies = !scenario.platforms || scenario.platforms.includes(process.platform as 'win32' | 'darwin');
  test(`${scenario.id} ${scenario.title}`, async () => {
    test.skip(!applies, `not for ${process.platform}`);
    test.info().annotations.push({ type: 'file', description: path.relative(SCENARIO_DIR, file) });
    const fakes = scenario.given?.fakes ? await startFakes(withTokens(scenario.given.fakes, { APP_VERSION }) as FakesConfig) : null;
    const env = fakes ? { APM_HOST_MAP: JSON.stringify(fakes.hostMap()), APM_TEST_CA_FILE: TEST_CA_FILE } : undefined;
    let running = await start(scenario, fakes);
    const refs: Refs = {};
    const inflight = new Map<string, Promise<CallOutcome>>();
    const tok = (v: unknown) => withTokens(v, { SANDBOX: running.sandbox.root, APP_VERSION, ...(fakes ? { FAKES: fakes.baseUrl } : {}) });
    try {
      for (const [i, step] of scenario.steps.entries()) {
        const label = `step ${i + 1}`;
        if ('restart' in step) {
          running = await restartElectron(running, { env });
          await installEventRecorder(running.page, recordedEvents);
          await installExternalOpenCapture(running.app);
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
        if ('expectExternalOpens' in step) {
          if (step.waitMs) await new Promise((r) => setTimeout(r, step.waitMs));
          const got = await takeExternalOpens(running.app);
          const problems = match(got, tok(resolveRefs(step.expectExternalOpens, refs)), `${label} external opens`);
          expect(problems, problems.join('\n')).toEqual([]);
          continue;
        }
        if ('expectStream' in step) {
          const got = await readStream(running.page, step.expectStream);
          const problems = match(got, tok(resolveRefs(step.expect, refs)), `${label} stream ${step.expectStream}`);
          expect(problems, problems.join('\n')).toEqual([]);
          continue;
        }
        if ('waitForStream' in step) {
          const deadline = Date.now() + (step.timeoutMs ?? 15_000);
          for (;;) {
            const items = await readStream(running.page, step.waitForStream);
            const text = items.map((x) => (typeof x === 'string' ? x : JSON.stringify(x))).join('');
            if ((step.count === undefined || items.length >= step.count) && (step.text === undefined || text.includes(step.text))) break;
            if (Date.now() > deadline) throw new Error(`${label}: stream ${step.waitForStream} did not reach the condition; got ${JSON.stringify(text.slice(-400))}`);
            await new Promise((r) => setTimeout(r, 100));
          }
          continue;
        }
        if ('await' in step) {
          const pending = inflight.get(step.await);
          if (!pending) throw new Error(`${label}: nothing started as "${step.await}"`);
          const outcome = await pending;
          if (step.save) refs[step.save] = 'value' in outcome ? outcome.value : outcome;
          const problems = checkOutcome(outcome, step, `${label} await ${step.await}`, tok, refs);
          expect(problems, problems.join('\n')).toEqual([]);
          continue;
        }
        const args = tok(resolveRefs(step.args ?? [], refs)) as unknown[];
        if (step.async) {
          inflight.set(step.async, call(running, step.call, args));
          continue;
        }
        const outcome = await call(running, step.call, args);
        if (step.save) refs[step.save] = 'value' in outcome ? outcome.value : outcome;
        const problems = checkOutcome(outcome, step, `${label} ${step.call}`, tok, refs);
        expect(problems, problems.join('\n')).toEqual([]);
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
