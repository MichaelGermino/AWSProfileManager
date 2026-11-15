/**
 * Tool, not a test: dumps the accessibility tree (Playwright aria snapshot) of each 1.x screen so
 * UI flows can be written against roles and accessible names. Output goes to
 * test-results/aria/<screen>.yml. Run: npx playwright test -c tests/parity tools/aria-dump --workers 1
 */
import { test } from '@playwright/test';
import fs from 'node:fs';
import path from 'node:path';
import { launchElectron, REPO_ROOT } from '../support/electronApp';

const OUT = path.join(REPO_ROOT, 'test-results', 'aria');

test('dump aria snapshots of every screen', async () => {
  test.skip(!process.env.APM_ARIA_DUMP, 'set APM_ARIA_DUMP=1 to run this tool');
  fs.mkdirSync(OUT, { recursive: true });
  const running = await launchElectron({
    dataDir: 'two-folders',
    files: { 'settings.json': { setupCompleted: true, seenTourIds: ['v1.5-features-ga', 'v1.6-features'], lastChangelogVersionSeen: '0.0.0' } },
  });
  const { page } = running;
  try {
    const save = async (name: string) => {
      await page.waitForTimeout(800);
      fs.writeFileSync(path.join(OUT, `${name}.yml`), await page.locator('body').ariaSnapshot());
    };
    await save('profiles');
    for (const [hash, name] of [['#/terminal', 'terminal'], ['#/settings', 'settings']] as const) {
      await page.evaluate((h) => (window.location.hash = h), hash);
      await save(name);
    }
    // Settings tabs, if any are exposed as tabs.
    const tabs = page.getByRole('tab');
    const n = await tabs.count();
    for (let i = 0; i < n; i++) {
      const label = ((await tabs.nth(i).textContent()) ?? `tab${i}`).trim().replace(/[^\w-]+/g, '_');
      await tabs.nth(i).click();
      await save(`settings-${label}`);
    }
    // Profiles page states.
    await page.evaluate(() => (window.location.hash = '#/'));
    await page.waitForTimeout(500);
    await page.getByRole('button', { name: 'Add profile', exact: true }).click();
    await save('profile-add-form');
    await page.keyboard.press('Escape');
    await page.evaluate(() => (window.location.hash = '#/'));
    await page.waitForTimeout(300);
    await page.getByRole('button', { name: 'More ways to add profiles' }).click();
    await save('add-menu');
    await page.keyboard.press('Escape');
    await page.getByRole('button', { name: 'Folder options for Team A' }).click();
    await save('folder-menu');
    await page.keyboard.press('Escape');
    await page.getByText('Dev', { exact: true }).first().click({ button: 'right' });
    await save('profile-context-menu');
    await page.keyboard.press('Escape');
    await page.getByText('Dev', { exact: true }).first().hover();
    await save('profile-row-hover');
    // Credentials modal: refresh a profile with no saved credentials.
    await page.evaluate(() => (window as unknown as { electron: { refreshProfile(id: string): Promise<unknown> } }).electron.refreshProfile('p1'));
    await save('credentials-modal');
  } finally {
    await running.close();
  }
});
