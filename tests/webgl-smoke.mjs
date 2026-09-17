// Run while serving ui/ at http://127.0.0.1:8765 with Playwright installed.
import {chromium} from 'playwright';
import assert from 'node:assert/strict';

const browser = await chromium.launch({headless:true,args:['--use-gl=angle','--use-angle=swiftshader','--enable-unsafe-swiftshader']});
const failures = [];
try {
  const page = await browser.newPage({viewport:{width:1280,height:900}});
  page.on('pageerror', error => failures.push(`normal: ${error.message}`));
  await page.goto('http://127.0.0.1:8765/');
  await page.waitForFunction(() => !!window.linkscope?.renderer);
  assert.equal(await page.evaluate(() => linkscope.renderer.disabled), undefined, 'WebGL2 renderer did not initialize');
  await page.click('#empty-demo');
  await page.click('#demo-load');
  await page.waitForFunction(() => linkscope.model.nodes.length === 350);
  assert.equal(await page.evaluate(() => linkscope.renderer.gl.getError()), 0, 'WebGL error after demo');
  await page.click('#layout');
  await page.waitForFunction(() => linkscope.renderer.layoutSteps === 0, null, {timeout:30000});
  assert.equal(await page.evaluate(() => linkscope.renderer.gl.getError()), 0, 'WebGL error after layout');
  await page.click('#settings');
  assert.equal(await page.locator('#settings-dialog').evaluate(dialog => dialog.open), true);
  await page.locator('#settings-dialog [data-close]').click();
  await page.close();

  const fallback = await browser.newPage({viewport:{width:1280,height:900}});
  fallback.on('pageerror', error => failures.push(`safe mode: ${error.message}`));
  await fallback.addInitScript(() => {
    const original = HTMLCanvasElement.prototype.getContext;
    HTMLCanvasElement.prototype.getContext = function(type, ...args) {
      if (type === 'webgl2') return null;
      return original.call(this, type, ...args);
    };
  });
  await fallback.goto('http://127.0.0.1:8765/');
  await fallback.waitForFunction(() => !!window.linkscope?.renderer);
  assert.equal(await fallback.evaluate(() => linkscope.renderer.disabled), true);
  assert.match(await fallback.locator('#status').textContent(), /безопасный режим/);
  await fallback.click('#empty-add');
  await fallback.fill('#node-value','example.org');
  await fallback.locator('#add-form button.primary').click();
  await fallback.waitForFunction(() => linkscope.model.nodes.length === 1);
  await fallback.click('#settings');
  assert.equal(await fallback.locator('#settings-dialog').evaluate(dialog => dialog.open), true);
  await fallback.locator('#settings-dialog [data-close]').click();
  await fallback.click('#demo');
  await fallback.click('#demo-load');
  await fallback.waitForFunction(() => linkscope.model.nodes.length === 350);
  assert.equal(await fallback.locator('#layout').isDisabled(), true);
  assert.match(await fallback.locator('#graph-note').textContent(), /безопасный режим/);
  await fallback.close();
  assert.deepEqual(failures, [], 'uncaught browser errors');
  console.log('WebGL2 normal startup, layout, controls and WebGL2-unavailable safe mode passed');
} finally {
  await browser.close();
}
