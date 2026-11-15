/**
 * Electron runner for the shared backend scenarios in native/tests/fixtures/scenarios/**.json.
 * Each scenario becomes one test. Calls go through window.electron (the real preload bridge),
 * mapped from channel names by src/shared/ipcContract.ts, so they exercise the same path as the UI.
 */
import { expect, test } from '@playwright/test';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { IPC_REQUESTS } from '../../src/shared/ipcContract';
import { FIXTURES, launchElectron, readKeychain, restartElectron, type RunningApp } from './support/electronApp';
import { match, resolveRefs, type FileExpectation, type Refs, type Scenario } from './support/scenario';

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
  // Electron resolves void handlers with undefined; the shared format (and Rust) uses null.
  return result === undefined ? null : result;
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
  if ('contains' in exp) return exp.contains.filter((s) => !text.includes(s)).map((s) => `${label}: missing ${JSON.stringify(s)}`);
  return match(JSON.parse(text), resolveRefs(exp.json, refs), label);
}

for (const { file, scenario } of loadScenarios(SCENARIO_DIR)) {
  const applies = !scenario.platforms || scenario.platforms.includes(process.platform as 'win32' | 'darwin');
  test(`${scenario.id} ${scenario.title}`, async () => {
    test.skip(!applies, `not for ${process.platform}`);
    test.info().annotations.push({ type: 'file', description: path.relative(SCENARIO_DIR, file) });
    let running = await launchElectron(scenario.given ?? {});
    const refs: Refs = {};
    try {
      for (const [i, step] of scenario.steps.entries()) {
        if ('restart' in step) {
          running = await restartElectron(running);
          continue;
        }
        const args = resolveRefs(step.args ?? [], refs) as unknown[];
        const result = await call(running, step.call, args);
        if (step.save) refs[step.save] = result;
        if ('expect' in step) {
          const problems = match(result, resolveRefs(step.expect, refs), `step ${i + 1} ${step.call}`);
          expect(problems, problems.join('\n')).toEqual([]);
        }
      }
      const then = scenario.then ?? {};
      const problems: string[] = [];
      for (const [name, exp] of Object.entries(then.files ?? {})) {
        problems.push(...checkFile(name, path.join(running.sandbox.appData, name), exp, refs));
      }
      if (then.credentials) problems.push(...checkFile('credentials', running.sandbox.credentialsFile, then.credentials, refs));
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
      expect(problems, problems.join('\n')).toEqual([]);
    } finally {
      await running.close();
    }
  });
}
