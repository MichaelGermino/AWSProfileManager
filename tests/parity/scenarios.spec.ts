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
import { isUiStep, runUiStep } from './support/uiSteps';
import { startFakes, TEST_CA_FILE, type Fakes, type FakesConfig } from '../fakes/fakes';

const SCENARIO_DIR = path.join(FIXTURES, 'scenarios');
/** UI flows use the same format plus UI steps (flows/README.md). */
const FLOW_DIR = path.join(FIXTURES, 'flows');
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

const requestByChannel = new Map(IPC_REQUESTS.map((r) => [r.channel, r]));
const recordedEvents = IPC_EVENTS.filter((e) => e.method && (e.window ?? 'main') === 'main').map((e) => ({
  channel: e.channel,
  method: e.method!,
}));

type RawOutcome = { ok: true; value: unknown } | { ok: false; error: string };

/** The auth log viewer window (opened on demand), whose bridge is window.logViewer. */
async function logViewerPage(running: RunningApp) {
  const find = () => running.app.windows().find((w) => w.url().includes('auth-log-viewer'));
  if (!find()) {
    await running.page.evaluate(() => (window as unknown as { electron: { openAuthLogViewer(): Promise<void> } }).electron.openAuthLogViewer());
  }
  for (let i = 0; i < 50 && !find(); i++) await new Promise((r) => setTimeout(r, 100));
  const page = find();
  if (!page) throw new Error('auth log viewer window did not open');
  await page.waitForFunction(() => typeof (window as unknown as { logViewer?: unknown }).logViewer === 'object');
  return page;
}

async function call(running: RunningApp, channel: string, args: unknown[]): Promise<CallOutcome> {
  const req = requestByChannel.get(channel);
  if (!req) throw new Error(`unknown channel ${channel}`);
  let outcome: RawOutcome;
  if (!req.method) {
    // Handled in main with no preload method (e.g. auth:refreshAll, used by the tray): call the
    // registered ipcMain handler directly.
    outcome = await running.app.evaluate(async ({ ipcMain, BrowserWindow }, { channel, args }) => {
      const handlers = (ipcMain as unknown as { _invokeHandlers: Map<string, (e: unknown, ...a: unknown[]) => unknown> })._invokeHandlers;
      const handler = handlers.get(channel);
      if (!handler) return { ok: false as const, error: `no handler for ${channel}` };
      const sender = BrowserWindow.getAllWindows()[0]?.webContents;
      try {
        return { ok: true as const, value: await handler({ sender }, ...args) };
      } catch (err) {
        return { ok: false as const, error: err instanceof Error ? err.message : String(err) };
      }
    }, { channel, args });
  } else {
    const page = req.window === 'logViewer' ? await logViewerPage(running) : running.page;
    const bridge = req.window === 'logViewer' ? 'logViewer' : 'electron';
    outcome = await page.evaluate(
      async ({ method, args, bridge }) => {
        const fn = (window as unknown as Record<string, Record<string, (...a: unknown[]) => unknown>>)[bridge][method];
        try {
          return { ok: true as const, value: await fn(...args) };
        } catch (err) {
          return { ok: false as const, error: err instanceof Error ? err.message : String(err) };
        }
      },
      { method: req.method, args, bridge }
    );
  }
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

function scenarioEnv(scenario: Scenario, fakes: Fakes | null): Record<string, string> {
  return {
    ...(scenario.given?.env ?? {}),
    ...(fakes ? { APM_HOST_MAP: JSON.stringify(fakes.hostMap()), APM_TEST_CA_FILE: TEST_CA_FILE } : {}),
  };
}

async function start(scenario: Scenario, fakes: Fakes | null): Promise<RunningApp> {
  const given = (fakes ? withTokens(scenario.given ?? {}, { FAKES: fakes.baseUrl }) : scenario.given ?? {}) as Scenario['given'] & object;
  const env = scenarioEnv(scenario, fakes);
  let running = await launchElectron({ ...given, env });
  if (given.keychain) {
    // Keychain is only reachable from inside Electron; seed, then restart so the app starts with it.
    const entries = Object.fromEntries(Object.entries(given.keychain).map(([k, v]) => [k, materializeSecret(v)]));
    await seedKeychain(running, entries);
    running = await restartElectron(running, { env });
  }
  await installEventRecorder(running.page, recordedEvents);
  await installExternalOpenCapture(running.app);
  await settle(running, fakes);
  return running;
}

/**
 * Let 1.x finish its startup work before a scenario begins. The window mounts the Terminal screen
 * in the background up to ~3 s after launch, which starts a shell and fetches the AWS CLI docs;
 * without waiting, that work interleaves with the scenario (extra terminal:start / docs requests).
 * 2.0 starts the shell on first visit instead, so this is a 1.x-only wait.
 * Then everything recorded so far (events, streams, fake requests) is cleared.
 */
async function settle(running: RunningApp, fakes: Fakes | null): Promise<void> {
  const deadline = Date.now() + 8_000;
  while (Date.now() < deadline) {
    const started = (await readStream(running.page, 'terminal:data')).length > 0 || (await takeEventsPeek(running));
    if (started) break;
    await new Promise((r) => setTimeout(r, 150));
  }
  await new Promise((r) => setTimeout(r, 1_500));
  await takeEvents(running.page);
  await running.page.evaluate(() => {
    const w = window as unknown as { __apmStreams: Record<string, unknown[]> };
    for (const k of Object.keys(w.__apmStreams ?? {})) w.__apmStreams[k] = [];
  });
  if (fakes) fakes.requests.length = 0;
}

/** True once the background terminal mount has reported an error (it did start, then failed). */
async function takeEventsPeek(running: RunningApp): Promise<boolean> {
  return running.page.evaluate(() =>
    ((window as unknown as { __apmEvents?: unknown[][] }).__apmEvents ?? []).some((e) => e[0] === 'terminal:error')
  );
}

for (const { file, scenario } of [...loadScenarios(SCENARIO_DIR), ...loadScenarios(FLOW_DIR)]) {
  const applies = !scenario.platforms || scenario.platforms.includes(process.platform as 'win32' | 'darwin');
  test(`${scenario.id} ${scenario.title}`, async () => {
    test.skip(!applies, `not for ${process.platform}`);
    test.info().annotations.push({ type: 'file', description: path.relative(FIXTURES, file) });
    const fakes = scenario.given?.fakes ? await startFakes(withTokens(scenario.given.fakes, { APP_VERSION }) as FakesConfig) : null;
    const env = scenarioEnv(scenario, fakes);
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
          await settle(running, null);
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
        if (isUiStep(step)) {
          await runUiStep(running, step, tok);
          continue;
        }
        if ('resetFakeRequests' in step) {
          if (fakes) fakes.requests.length = 0;
          continue;
        }
        if ('advanceClock' in step) {
          await running.app.evaluate((_e, ms) => {
            const g = globalThis as unknown as { __apmClockOffset?: number; __apmRealNow?: () => number };
            if (!g.__apmRealNow) {
              g.__apmRealNow = Date.now.bind(Date);
              g.__apmClockOffset = 0;
              Date.now = () => g.__apmRealNow!() + g.__apmClockOffset!;
            }
            g.__apmClockOffset! += ms;
          }, step.advanceClock);
          continue;
        }
        if ('wait' in step) {
          await new Promise((r) => setTimeout(r, step.wait));
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
            // Terminal output carries ANSI colour/cursor codes (PSReadLine colours each token); match on plain text.
            const text = items
              .map((x) => (typeof x === 'string' ? x : JSON.stringify(x)))
              .join('')
              .replace(/\x1b\[[0-9;?]*[ -/]*[@-~]/g, '');
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
