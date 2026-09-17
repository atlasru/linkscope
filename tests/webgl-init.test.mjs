import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {FallbackRenderer} from '../ui/fallback-renderer.js';

const shader = readFileSync(new URL('../ui/renderer.js', import.meta.url), 'utf8');
const app = readFileSync(new URL('../ui/app.js', import.meta.url), 'utf8');

test('GLSL does not use the reserved active identifier', () => {
  assert.doesNotMatch(shader, /\bin\s+float\s+active\s*;/);
  assert.match(shader, /\bin\s+float\s+isActive\s*;/);
  assert.match(shader, /this\.attrib\(p,'isActive',this\.activeBuffer,1\)/);
});

test('startup failures do not abort UI handler registration', () => {
  assert.match(app, /new FallbackRenderer\(/);
  assert.doesNotMatch(app, /WebGL2 недоступен';throw e/);
});

test('safe-mode renderer preserves non-GPU interaction contract', () => {
  const renderer = new FallbackRenderer({getBoundingClientRect: () => ({width:800,height:600})}, 'test failure');
  const model = {
    nodes: [{timestamp:100}, {timestamp:200}],
    positions: new Float32Array([0,0,100,100]),
    edges: [{a:0,b:1}],
    links: new Uint32Array([0,1])
  };
  renderer.setModel(model);
  renderer.fit();
  assert.equal(renderer.disabled, true);
  assert.ok(Number.isFinite(renderer.zoom));
  assert.equal(renderer.pick(...renderer.screen(0)),0);
  renderer.filter(150,250);
  assert.equal(renderer.edgeCount,0);
  renderer.select([1]);
  renderer.readPositions();
  assert.throws(() => renderer.startLayout(), /WebGL2/);
});
