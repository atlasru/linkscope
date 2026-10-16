import { defineConfig } from '@playwright/test';
export default defineConfig({
  testDir: '../tests', testMatch: 'canvas.spec.ts', workers: 1, retries: 0,
  timeout: 30_000, use: {
    baseURL: process.env.LINKSCOPE_TEST_URL ?? 'http://127.0.0.1:1420', browserName: 'chromium', channel: 'msedge', headless: true,
    launchOptions: { args: ['--disable-webgl'] },
  },
  outputDir: '../test-results-canvas',
});
