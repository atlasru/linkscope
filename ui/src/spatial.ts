/** World-space indexes. They do not depend on the renderer or entity schema. */
export interface Point { id: string; x: number; y: number }
export interface Bounds { minX: number; minY: number; maxX: number; maxY: number }

export function intersects(a: Bounds, b: Bounds): boolean {
  return a.minX <= b.maxX && a.maxX >= b.minX && a.minY <= b.maxY && a.maxY >= b.minY;
}

export function pointSegmentDistanceSquared(px: number, py: number, ax: number, ay: number, bx: number, by: number): number {
  const dx = bx - ax, dy = by - ay;
  const length = dx * dx + dy * dy;
  const t = length === 0 ? 0 : Math.max(0, Math.min(1, ((px - ax) * dx + (py - ay) * dy) / length));
  return (px - ax - t * dx) ** 2 + (py - ay - t * dy) ** 2;
}

/** A sparse grid: moving a node changes two buckets at most. */
export class PointIndex {
  private readonly points = new Map<string, Point>();
  private readonly buckets = new Map<string, { x: number; y: number; points: Map<string, Point> }>();
  constructor(readonly cellSize = 160) {
    if (!Number.isFinite(cellSize) || cellSize <= 0) throw new Error('Invalid spatial cell size');
  }
  get size(): number { return this.points.size; }
  clear(): void { this.points.clear(); this.buckets.clear(); }
  get(id: string): Point | undefined { return this.points.get(id); }
  private key(x: number, y: number): string { return `${x},${y}`; }
  remove(id: string): void {
    const old = this.points.get(id);
    if (!old) return;
    const key = this.key(Math.floor(old.x / this.cellSize), Math.floor(old.y / this.cellSize));
    const bucket = this.buckets.get(key);
    bucket?.points.delete(id);
    if (bucket?.points.size === 0) this.buckets.delete(key);
    this.points.delete(id);
  }
  set(id: string, x: number, y: number): void {
    if (!Number.isFinite(x) || !Number.isFinite(y)) throw new Error('Spatial coordinates must be finite');
    const old = this.points.get(id);
    if (old?.x === x && old.y === y) return;
    this.remove(id);
    const cx = Math.floor(x / this.cellSize), cy = Math.floor(y / this.cellSize), key = this.key(cx, cy);
    let bucket = this.buckets.get(key);
    if (!bucket) { bucket = { x: cx, y: cy, points: new Map() }; this.buckets.set(key, bucket); }
    const point = { id, x, y };
    this.points.set(id, point);
    bucket.points.set(id, point);
  }
  query(bounds: Bounds): Point[] {
    const found: Point[] = [];
    const minX = Math.floor(bounds.minX / this.cellSize), maxX = Math.floor(bounds.maxX / this.cellSize);
    const minY = Math.floor(bounds.minY / this.cellSize), maxY = Math.floor(bounds.maxY / this.cellSize);
    const append = (bucket: { points: Map<string, Point> }): void => {
      for (const p of bucket.points.values()) {
        if (p.x >= bounds.minX && p.x <= bounds.maxX && p.y >= bounds.minY && p.y <= bounds.maxY) found.push(p);
      }
    };
    // A far zoom must iterate occupied cells, not billions of empty cells.
    if ((maxX - minX + 1) * (maxY - minY + 1) > this.buckets.size * 2) {
      for (const bucket of this.buckets.values()) {
        if (bucket.x >= minX && bucket.x <= maxX && bucket.y >= minY && bucket.y <= maxY) append(bucket);
      }
    } else {
      for (let x = minX; x <= maxX; x++) for (let y = minY; y <= maxY; y++) {
        const bucket = this.buckets.get(this.key(x, y));
        if (bucket) append(bucket);
      }
    }
    return found;
  }
  nearest(x: number, y: number, radius: number, include: (id: string) => boolean = () => true): Point | undefined {
    let best: Point | undefined, bestDistance = radius * radius;
    for (const point of this.query({ minX: x - radius, minY: y - radius, maxX: x + radius, maxY: y + radius })) {
      const distance = (point.x - x) ** 2 + (point.y - y) ** 2;
      if (distance <= bestDistance && include(point.id)) { bestDistance = distance; best = point; }
    }
    return best;
  }
  bounds(): Bounds | undefined {
    if (!this.points.size) return undefined;
    const result = { minX: Infinity, minY: Infinity, maxX: -Infinity, maxY: -Infinity };
    for (const p of this.points.values()) {
      result.minX = Math.min(result.minX, p.x); result.minY = Math.min(result.minY, p.y);
      result.maxX = Math.max(result.maxX, p.x); result.maxY = Math.max(result.maxY, p.y);
    }
    return result;
  }
}

interface TreeNode {
  bounds: Bounds;
  height: number;
  id?: string;
  parent?: TreeNode;
  left?: TreeNode;
  right?: TreeNode;
}
function union(a: Bounds, b: Bounds): Bounds {
  return { minX: Math.min(a.minX, b.minX), minY: Math.min(a.minY, b.minY), maxX: Math.max(a.maxX, b.maxX), maxY: Math.max(a.maxY, b.maxY) };
}
function perimeter(a: Bounds): number { return 2 * (a.maxX - a.minX + a.maxY - a.minY); }
function refresh(node: TreeNode): void {
  if (!node.left || !node.right) return;
  node.bounds = union(node.left.bounds, node.right.bounds);
  node.height = 1 + Math.max(node.left.height, node.right.height);
}

/** Balanced dynamic AABB hierarchy. Long edges do not occupy thousands of grid cells. */
export class BoundsIndex {
  private root?: TreeNode;
  private readonly leaves = new Map<string, TreeNode>();
  get size(): number { return this.leaves.size; }
  clear(): void { this.root = undefined; this.leaves.clear(); }
  /** Morton ordered, height-balanced bulk construction avoids N dynamic insertions on project open. */
  load(entries: Iterable<readonly [string, Bounds]>): void {
    this.clear();
    const rows: { node: TreeNode; key: number }[] = [];
    let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
    for (const [id, bounds] of entries) {
      if (![bounds.minX, bounds.minY, bounds.maxX, bounds.maxY].every(Number.isFinite) || bounds.minX > bounds.maxX || bounds.minY > bounds.maxY) throw new Error('Invalid spatial bounds');
      if (this.leaves.has(id)) throw new Error('Duplicate spatial identifier');
      const node: TreeNode = { id, bounds: { ...bounds }, height: 0 };
      this.leaves.set(id, node); rows.push({ node, key: 0 });
      const x = bounds.minX / 2 + bounds.maxX / 2, y = bounds.minY / 2 + bounds.maxY / 2;
      minX = Math.min(minX, x); minY = Math.min(minY, y); maxX = Math.max(maxX, x); maxY = Math.max(maxY, y);
    }
    const spread = (value: number): number => {
      value = (value | (value << 8)) & 0x00ff00ff;
      value = (value | (value << 4)) & 0x0f0f0f0f;
      value = (value | (value << 2)) & 0x33333333;
      return (value | (value << 1)) & 0x55555555;
    };
    const width = Math.max(1, maxX - minX), height = Math.max(1, maxY - minY);
    for (const row of rows) {
      const b = row.node.bounds;
      const x = Math.round(((b.minX / 2 + b.maxX / 2) - minX) / width * 65535);
      const y = Math.round(((b.minY / 2 + b.maxY / 2) - minY) / height * 65535);
      row.key = (spread(x) | (spread(y) << 1)) >>> 0;
    }
    rows.sort((a, b) => a.key - b.key);
    const build = (start: number, end: number): TreeNode => {
      if (end - start === 1) return rows[start]!.node;
      const middle = start + Math.floor((end - start) / 2), left = build(start, middle), right = build(middle, end);
      const node: TreeNode = { bounds: union(left.bounds, right.bounds), height: Math.max(left.height, right.height) + 1, left, right };
      left.parent = node; right.parent = node; return node;
    };
    this.root = rows.length ? build(0, rows.length) : undefined;
  }
  private replace(old: TreeNode, next: TreeNode): void {
    next.parent = old.parent;
    if (!old.parent) this.root = next;
    else if (old.parent.left === old) old.parent.left = next;
    else old.parent.right = next;
  }
  private balance(a: TreeNode): TreeNode {
    if (!a.left || !a.right || a.height < 2) return a;
    const b = a.left, c = a.right, difference = c.height - b.height;
    if (difference > 1 && c.left && c.right) {
      const f = c.left, g = c.right;
      this.replace(a, c); c.left = a; a.parent = c;
      if (f.height > g.height) { c.right = f; f.parent = c; a.right = g; g.parent = a; }
      else { c.right = g; g.parent = c; a.right = f; f.parent = a; }
      refresh(a); refresh(c); return c;
    }
    if (difference < -1 && b.left && b.right) {
      const d = b.left, e = b.right;
      this.replace(a, b); b.left = a; a.parent = b;
      if (d.height > e.height) { b.right = d; d.parent = b; a.left = e; e.parent = a; }
      else { b.right = e; e.parent = b; a.left = d; d.parent = a; }
      refresh(a); refresh(b); return b;
    }
    return a;
  }
  private repair(from?: TreeNode): void {
    let node = from;
    while (node) { refresh(node); node = this.balance(node); node = node.parent; }
  }
  set(id: string, bounds: Bounds): void {
    if (![bounds.minX, bounds.minY, bounds.maxX, bounds.maxY].every(Number.isFinite) || bounds.minX > bounds.maxX || bounds.minY > bounds.maxY) throw new Error('Invalid spatial bounds');
    const previous = this.leaves.get(id);
    if (previous && previous.bounds.minX === bounds.minX && previous.bounds.minY === bounds.minY && previous.bounds.maxX === bounds.maxX && previous.bounds.maxY === bounds.maxY) return;
    this.remove(id);
    const leaf: TreeNode = { id, bounds: { ...bounds }, height: 0 };
    this.leaves.set(id, leaf);
    if (!this.root) { this.root = leaf; return; }
    let sibling = this.root;
    while (sibling.left && sibling.right) {
      const combined = union(sibling.bounds, bounds);
      const inherited = 2 * (perimeter(combined) - perimeter(sibling.bounds));
      const cost = (child: TreeNode): number => {
        const merged = perimeter(union(child.bounds, bounds));
        return (child.left ? merged - perimeter(child.bounds) : merged) + inherited;
      };
      const leftCost = cost(sibling.left), rightCost = cost(sibling.right);
      sibling = leftCost < rightCost ? sibling.left : sibling.right;
    }
    const parent: TreeNode = { bounds: union(bounds, sibling.bounds), height: sibling.height + 1, left: sibling, right: leaf };
    this.replace(sibling, parent); sibling.parent = parent; leaf.parent = parent;
    this.repair(parent);
  }
  remove(id: string): void {
    const leaf = this.leaves.get(id);
    if (!leaf) return;
    this.leaves.delete(id);
    const parent = leaf.parent;
    if (!parent) { this.root = undefined; return; }
    const sibling = parent.left === leaf ? parent.right : parent.left;
    if (!sibling) throw new Error('Broken spatial tree');
    this.replace(parent, sibling);
    this.repair(sibling.parent);
  }
  /** Limit is for visual LOD only. Picking must use the default unlimited query. */
  query(bounds: Bounds, limit = Infinity): string[] {
    const found: string[] = [];
    if (!this.root || limit <= 0) return found;
    const stack = [this.root];
    while (stack.length && found.length < limit) {
      const node = stack.pop()!;
      if (!intersects(bounds, node.bounds)) continue;
      if (node.id !== undefined) found.push(node.id);
      else { if (node.left) stack.push(node.left); if (node.right) stack.push(node.right); }
    }
    return found;
  }
  /** One representative per spatial subtree, rather than drawing a prefix from one corner. */
  sample(bounds: Bounds, limit: number): string[] {
    if (!this.root || limit < 1 || !intersects(bounds, this.root.bounds)) return [];
    let frontier = [this.root];
    while (frontier.length < limit) {
      const next: TreeNode[] = []; let split = false;
      for (const node of frontier) {
        if (node.id !== undefined) next.push(node);
        else {
          split = true;
          if (node.left && intersects(bounds, node.left.bounds)) next.push(node.left);
          if (node.right && intersects(bounds, node.right.bounds)) next.push(node.right);
        }
      }
      frontier = next;
      if (!split || !frontier.length) break;
    }
    const found: string[] = [], count = Math.min(limit, frontier.length);
    for (let i = 0; i < count; i++) {
      const stack = [frontier[Math.floor(i * frontier.length / count)]!];
      while (stack.length) {
        const node = stack.pop()!;
        if (!intersects(bounds, node.bounds)) continue;
        if (node.id !== undefined) { found.push(node.id); break; }
        if (node.left) stack.push(node.left);
        if (node.right) stack.push(node.right);
      }
    }
    return found;
  }
  /** Exposed for invariant tests rather than renderer behavior. */
  validate(): void {
    const seen = new Set<string>();
    const visit = (node?: TreeNode): number => {
      if (!node) return -1;
      if (node.id !== undefined) {
        if (node.left || node.right || node.height !== 0) throw new Error('Invalid leaf');
        seen.add(node.id); return 0;
      }
      if (!node.left || !node.right || node.left.parent !== node || node.right.parent !== node) throw new Error('Invalid parent');
      const left = visit(node.left), right = visit(node.right);
      const expected = union(node.left.bounds, node.right.bounds);
      if (node.height !== 1 + Math.max(left, right) || Math.abs(left - right) > 1 || Object.keys(expected).some(key => expected[key as keyof Bounds] !== node.bounds[key as keyof Bounds])) throw new Error('Invalid bounds hierarchy');
      return node.height;
    };
    if (this.root?.parent) throw new Error('Invalid root');
    visit(this.root);
    if (seen.size !== this.leaves.size) throw new Error('Lost spatial leaves');
  }
}
