/** Approximate force layout. All nodes and valid edges participate; the UI applies the result as one undoable edit. */
export {};
interface LayoutRequest { positions: Float64Array; edges: Uint32Array; iterations: number }
interface LayoutScope {
  onmessage: ((event: MessageEvent<LayoutRequest>) => void) | null;
  postMessage(message: { positions?: Float64Array; error?: string }, transfer?: Transferable[]): void;
}
const scope = self as unknown as LayoutScope;

scope.onmessage = (event): void => {
  try {
    const { positions, edges } = event.data;
    const count = positions.length / 2;
    if (!Number.isInteger(count) || edges.length % 2 || positions.some(value => !Number.isFinite(value)) || edges.some(value => value >= count)) throw new Error('Invalid layout input');
    if (positions.some(value => Math.abs(value) > 1e12)) throw new Error('Layout coordinates exceed the supported range (±1e12). Move the graph nearer the origin first.');
    if (count < 2) { scope.postMessage({ positions }, [positions.buffer]); return; }
    const iterations = Math.max(1, Math.min(200, event.data.iterations));
    const movement = new Float64Array(positions.length), degrees = new Uint32Array(count);
    for (const endpoint of edges) degrees[endpoint]!++;
    // Reproducible tiny displacements separate coincident nodes without a random seed dependency.
    for (let i = 0; i < count; i++) {
      positions[i * 2]! += Math.cos(i * 2.399963229728653) * 0.02;
      positions[i * 2 + 1]! += Math.sin(i * 2.399963229728653) * 0.02;
    }
    const ideal = 105, cellSize = ideal * 2;
    for (let iteration = 0; iteration < iterations; iteration++) {
      movement.fill(0);
      const cells = new Map<string, { x: number; y: number; count: number }>();
      let centerX = 0, centerY = 0;
      for (let i = 0; i < count; i++) {
        const x = positions[i * 2]!, y = positions[i * 2 + 1]!;
        centerX += x; centerY += y;
        const key = `${Math.floor(x / cellSize)},${Math.floor(y / cellSize)}`;
        const cell = cells.get(key);
        if (cell) { cell.x += x; cell.y += y; cell.count++; }
        else cells.set(key, { x, y, count: 1 });
      }
      centerX /= count; centerY /= count;
      for (let i = 0; i < count; i++) {
        const x = positions[i * 2]!, y = positions[i * 2 + 1]!, cx = Math.floor(x / cellSize), cy = Math.floor(y / cellSize);
        let fx = (centerX - x) * 0.002, fy = (centerY - y) * 0.002;
        // Aggregate neighboring cells, including a self-corrected aggregate of this node's cell.
        // Constant work per node; dense cells never degrade into pairwise O(N²) work.
        for (let ox = -1; ox <= 1; ox++) for (let oy = -1; oy <= 1; oy++) {
          const cell = cells.get(`${cx + ox},${cy + oy}`);
          if (!cell) continue;
          const same = ox === 0 && oy === 0, others = cell.count - (same ? 1 : 0);
          if (!others) continue;
          let dx = x - (cell.x - (same ? x : 0)) / others, dy = y - (cell.y - (same ? y : 0)) / others;
          if (dx * dx + dy * dy < 0.01) { dx = Math.cos(i * 2.399963229728653); dy = Math.sin(i * 2.399963229728653); }
          const distanceSquared = Math.max(400, dx * dx + dy * dy);
          // Damping bounds the contribution of a dense cell while still expanding overlaps.
          const force = ideal * ideal * Math.sqrt(others) / distanceSquared;
          fx += dx * force; fy += dy * force;
        }
        movement[i * 2] = fx; movement[i * 2 + 1] = fy;
      }
      for (let e = 0; e < edges.length; e += 2) {
        const a = edges[e]!, b = edges[e + 1]!;
        if (a === b) continue;
        const dx = positions[b * 2]! - positions[a * 2]!, dy = positions[b * 2 + 1]! - positions[a * 2 + 1]!;
        const distance = Math.max(1, Math.hypot(dx, dy));
        const force = (distance - ideal) / distance * 0.18 / Math.sqrt(Math.max(degrees[a]!, degrees[b]!, 1));
        movement[a * 2]! += dx * force; movement[a * 2 + 1]! += dy * force;
        movement[b * 2]! -= dx * force; movement[b * 2 + 1]! -= dy * force;
      }
      const temperature = 18 * (1 - iteration / iterations) + 0.25;
      for (let i = 0; i < count; i++) {
        const dx = movement[i * 2]!, dy = movement[i * 2 + 1]!, length = Math.max(1, Math.hypot(dx, dy));
        const scale = Math.min(1, temperature / length);
        positions[i * 2]! += dx * scale; positions[i * 2 + 1]! += dy * scale;
      }
    }
    scope.postMessage({ positions }, [positions.buffer]);
  } catch (error) {
    scope.postMessage({ error: error instanceof Error ? error.message : String(error) });
  }
};
