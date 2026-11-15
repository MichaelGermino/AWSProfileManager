/**
 * Tool, not a test: dumps accessibility trees (Playwright aria snapshots) of 1.x screens and
 * dialogs so UI flows can be written against roles and accessible names. Output: .aria-dump/*.yml
 * (git-ignored; test-results/ is wiped on every run). Run:
 *   $env:APM_ARIA_DUMP='1'; npx playwright test -c tests/parity tools/aria-dump
 */
import { test, type Page } from '@playwright/test';
import fs from 'node:fs';
import path from 'node:path';
import { launchElectron, REPO_ROOT, seedKeychain, restartElectron, type LaunchOptions } from '../support/electronApp';
import { encryptV1 } from '../support/scenario';

const OUT = path.join(REPO_ROOT, '.aria-dump');
const SETTLED = { setupCompleted: true, seenTourIds: ['v1.5-features-ga', 'v1.6-features'], lastChangelogVersionSeen: '9.9.9' };

test.skip(!process.env.APM_ARIA_DUMP, 'set APM_ARIA_DUMP=1 to run this tool');

async function save(page: Page, name: string): Promise<void> {
  fs.mkdirSync(OUT, { recursive: true });
  await page.waitForTimeout(800);
  fs.writeFileSync(path.join(OUT, `${name}.yml`), await page.locator('body').ariaSnapshot());
}

async function withApp(opts: LaunchOptions, body: (page: Page) => Promise<void>): Promise<void> {
  const running = await launchElectron(opts);
  try {
    await running.page.waitForTimeout(3500); // let the background Terminal mount finish
    await body(running.page);
  } finally {
    await running.close();
  }
}

test('screens and profile states', async () => {
  await withApp({ dataDir: 'two-folders', files: { 'settings.json': SETTLED } }, async (page) => {
    await save(page, 'profiles');
    await page.getByRole('button', { name: 'Add profile', exact: true }).click();
    await save(page, 'profile-add-form');
    await page.getByRole('button', { name: 'Identity Center Browser sign-in, then silent renewal' }).click();
    await save(page, 'profile-add-form-sso');
    await page.getByRole('button', { name: 'Cancel', exact: true }).first().click();
    await page.getByRole('link', { name: 'Terminal' }).first().click();
    await save(page, 'terminal');
    await page.getByRole('link', { name: 'Settings' }).first().click();
    for (const tab of ['General', 'Appearance', 'Credentials', 'Terminal & AI', 'Advanced']) {
      await page.getByRole('tab', { name: tab }).click();
      await save(page, `settings-${tab.replace(/[^\w]+/g, '_')}`);
    }
  });
});

test('master password create dialog', async () => {
  await withApp({ files: { 'settings.json': SETTLED } }, async (page) => {
    await page.getByRole('link', { name: 'Settings' }).first().click();
    await page.getByRole('tab', { name: 'Credentials' }).click();
    await save(page, 'settings-credentials-before');
    await page.getByRole('textbox', { name: 'Username' }).fill('alice');
    await page.getByRole('button', { name: 'Save default credentials' }).click();
    await save(page, 'master-password-dialog');
  });
});

test('unlock screen', async () => {
  const running = await launchElectron({ files: { 'settings.json': { ...SETTLED, masterPasswordEnabled: true } } });
  await seedKeychain(running, {
    __master_password_verifier__: encryptV1('Correct horse 1', 'aws-profile-manager:master-password-verifier:v1'),
  });
  const again = await restartElectron(running);
  try {
    await again.page.waitForTimeout(2500);
    await save(again.page, 'unlock-screen');
  } finally {
    await again.close();
  }
});

test('setup wizard on a fresh install', async () => {
  await withApp({}, async (page) => {
    await save(page, 'wizard-1');
  });
});

test('release notes modal', async () => {
  await withApp(
    { dataDir: 'two-folders', files: { 'settings.json': { setupCompleted: true, seenTourIds: ['v1.5-features-ga'] } }, env: { FORCE_WHATS_NEW: '1' } },
    async (page) => {
      await save(page, 'whats-new');
    }
  );
});

test('AI panel', async () => {
  await withApp(
    { dataDir: 'two-folders', files: { 'settings.json': { ...SETTLED, openWebUiApiUrl: 'https://127.0.0.1:9/api', openWebUiApiKey: 'sk', openWebUiModel: 'm' } } },
    async (page) => {
      await page.getByRole('link', { name: 'Terminal' }).first().click();
      await save(page, 'terminal-ai');
    }
  );
});

test('command explorer search', async () => {
  await withApp({ dataDir: 'two-folders', files: { 'settings.json': SETTLED } }, async (page) => {
    await page.getByRole('link', { name: 'Terminal' }).first().click();
    await page.getByRole('searchbox', { name: 'Search AWS CLI commands' }).fill('mb');
    await save(page, 'command-search');
  });
});

test('bulk import wizard', async () => {
  await withApp({ dataDir: 'two-folders', files: { 'settings.json': SETTLED } }, async (page) => {
    await page.getByRole('button', { name: 'More ways to add profiles' }).click();
    await page.getByRole('menuitem', { name: /Add accounts/ }).click();
    await save(page, 'import-1');
    await page.getByRole('button', { name: "I don't have one" }).click();
    await save(page, 'import-2');
    await page.getByRole('button', { name: /^SAML \/ ADFS/ }).click();
    await page.getByRole('button', { name: 'Continue' }).click();
    await save(page, 'import-3');
    await page.getByRole('textbox', { name: 'Master password' }).fill('Correct horse 1');
    await page.getByRole('textbox', { name: 'Confirm' }).fill('Correct horse 1');
    await page.getByRole('button', { name: 'Set password' }).click();
    await save(page, 'import-4');
    const cont = page.getByRole('button', { name: /^(Continue|Skip)/ }).first();
    if (await cont.isEnabled().catch(() => false)) {
      await cont.click();
      await save(page, 'import-5');
    }
  });
});

test('refresh-all and sso-expired dialogs', async () => {
  await withApp(
    {
      files: {
        'settings.json': SETTLED,
        'profiles.json': {
          profiles: [
            { id: 'a', name: 'A', idpEntryUrl: 'https://idp.example.test/sso', label: '', autoRefresh: true, refreshIntervalMinutes: 60, credentialProfileName: 'a' },
            { id: 'b', name: 'B', idpEntryUrl: 'https://idp.example.test/sso', label: '', autoRefresh: true, refreshIntervalMinutes: 60, credentialProfileName: 'b' },
            { id: 's', name: 'S', idpEntryUrl: '', label: '', autoRefresh: false, refreshIntervalMinutes: 60, credentialProfileName: 's', authType: 'identityCenter', ssoStartUrl: 'https://d-1.awsapps.com/start', ssoRegion: 'us-west-2', ssoAccountId: '1', ssoRoleName: 'R' },
          ],
        },
      },
    },
    async (page) => {
      await page.evaluate(() => (window as unknown as { electron: { refreshAutoRefreshProfiles(): Promise<void> } }).electron.refreshAutoRefreshProfiles());
      await save(page, 'refresh-all-dialog');
    }
  );
});

test('forgot master password', async () => {
  const running = await launchElectron({ files: { 'settings.json': { ...SETTLED, masterPasswordEnabled: true } } });
  await seedKeychain(running, {
    __master_password_verifier__: encryptV1('Correct horse 1', 'aws-profile-manager:master-password-verifier:v1'),
  });
  const again = await restartElectron(running);
  try {
    await again.page.waitForTimeout(2500);
    await again.page.getByRole('button', { name: 'Forgot master password?' }).click();
    await save(again.page, 'forgot-master-password');
  } finally {
    await again.close();
  }
});
