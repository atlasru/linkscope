/** Reproducible CPU spatial benchmark. This does NOT measure WebView drawing or IPC. */
import { performance } from 'node:perf_hooks';
import { writeFile } from 'node:fs/promises';
import { BoundsIndex, PointIndex, type Bounds } from '../ui/src/spatial';

const nodeCount = Number(process.env.BENCH_NODES ?? 100_000), edgeCount = Number(process.env.BENCH_EDGES ?? 500_000);
if (!Number.isSafeInteger(nodeCount) || !Number.isSafeInteger(edgeCount) || nodeCount < 1 || edgeCount < 0) throw new Error('Invalid dataset size');
const columns = Math.ceil(Math.sqrt(nodeCount * 1.6)), step = 24;
const nodes = new PointIndex(160), edges = new BoundsIndex();
const point = (i: number): { x: number; y: number } => ({ x: (i % columns) * step, y: Math.floor(i / columns) * step });
const started = performance.now();
for (let i = 0; i < nodeCount; i++) { const p = point(i); nodes.set(String(i), p.x, p.y); }
const nodeBuildMs = performance.now() - started;
const edgeStart = performance.now();
edges.load((function* (): Generator<readonly [string, Bounds]> { for (let i = 0; i < edgeCount; i++) {
  const source = i % nodeCount;
  // Four local edges and one reproducible long edge per source at default size.
  const target = Math.floor(i / nodeCount) % 5 === 4 ? (Math.imul(source, 1664525) + 1013904223) >>> 0 : source + [1, columns, columns + 1, columns - 1][Math.floor(i / nodeCount) % 4]!;
  const a = point(source), b = point(target % nodeCount);
  yield [String(i), { minX: Math.min(a.x, b.x), minY: Math.min(a.y, b.y), maxX: Math.max(a.x, b.x), maxY: Math.max(a.y, b.y) }];
} })());
const edgeBuildMs = performance.now() - edgeStart;
edges.validate();
const times = (run: (bounds: Bounds) => number): { medianMs: number; p95Ms: number; meanResults: number } => {
  const samples: number[] = []; let results = 0;
  for (let i = 0; i < 60; i++) {
    const p = point((i * 15427) % nodeCount), bounds = { minX: p.x - 500, minY: p.y - 350, maxX: p.x + 500, maxY: p.y + 350 };
    const start = performance.now(); const count = run(bounds); const elapsed = performance.now() - start;
    if (i >= 10) { samples.push(elapsed); results += count; }
  }
  samples.sort((a, b) => a - b);
  return { medianMs: samples[Math.floor(samples.length / 2)]!, p95Ms: samples[Math.floor(samples.length * 0.95)]!, meanResults: results / samples.length };
};
const nodeQuery = times(bounds => nodes.query(bounds).length);
const edgeQueryUnbounded = times(bounds => edges.query(bounds).length);
const edgeQueryLod7000 = times(bounds => edges.sample(bounds, 7000).length);
const dragStart = performance.now();
for (let i = 0; i < 1000; i++) { const p = point(i * 37 % nodeCount); nodes.set(String(i * 37 % nodeCount), p.x + 10, p.y - 10); }
const move1000NodesMs = performance.now() - dragStart;
const result = {
  timestamp: new Date().toISOString(), runtime: process.version, platform: `${process.platform}/${process.arch}`,
  dataset: { nodes: nodeCount, edges: edgeCount, seed: 'lattice-24px-80%-local-20%-lcg-long', viewportWorldUnits: [1000, 700], samples: 50 },
  nodeBuildMs, edgeBuildMs, nodeQuery, edgeQueryUnbounded, edgeQueryLod7000, move1000NodesMs,
  memory: process.memoryUsage(),
  caveats: ['CPU spatial indexes only; excludes graph records, IPC, renderer, WebView GPU and worker layout.', 'Long-edge AABB queries return false positives; exact curve picking has additional cost.', 'Heap includes benchmark temporary allocations and has not been forced through garbage collection.'],
};
console.log(JSON.stringify(result, null, 2));
if (process.env.BENCH_OUTPUT) await writeFile(process.env.BENCH_OUTPUT, `${JSON.stringify(result, null, 2)}\n`);
