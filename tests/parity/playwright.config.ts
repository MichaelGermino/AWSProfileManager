import { defineConfig } from '@playwright/test';

/**
 * Parity suite against the 1.x Electron build (docs/native-rewrite-plan.md §5).
 * Run: npm run build && npx playwright test -c tests/parity
 * One worker: each test launches a full app with a tray icon and keychain access.
 */
export default defineConfig({
  testDir: '.',
  testMatch: /.*\.spec\.ts$/,
  workers: 1,
  timeout: 90_000,
  expect: { timeout: 10_000 },
  reporter: [['list']],
  globalTeardown: './support/globalTeardown.ts',
  use: { trace: 'off' },
});
