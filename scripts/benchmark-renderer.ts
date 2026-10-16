/** Actual Canvas2D measurements in installed Edge. Start `npm run dev` first. */
import { chromium } from '@playwright/test';
import { writeFile } from 'node:fs/promises';
const browser = await chromium.launch({ channel: 'msedge', headless: true });
try {
  const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
  // tsx/esbuild preserves function names through this helper, including functions serialized by Playwright.
  await page.addInitScript('globalThis.__name = (target, value) => Object.defineProperty(target, "name", {value, configurable: true});');
  const baseUrl = (process.env.BENCH_URL ?? 'http://127.0.0.1:1420').replace(/\/$/, '');
  // A blank same-origin fixture avoids Vite hot reload or unrelated application startup during the long measurement.
  await page.route(`${baseUrl}/`, route => route.fulfill({ contentType: 'text/html', body: '<!doctype html><html><body></body></html>' }));
  await page.goto(`${baseUrl}/`);
  const result = await page.evaluate(async () => {
    const modulePath = '/src/canvas.ts';
    const { GraphCanvas } = await import(modulePath) as { GraphCanvas: typeof import('../ui/src/canvas').GraphCanvas };
    document.body.replaceChildren(); document.body.style.cssText = 'margin:0;width:100vw;height:100vh;';
    const host = document.createElement('div'); host.style.cssText = 'width:100vw;height:100vh'; document.body.append(host);
    const graph = new GraphCanvas(host, { select: () => {}, move: () => {}, create: () => {}, connect: () => {}, context: () => {} });
    (window as unknown as { __benchmarkGraph: typeof graph }).__benchmarkGraph = graph;
    const nodes = 100_000, edges = 500_000, columns = 400;
    const entities = new Map<string, import('../ui/src/types').Entity>(), relationships = new Map<string, import('../ui/src/types').Relationship>();
    const empty = {};
    const recordStart = performance.now();
    for (let i = 0; i < nodes; i++) { const id = String(i); entities.set(id, { id, kind: 'entity', label: id, x: i % columns * 24, y: Math.floor(i / columns) * 24, properties: empty, metadata: empty, sources: [], notes: '' }); }
    for (let i = 0; i < edges; i++) {
      const source = i % nodes, level = Math.floor(i / nodes);
      const target = level === 4 ? ((Math.imul(source, 1664525) + 1013904223) >>> 0) % nodes : (source + [1, columns, columns + 1, columns - 1][level]!) % nodes;
      const id = String(i); relationships.set(id, { id, source: String(source), target: String(target), kind: 'related', label: '', directed: true, properties: empty, metadata: empty, sources: [], notes: '' });
    }
    const recordsMs = performance.now() - recordStart, loadStart = performance.now(); graph.setData(entities, relationships, new Map());
    const setDataMs = performance.now() - loadStart;
    const frame = async (): Promise<void> => {
      for (let refinement = 0; refinement < 8; refinement++) {
        await new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
        if (!graph.getRenderStats().pendingFrame) return;
      }
      throw new Error('Renderer did not settle within its bounded refinement window');
    };
    graph.fit();
    await new Promise<void>(resolve => requestAnimationFrame(() => resolve()));
    const firstOverviewFrame = graph.getRenderStats();
    await frame(); const overview = graph.getRenderStats();
    const frameSamples: number[] = [], counts: { visibleNodes: number; drawnNodes: number; drawnEdges: number }[] = [];
    for (let i = 0; i < 25; i++) {
      graph.setCamera({ x: 1200 + i * 240, y: 700 + (i % 5) * 900, zoom: 1 }); await frame();
      const stats = graph.getRenderStats(); if (i >= 5) { frameSamples.push(stats.frameMs); counts.push(stats); }
    }
    frameSamples.sort((a, b) => a - b);
    const workerStart = performance.now(); const layout = await graph.layout(); const layoutMs = performance.now() - workerStart;
    const output = {
      timestamp: new Date().toISOString(), userAgent: navigator.userAgent, viewport: [innerWidth, innerHeight], devicePixelRatio,
      dataset: { nodes, edges, seed: 'lattice-24px-80%-local-20%-lcg-long' }, recordsMs, setDataMs, firstOverviewFrame, overview,
      normalZoom: { samples: frameSamples.length, medianFrameMs: frameSamples[Math.floor(frameSamples.length / 2)], p95FrameMs: frameSamples[Math.floor(frameSamples.length * 0.95)], counts },
      workerLayout: { elapsedMs: layoutMs, returnedNodes: layout.length, finite: layout.every(point => Number.isFinite(point.x) && Number.isFinite(point.y)), iterations: 60 },
      caveats: ['Headless installed Edge, not the native Tauri WebView2 host.', 'Demand-driven frame duration includes CPU drawing submission but not guaranteed GPU presentation.', 'Adaptive edge/node LOD is active and samples settle before measurement; full simultaneous graph rendering is not claimed.', 'setData is synchronous; target-size project opening still blocks the renderer during indexing.', 'Layout results measured but not applied to the graph during this benchmark.'],
    };
    return output;
  });
  // Keep completed measurements even if a later screenshot fails.
  await writeFile(process.env.BENCH_OUTPUT ?? 'docs/benchmark-renderer.json', `${JSON.stringify(result, null, 2)}\n`);
  const viewportImage = 'docs/benchmark-renderer-viewport.png', overviewImage = 'docs/benchmark-renderer-overview.png';
  await page.screenshot({ path: viewportImage });
  await page.evaluate(async () => {
    const graph = (window as unknown as { __benchmarkGraph: import('../ui/src/canvas').GraphCanvas }).__benchmarkGraph;
    graph.fit();
    for (let refinement = 0; refinement < 8; refinement++) {
      await new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
      if (!graph.getRenderStats().pendingFrame) break;
    }
  });
  await page.screenshot({ path: overviewImage });
  await page.evaluate(() => (window as unknown as { __benchmarkGraph: import('../ui/src/canvas').GraphCanvas }).__benchmarkGraph.destroy());
  const output = JSON.stringify({ ...result, screenshots: [viewportImage, overviewImage] }, null, 2); console.log(output);
  await writeFile(process.env.BENCH_OUTPUT ?? 'docs/benchmark-renderer.json', `${output}\n`);
} finally { await browser.close(); }
