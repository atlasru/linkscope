// Run while serving ui/ at http://127.0.0.1:8765 with Playwright installed.
import {chromium} from 'playwright';
import assert from 'node:assert/strict';

const browser = await chromium.launch({headless:true,args:['--use-gl=angle','--use-angle=swiftshader','--enable-unsafe-swiftshader']});
const failures = [];
const url = 'http://127.0.0.1:8765/';
async function open(name, injection) {
  const page = await browser.newPage({viewport:{width:1280,height:900}});
  page.on('pageerror', error => failures.push(`${name}: ${error.message}`));
  if (injection) await page.addInitScript(injection);
  await page.goto(url);
  await page.waitForFunction(() => !!window.linkscope?.renderer);
  return page;
}
async function controls(page) {
  await page.click('#settings');
  assert.equal(await page.locator('#settings-dialog').evaluate(dialog => dialog.open), true);
  await page.locator('#settings-dialog [data-close]').click();
  await page.click('#demo');
  await page.click('#demo-load');
  await page.waitForFunction(() => linkscope.model.nodes.length === 350);
  await page.click('#map-tab');
  assert.equal(await page.locator('#map').isVisible(), true);
  await page.click('#graph-tab');
  assert.equal(await page.locator('#map').isVisible(), false);
}
async function fallbackInteraction(page, reason) {
  assert.equal(await page.evaluate(() => linkscope.renderer.disabled), true);
  assert.match(await page.locator('#status').textContent(), /Canvas2D/);
  assert.match(await page.locator('#graph-note').textContent(), reason);
  assert.equal(await page.locator('#layout').isDisabled(), true);
  await controls(page);
  const point = await page.evaluate(() => linkscope.renderer.screen(0));
  const box = await page.locator('#graph').boundingBox();
  await page.mouse.click(box.x + point[0], box.y + point[1]);
  await page.waitForFunction(() => document.querySelector('#selection .entity-title')?.textContent === linkscope.model.nodes[0].value);
  const before = await page.evaluate(() => linkscope.renderer.zoom);
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.wheel(0, -200);
  assert.ok(await page.evaluate(() => linkscope.renderer.zoom) > before);
  const camera = await page.evaluate(() => [...linkscope.renderer.camera]);
  await page.mouse.move(box.x + 200, box.y + 200);
  await page.mouse.down();
  await page.mouse.move(box.x + 240, box.y + 220);
  await page.mouse.up();
  assert.notDeepEqual(await page.evaluate(() => linkscope.renderer.camera), camera);
  assert.equal(await page.locator('#overlay').evaluate(c => c.getContext('2d').getImageData(0,0,c.width,c.height).data.some(v => v > 0)), true);
}
try {
  const normal = await open('normal');
  assert.equal(await normal.evaluate(() => linkscope.renderer.disabled), undefined, 'WebGL2 renderer did not initialize');
  await controls(normal);
  assert.equal(await normal.evaluate(() => linkscope.renderer.gl.getError()), 0);
  await normal.click('#layout');
  await normal.waitForFunction(() => linkscope.renderer.layoutSteps === 0, null, {timeout:30000});
  assert.equal(await normal.evaluate(() => linkscope.renderer.gl.getError()), 0);
  // The same WebGL canvas must remain an event target when the context is lost.
  await normal.locator('#graph').evaluate(canvas => canvas.dispatchEvent(new Event('webglcontextlost', {cancelable:true})));
  await normal.waitForFunction(() => linkscope.renderer.disabled);
  assert.match(await normal.locator('#status').textContent(), /потерян WebGL-контекст/);
  await normal.click('#settings');
  assert.equal(await normal.locator('#settings-dialog').evaluate(d => d.open), true);
  await normal.close();

  const unavailable = await open('unavailable', () => {
    const original = HTMLCanvasElement.prototype.getContext;
    HTMLCanvasElement.prototype.getContext = function(type, ...args) {
      return type === 'webgl2' ? null : original.call(this, type, ...args);
    };
  });
  await fallbackInteraction(unavailable, /context creation/);
  await unavailable.close();

  const shader = await open('shader', () => {
    const original = WebGL2RenderingContext.prototype.getShaderParameter;
    WebGL2RenderingContext.prototype.getShaderParameter = function(shader, parameter) {
      return parameter === this.COMPILE_STATUS ? false : original.call(this, shader, parameter);
    };
    WebGL2RenderingContext.prototype.getShaderInfoLog = () => 'simulated compiler error';
  });
  await fallbackInteraction(shader, /shader compilation failed/);
  assert.match(await shader.locator('#activity').textContent(), /simulated compiler error/);
  await shader.close();

  const thrown = await open('thrown', () => {
    const original = HTMLCanvasElement.prototype.getContext;
    HTMLCanvasElement.prototype.getContext = function(type, ...args) {
      if (type === 'webgl2') throw new Error('simulated context exception');
      return original.call(this, type, ...args);
    };
  });
  await fallbackInteraction(thrown, /simulated context exception/);
  await thrown.close();
  assert.deepEqual(failures, [], 'uncaught browser errors');
  console.log('WebGL2 startup, WebGL context loss, null/throwing contexts, shader failure, Canvas2D controls, selection, pan and zoom passed');
} finally {
  await browser.close();
}
