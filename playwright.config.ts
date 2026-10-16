import {defineConfig} from '@playwright/test';
export default defineConfig({
  testDir:'./tests',testMatch:'*.spec.ts',fullyParallel:false,workers:1,timeout:30000,
  use:{baseURL:process.env.LINKSCOPE_TEST_URL??'http://127.0.0.1:1420',headless:true,channel:process.platform==='win32'?'msedge':undefined,viewport:{width:1440,height:900}},
  webServer:process.env.LINKSCOPE_TEST_URL?undefined:{command:'npm run dev',url:'http://127.0.0.1:1420',reuseExistingServer:true,timeout:60000},
  reporter:[['list']],outputDir:'test-results/browser'
});
