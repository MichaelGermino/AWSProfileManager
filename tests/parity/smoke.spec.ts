import { expect, test } from '@playwright/test';
import fs from 'node:fs';
import path from 'node:path';
import { launchElectron } from './support/electronApp';

test('SMOKE-001 app launches in an isolated sandbox', async () => {
  const running = await launchElectron();
  try {
    const { page, sandbox } = running;
    const profiles = await page.evaluate(() => (window as any).electron.getProfiles());
    expect(profiles).toEqual([]);
    const version = await page.evaluate(() => (window as any).electron.getAppVersion());
    expect(version).toMatch(/^\d+\.\d+\.\d+/);
    // Writes land in the sandbox, not in %APPDATA%.
    await page.evaluate(() => (window as any).electron.setSidebarCollapsed(true));
    expect(fs.existsSync(path.join(sandbox.appData, 'ui-prefs.json'))).toBe(true);
  } finally {
    await running.close();
  }
});
