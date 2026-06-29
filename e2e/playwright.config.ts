import { defineConfig } from '@playwright/test';

const baseURL = process.env.MAXIO_E2E_BASE_URL ?? 'http://127.0.0.1:19010';

export default defineConfig({
  testDir: './tests',
  timeout: 60_000,
  retries: process.env.CI ? 1 : 0,
  use: {
    baseURL,
    trace: 'on-first-retry',
  },
  reporter: [['list']],
});