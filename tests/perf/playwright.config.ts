import { defineConfig } from '@playwright/test';

/** Memory measurements (layer G). Run after `npm run build`: npx playwright test -c tests/perf */
export default defineConfig({
  testDir: '.',
  testMatch: /.*-memory\.spec\.ts$/,
  workers: 1,
  timeout: 10 * 60_000,
  reporter: [['list']],
  use: { trace: 'off' },
});
