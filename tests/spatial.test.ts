import test from 'node:test';
import assert from 'node:assert/strict';
import { BoundsIndex, PointIndex, intersects, pointSegmentDistanceSquared, type Bounds } from '../ui/src/spatial';

test('point grid handles negatives, boundary cells, replacement, removal and huge viewports', () => {
  const index = new PointIndex(10);
  index.set('negative', -10, -10); index.set('origin', 0, 0); index.set('boundary', 10, 10);
  assert.deepEqual(index.query({ minX: -10, minY: -10, maxX: 0, maxY: 0 }).map(p => p.id).sort(), ['negative', 'origin']);
  assert.equal(index.nearest(9, 9, 3)?.id, 'boundary');
  assert.equal(index.nearest(9, 9, 3, id => id !== 'boundary'), undefined);
  index.set('boundary', 100, 100);
  assert.equal(index.nearest(9, 9, 3), undefined);
  assert.equal(index.query({ minX: -1e12, minY: -1e12, maxX: 1e12, maxY: 1e12 }).length, 3);
  index.remove('origin'); index.remove('absent');
  assert.deepEqual(index.bounds(), { minX: -10, minY: -10, maxX: 100, maxY: 100 });
  assert.throws(() => index.set('invalid', NaN, 0));
  assert.equal(index.size, 2);
  index.clear(); assert.equal(index.bounds(), undefined);
});

test('point queries match brute force through repeated moves', () => {
  const index = new PointIndex(17), positions = new Map<string, { x: number; y: number }>();
  for (let i = 0; i < 2_000; i++) {
    const x = (i * 443 % 1249) - 624, y = (i * 691 % 2017) - 1008;
    positions.set(String(i), { x, y }); index.set(String(i), x, y);
  }
  for (let pass = 0; pass < 30; pass++) {
    for (let i = pass; i < 2_000; i += 31) { const p = { x: i * 9 - pass * 7, y: i * 3 }; positions.set(String(i), p); index.set(String(i), p.x, p.y); }
    const rect = { minX: pass * 11 - 500, minY: pass * 13 - 750, maxX: pass * 11 + 100, maxY: pass * 13 + 200 };
    const expected = [...positions].filter(([, p]) => p.x >= rect.minX && p.x <= rect.maxX && p.y >= rect.minY && p.y <= rect.maxY).map(([id]) => id).sort();
    assert.deepEqual(index.query(rect).map(p => p.id).sort(), expected);
  }
});

test('bounds tree preserves every edge, including huge overlapping bounds and point bounds', () => {
  const index = new BoundsIndex(), all = new Map<string, Bounds>();
  for (let i = 0; i < 10_000; i++) {
    const x = i * 17 % 5000 - 2500, y = i * 31 % 7000 - 3500;
    const bounds = { minX: x, minY: y, maxX: x + i % 700, maxY: y + i % 1100 };
    index.set(String(i), bounds); all.set(String(i), bounds);
  }
  index.set('huge', { minX: -1e12, minY: -1e12, maxX: 1e12, maxY: 1e12 });
  all.set('huge', { minX: -1e12, minY: -1e12, maxX: 1e12, maxY: 1e12 });
  index.set('point', { minX: 0, minY: 0, maxX: 0, maxY: 0 }); all.set('point', { minX: 0, minY: 0, maxX: 0, maxY: 0 });
  index.validate();
  for (let q = 0; q < 40; q++) {
    const bounds = { minX: q * 41 - 1500, minY: q * 27 - 1900, maxX: q * 41 - 1000, maxY: q * 27 - 1000 };
    const expected = [...all].filter(([, b]) => intersects(bounds, b)).map(([id]) => id).sort();
    assert.deepEqual(index.query(bounds).sort(), expected);
    assert.equal(index.query(bounds, 7).length, Math.min(7, expected.length));
  }
  for (let i = 0; i < 10_000; i += 2) { index.remove(String(i)); all.delete(String(i)); }
  for (let i = 1; i < 10_000; i += 5) { const b = { minX: i, minY: i, maxX: i + 1, maxY: i + 1 }; index.set(String(i), b); all.set(String(i), b); }
  index.validate(); assert.equal(index.size, all.size);
  const query = { minX: -100, minY: -200, maxX: 500, maxY: 600 };
  assert.deepEqual(index.query(query).sort(), [...all].filter(([, b]) => intersects(query, b)).map(([id]) => id).sort());
  for (const id of all.keys()) index.remove(id);
  index.validate(); assert.equal(index.size, 0); assert.deepEqual(index.query(query), []);
});

test('bounds tree rejects invalid input and uses inclusive intersections', () => {
  const index = new BoundsIndex();
  assert.throws(() => index.set('bad', { minX: 1, minY: 0, maxX: 0, maxY: 1 }));
  assert.throws(() => index.set('bad', { minX: 0, minY: 0, maxX: Infinity, maxY: 1 }));
  index.set('line', { minX: 1, minY: 0, maxX: 1, maxY: 5 });
  assert.deepEqual(index.query({ minX: 0, minY: 5, maxX: 1, maxY: 10 }), ['line']);
});

test('bulk-loaded tree matches incremental queries and remains editable at uneven sizes', () => {
  for (const count of [0, 1, 2, 3, 5, 9, 127, 1001, 10_003]) {
    const entries: [string, Bounds][] = Array.from({ length: count }, (_, i) => [String(i), { minX: i * 31 % 701, minY: i * 17 % 607, maxX: i * 31 % 701 + 13, maxY: i * 17 % 607 + 19 }]);
    const bulk = new BoundsIndex(); bulk.load(entries); bulk.validate(); assert.equal(bulk.size, count);
    const rect = { minX: 100, minY: 100, maxX: 400, maxY: 400 };
    assert.deepEqual(bulk.query(rect).sort(), entries.filter(([, bounds]) => intersects(bounds, rect)).map(([id]) => id).sort());
    for (let i = 0; i < count; i += 3) bulk.remove(String(i));
    for (let i = 0; i < count; i += 7) bulk.set(String(i), { minX: -i, minY: -i, maxX: 0, maxY: 0 });
    bulk.validate();
  }
});

test('segment distance supports degenerate, projected and endpoint picks', () => {
  assert.equal(pointSegmentDistanceSquared(3, 4, 0, 0, 0, 0), 25);
  assert.equal(pointSegmentDistanceSquared(5, 2, 0, 0, 10, 0), 4);
  assert.equal(pointSegmentDistanceSquared(13, 4, 0, 0, 10, 0), 25);
});

test('visual sampling is bounded, spatially distributed and never affects exhaustive picking', () => {
  const index = new BoundsIndex();
  index.load(Array.from({ length: 1000 }, (_, i) => [String(i), { minX: i, minY: 0, maxX: i + 1, maxY: 1 }] as const));
  const all = { minX: -1, minY: -1, maxX: 2000, maxY: 2 }, sample = index.sample(all, 20);
  assert.equal(sample.length, 20); assert.equal(new Set(sample).size, 20);
  assert.ok(Math.min(...sample.map(Number)) < 100); assert.ok(Math.max(...sample.map(Number)) > 900);
  assert.equal(index.query(all).length, 1000);
  const local = { minX: 401, minY: 0, maxX: 413, maxY: 1 };
  assert.ok(index.sample(local, 3).every(id => index.query(local).includes(id)));
  assert.deepEqual(index.sample({ minX: -100, minY: -100, maxX: -1, maxY: -1 }, 20), []);
  assert.deepEqual(index.sample(all, 0), []);
});
