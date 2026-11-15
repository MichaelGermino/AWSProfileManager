/**
 * Tool, not a test: screenshots of every 1.x screen and dialog for the unslop audit (Phase 2).
 * Output: .screens/*.png (git-ignored). Run:
 *   $env:APM_SCREENS='1'; npx playwright test -c tests/parity tools/screens
 */
import { test, type Page } from '@playwright/test';
import fs from 'node:fs';
import path from 'node:path';
import { launchElectron, REPO_ROOT, type LaunchOptions } from '../support/electronApp';

const OUT = path.join(REPO_ROOT, '.screens');
const THEME = (process.env.APM_SCREENS_THEME ?? 'dark') as 'light' | 'dark';
const SETTLED = { setupCompleted: true, seenTourIds: ['v1.5-features-ga', 'v1.6-features'], lastChangelogVersionSeen: '9.9.9', theme: THEME };
const RICH = {
  profiles: [
    { id: 'p1', name: 'Dev', idpEntryUrl: 'https://idp.example.test/sso', label: 'Development', autoRefresh: true, refreshIntervalMinutes: 60, credentialProfileName: 'dev', folderId: 'f1', roleArn: 'arn:aws:iam::111122223333:role/Developer', roleDisplayText: 'Shared Services (111122223333) - Developer', expiration: '2099-01-01T00:00:00.000Z', iconName: 'command-line', iconColor: '#3b82f6' },
    { id: 'p2', name: 'Prod', idpEntryUrl: 'https://idp.example.test/sso', label: 'Production', autoRefresh: true, refreshIntervalMinutes: 120, credentialProfileName: 'prod', folderId: 'f2', roleArn: 'arn:aws:iam::444455556666:role/ReadOnly', expiration: '2000-01-01T00:00:00.000Z', iconName: 'shield-check', iconColor: '#ef4444' },
    { id: 'p3', name: 'Sandbox', idpEntryUrl: 'https://idp.example.test/sso', label: 'Sandbox', autoRefresh: false, refreshIntervalMinutes: 60, credentialProfileName: 'sandbox', folderId: 'f1', iconName: 'beaker', iconColor: '#10b981' },
    { id: 'p4', name: 'Data Lake', idpEntryUrl: '', label: 'Analytics', autoRefresh: true, refreshIntervalMinutes: 60, credentialProfileName: 'data', authType: 'identityCenter', ssoStartUrl: 'https://d-1234567890.awsapps.com/start', ssoRegion: 'us-west-2', ssoAccountId: '777788889999', ssoRoleName: 'DataEngineer', iconName: 'circle-stack', iconColor: '#a855f7' },
  ],
};
const FOLDERS = { folders: [{ id: 'f1', name: 'Engineering', color: '#3b82f6' }, { id: 'f2', name: 'Production', color: '#ef4444' }] };

test.skip(!process.env.APM_SCREENS, 'set APM_SCREENS=1 to run this tool');

async function shot(page: Page, name: string): Promise<void> {
  fs.mkdirSync(OUT, { recursive: true });
  await page.waitForTimeout(700);
  await page.screenshot({ path: path.join(OUT, `${THEME}-${name}.png`) });
}

async function withApp(opts: LaunchOptions, body: (page: Page) => Promise<void>): Promise<void> {
  const running = await launchElectron(opts);
  try {
    await running.page.setViewportSize({ width: 1280, height: 820 });
    await running.page.waitForTimeout(3500);
    await body(running.page);
  } finally {
    await running.close();
  }
}

const richData: LaunchOptions = {
  credentials: '[dev]\naws_access_key_id=A\naws_secret_access_key=B\naws_session_token=C\n[prod]\naws_access_key_id=A\n',
  files: { 'settings.json': { ...SETTLED, openWebUiApiUrl: 'https://127.0.0.1:9/api', openWebUiApiKey: 'sk', openWebUiModel: 'model' }, 'profiles.json': RICH, 'folders.json': FOLDERS },
};

test('main screens', async () => {
  await withApp(richData, async (page) => {
    await shot(page, '01-profiles');
    await page.getByRole('button', { name: 'Grid view' }).click();
    await shot(page, '02-profiles-grid');
    await page.getByRole('button', { name: 'List view' }).click();
    await page.getByRole('button', { name: 'Edit Dev' }).click();
    await shot(page, '03-profile-edit');
    await page.getByRole('button', { name: 'Cancel', exact: true }).first().click();
    await page.getByRole('group', { name: 'Dev', exact: true }).click({ button: 'right' });
    await shot(page, '04-context-menu');
    await page.keyboard.press('Escape');
    await page.getByRole('link', { name: 'Terminal' }).first().click();
    await shot(page, '05-terminal');
    await page.getByRole('link', { name: 'Settings' }).first().click();
    for (const tab of ['General', 'Appearance', 'Credentials', 'Terminal & AI', 'Advanced']) {
      await page.getByRole('tab', { name: tab }).click();
      await shot(page, `06-settings-${tab.replace(/[^\w]+/g, '_')}`);
    }
  });
});

test('dialogs', async () => {
  await withApp(richData, async (page) => {
    await page.evaluate(() => (window as unknown as { electron: { refreshProfile(id: string): Promise<unknown> } }).electron.refreshProfile('p3'));
    await shot(page, '07-credentials-dialog');
  });
  await withApp({}, async (page) => {
    await shot(page, '08-wizard');
    await page.getByRole('button', { name: 'Skip setup' }).first().click();
    await shot(page, '09-empty-profiles');
  });
  await withApp({ files: { 'settings.json': { setupCompleted: true, seenTourIds: ['v1.5-features-ga'] }, 'profiles.json': RICH, 'folders.json': FOLDERS }, env: { FORCE_WHATS_NEW: '1' } }, async (page) => {
    await shot(page, '10-whats-new');
  });
});
