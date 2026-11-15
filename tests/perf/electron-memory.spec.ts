/**
 * Layer G: memory per app state for the 1.x Electron build (the baseline the GPUI app must beat).
 * Run: npx playwright test -c tests/perf
 * Appends one JSON line per state to tests/perf/results/electron.jsonl and prints a table.
 * States: Profiles idle, Terminal open, hidden to tray (window closed), each after settling.
 */
import { test } from '@playwright/test';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { launchElectron } from '../parity/support/electronApp';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const RESULTS = path.join(HERE, 'results');
const MEASURE = path.join(HERE, 'measure-memory.ps1');
const SETTLE_MS = Number(process.env.APM_PERF_SETTLE_MS ?? 15_000);

function measure(pid: number, label: string, outFile: string): void {
  execFileSync('powershell', ['-NoProfile', '-File', MEASURE, '-RootPid', String(pid), '-Label', label, '-Samples', '5', '-IntervalSeconds', '2', '-OutFile', outFile], {
    stdio: 'inherit',
  });
}

test('electron memory by state', async () => {
  test.setTimeout(10 * 60_000);
  test.skip(process.platform !== 'win32', 'measure-memory.ps1 is Windows-only for now');
  fs.mkdirSync(RESULTS, { recursive: true });
  const outFile = path.join(RESULTS, 'electron.jsonl');
  const running = await launchElectron({
    dataDir: 'two-folders',
    files: { 'settings.json': { setupCompleted: true, seenTourIds: ['v1.5-features-ga', 'v1.6-features'], lastChangelogVersionSeen: '9.9.9' } },
  });
  try {
    const pid = running.app.process().pid!;
    const { page } = running;
    await page.waitForTimeout(SETTLE_MS);
    measure(pid, 'electron: Profiles idle', outFile);

    await page.getByRole('link', { name: 'Terminal' }).click();
    await page.waitForTimeout(SETTLE_MS);
    measure(pid, 'electron: Terminal open', outFile);

    await page.getByRole('link', { name: 'Profiles' }).click();
    // The title-bar close button hides the window to the tray.
    await page.evaluate(() => (window as unknown as { electron: { windowClose(): Promise<void> } }).electron.windowClose());
    await new Promise((r) => setTimeout(r, SETTLE_MS * 2));
    measure(pid, 'electron: hidden to tray', outFile);
  } finally {
    await running.close();
  }
});
