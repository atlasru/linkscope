import { test, expect, type Page } from '@playwright/test';
import type { GraphCanvas, Position } from '../ui/src/canvas';
interface Harness {
  graph: GraphCanvas;
  selections: { ids: string[]; edgeId?: string }[];
  moves: Position[][];
  creations: Position[];
  connections: string[][];
}
async function setup(page: Page): Promise<void> {
  await page.goto('/');
  await page.evaluate(async () => {
    const modulePath = '/src/canvas.ts';
    const { GraphCanvas } = await import(modulePath) as { GraphCanvas: typeof import('../ui/src/canvas').GraphCanvas };
    document.body.replaceChildren(); document.body.style.cssText = 'margin:0;width:100vw;height:100vh;';
    const host = document.createElement('div'); host.style.cssText = 'width:100vw;height:100vh'; document.body.append(host);
    const state = window as unknown as Harness;
    state.selections = []; state.moves = []; state.creations = []; state.connections = [];
    state.graph = new GraphCanvas(host, {
      select: (ids, edgeId) => state.selections.push({ ids, edgeId }), move: positions => state.moves.push(positions),
      create: (x, y) => state.creations.push({ id: '', x, y }), connect: (source, target) => state.connections.push([source, target]), context: () => {},
    });
    const entity = (id: string, x: number, y: number) => ({ id, label: id, kind: 'entity', x, y, properties: {}, metadata: {}, notes: '', sources: [] });
    const edge = (id: string, source: string, target: string) => ({ id, source, target, label: '', kind: 'related', directed: true, properties: {}, metadata: {}, notes: '', sources: [] });
    state.graph.setData(new Map([['a', entity('a', -150, 0)], ['b', entity('b', 150, 0)], ['c', entity('c', 0, 180)]]), new Map([['ab', edge('ab', 'a', 'b')], ['loop', edge('loop', 'a', 'a')]]), new Map());
    state.graph.setCamera({ x: 0, y: 0, zoom: 1 });
    await new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
  });
  if (test.info().project.use.launchOptions?.args?.includes('--disable-webgl')) {
    expect(await page.evaluate(() => document.createElement('canvas').getContext('webgl2') === null)).toBe(true);
  }
}
test.use({ viewport: { width: 1000, height: 700 } });

test('canvas selects nodes, drags a multiselection once, and creates on empty double click', async ({ page }) => {
  await setup(page);
  await page.mouse.click(350, 350); await page.keyboard.down('Shift'); await page.mouse.click(650, 350); await page.keyboard.up('Shift');
  await page.mouse.move(350, 350); await page.mouse.down(); await page.mouse.move(390, 390, { steps: 5 }); await page.mouse.up();
  const moved = await page.evaluate(() => (window as unknown as Harness).moves);
  expect(moved).toHaveLength(1);
  expect(moved[0]).toEqual(expect.arrayContaining([{ id: 'a', x: -110, y: 40 }, { id: 'b', x: 190, y: 40 }]));
  await page.mouse.dblclick(200, 150);
  expect(await page.evaluate(() => (window as unknown as Harness).creations)).toEqual([{ id: '', x: -300, y: -200 }]);
});

test('canvas picks relationships and self loops, links nodes and rectangle-selects', async ({ page }) => {
  await setup(page);
  await page.mouse.click(500, 350);
  expect(await page.evaluate(() => (window as unknown as Harness).selections.at(-1)?.edgeId)).toBe('ab');
  await page.mouse.click(350, 304);
  expect(await page.evaluate(() => (window as unknown as Harness).selections.at(-1)?.edgeId)).toBe('loop');
  await page.evaluate(() => (window as unknown as Harness).graph.setTool('link'));
  await page.mouse.click(350, 350); await page.mouse.click(650, 350);
  expect(await page.evaluate(() => (window as unknown as Harness).connections)).toEqual([['a', 'b']]);
  await page.evaluate(() => { const s = window as unknown as Harness; s.graph.setTool('select'); s.graph.setSelection(new Set()); });
  await page.keyboard.down('Shift'); await page.mouse.move(250, 250); await page.mouse.down(); await page.mouse.move(750, 600, { steps: 5 }); await page.mouse.up(); await page.keyboard.up('Shift');
  expect(await page.evaluate(() => (window as unknown as Harness).selections.at(-1)?.ids.sort())).toEqual(['a', 'b', 'c']);
});

test('zoom preserves the world point under the pointer and background dragging pans', async ({ page }) => {
  await setup(page);
  await page.mouse.move(850, 100); await page.mouse.wheel(0, -200);
  await expect.poll(() => page.evaluate(() => (window as unknown as Harness).graph.getCamera().zoom)).toBeGreaterThan(1);
  const camera = await page.evaluate(() => (window as unknown as Harness).graph.getCamera());
  expect(camera.x + 350 / camera.zoom).toBeCloseTo(350, 8); expect(camera.y - 250 / camera.zoom).toBeCloseTo(-250, 8);
  await page.mouse.move(800, 600); await page.mouse.down(); await page.mouse.move(850, 620, { steps: 3 }); await page.mouse.up();
  const after = await page.evaluate(() => (window as unknown as Harness).graph.getCamera());
  expect(after.x).toBeCloseTo(camera.x - 50 / camera.zoom, 6); expect(after.y).toBeCloseTo(camera.y - 20 / camera.zoom, 6);
});

test('worker returns finite positions for every entity and cancellation is explicit', async ({ page }) => {
  await setup(page);
  const positions = await page.evaluate(async () => (window as unknown as Harness).graph.layout());
  expect(positions.map(p => p.id).sort()).toEqual(['a', 'b', 'c']);
  expect(positions.every(p => Number.isFinite(p.x) && Number.isFinite(p.y))).toBe(true);
  const cancelled = await page.evaluate(async () => {
    const graph = (window as unknown as Harness).graph;
    const pending = graph.layout().then(() => 'completed', (error: DOMException) => error.name);
    graph.cancelLayout(); return pending;
  });
  expect(cancelled).toBe('AbortError');
});

test('parallel edges remain individually pickable after incremental updates', async ({ page }) => {
  await setup(page);
  await page.evaluate(() => (window as unknown as Harness).graph.update({ revision: 1, canUndo: true, canRedo: false, patch: { relationships: [{ id: 'ab2', source: 'a', target: 'b', label: '', kind: 'related', directed: true, properties: {}, metadata: {}, notes: '', sources: [] }] } }));
  await page.mouse.click(500, 342.5);
  expect(await page.evaluate(() => (window as unknown as Harness).selections.at(-1)?.edgeId)).toBe('ab');
  await page.mouse.click(500, 357.5);
  expect(await page.evaluate(() => (window as unknown as Harness).selections.at(-1)?.edgeId)).toBe('ab2');
});

test('adaptive budgets stop refining at rest and preserve full selection', async ({ page }) => {
  await setup(page);
  const result = await page.evaluate(async () => {
    const graph = (window as unknown as Harness).graph;
    const original = Object.getOwnPropertyDescriptor(performance, 'now');
    let calls = 0;
    Object.defineProperty(performance, 'now', { configurable: true, value: () => ++calls * 200 });
    try {
      graph.setSelection(new Set(['a', 'b', 'c']));
      graph.setCamera({ x: 0, y: 0, zoom: 1 });
      for (let i = 0; i < 16; i++) await new Promise<void>(resolve => requestAnimationFrame(() => resolve()));
      const stats = graph.getRenderStats(), before = calls;
      for (let i = 0; i < 6; i++) await new Promise<void>(resolve => requestAnimationFrame(() => resolve()));
      return { stats, callsWhileAtRest: calls - before };
    } finally {
      if (original) Object.defineProperty(performance, 'now', original); else Reflect.deleteProperty(performance, 'now');
    }
  });
  expect(result.stats.nodeBudget).toBe(1000); expect(result.stats.edgeBudget).toBe(250);
  expect(result.stats.drawnNodes).toBe(3); expect(result.stats.pendingFrame).toBe(false);
  expect(result.callsWhileAtRest).toBe(0);
});
