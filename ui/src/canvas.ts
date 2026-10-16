import type { Collection, Delta, Entity, Relationship } from './types';
import { BoundsIndex, PointIndex, intersects, pointSegmentDistanceSquared, type Bounds, type Point } from './spatial';

export interface Camera { x: number; y: number; zoom: number }
export interface Position { id: string; x: number; y: number }
export interface CanvasCallbacks {
  select(ids: string[], edgeId?: string): void;
  move(positions: Position[]): void;
  create(x: number, y: number): void;
  connect(source: string, target: string): void;
  context(x: number, y: number, id?: string, edgeId?: string): void;
  camera?(): void;
}
interface Curve {
  kind: 0 | 1 | 2;
  x0: number; y0: number; x1: number; y1: number;
  cx: number; cy: number; dx: number; dy: number;
}
type Gesture =
  | { kind: 'pan'; clientX: number; clientY: number; camera: Camera; moved: boolean; clear: boolean }
  | { kind: 'nodes'; clientX: number; clientY: number; start: Point; origins: Position[]; moved: boolean }
  | { kind: 'box'; start: Point; end: Point; original: Set<string> };
const clamp = (value: number, min: number, max: number): number => Math.max(min, Math.min(max, value));
const pairKey = (source: string, target: string): string => JSON.stringify(source < target ? [source, target] : [target, source]);
const curveBounds = (c: Curve): Bounds => ({
  minX: Math.min(c.x0, c.x1, c.cx, c.dx), minY: Math.min(c.y0, c.y1, c.cy, c.dy),
  maxX: Math.max(c.x0, c.x1, c.cx, c.dx), maxY: Math.max(c.y0, c.y1, c.cy, c.dy),
});
function at(curve: Curve, t: number): { x: number; y: number } {
  const u = 1 - t;
  if (curve.kind === 0) return { x: u * curve.x0 + t * curve.x1, y: u * curve.y0 + t * curve.y1 };
  if (curve.kind === 1) return { x: u * u * curve.x0 + 2 * u * t * curve.cx + t * t * curve.x1, y: u * u * curve.y0 + 2 * u * t * curve.cy + t * t * curve.y1 };
  return { x: u ** 3 * curve.x0 + 3 * u * u * t * curve.cx + 3 * u * t * t * curve.dx + t ** 3 * curve.x1, y: u ** 3 * curve.y0 + 3 * u * u * t * curve.cy + 3 * u * t * t * curve.dy + t ** 3 * curve.y1 };
}
function curveDistanceSquared(curve: Curve, x: number, y: number): number {
  const steps = curve.kind === 0 ? 1 : curve.kind === 1 ? 20 : 36;
  let last = { x: curve.x0, y: curve.y0 }, best = Infinity;
  for (let i = 1; i <= steps; i++) {
    const next = at(curve, i / steps);
    best = Math.min(best, pointSegmentDistanceSquared(x, y, last.x, last.y, next.x, next.y)); last = next;
  }
  return best;
}

/** Render cache and interactions only: mutations reach the authoritative graph through callbacks. */
export class GraphCanvas {
  readonly element = document.createElement('canvas');
  private readonly context: CanvasRenderingContext2D;
  private readonly nodes = new PointIndex();
  private readonly edgeIndex = new BoundsIndex();
  private entities = new Map<string, Entity>();
  private relationships = new Map<string, Relationship>();
  private collections = new Map<string, Collection>();
  private readonly incident = new Map<string, Set<string>>();
  private readonly groups = new Map<string, Set<string>>();
  private readonly lanes = new Map<string, number>();
  private readonly curves = new Map<string, Curve>();
  private readonly collectionMembership = new Map<string, Set<string>>();
  private readonly collectionBounds = new Map<string, Bounds>();
  private readonly dirtyEdges = new Set<string>();
  private readonly dirtyCollections = new Set<string>();
  private selected = new Set<string>();
  private selectedEdge?: string;
  private filterQuery = '';
  private filterKind = '';
  private included?: Set<string>;
  private camera: Camera = { x: 0, y: 0, zoom: 1 };
  private tool: 'select' | 'link' = 'select';
  private linkSource?: string;
  private gesture?: Gesture;
  private space = false;
  private width = 1;
  private height = 1;
  private frame = 0;
  private disposed = false;
  private readonly resizeObserver: ResizeObserver;
  private readonly listeners = new AbortController();
  private worker?: Worker;
  private rejectLayout?: (error: Error) => void;
  private nodeLimit = 20_000;
  private edgeLimit = 7_000;
  private refinementAllowance = 3;
  private refinementDirection?: 'up' | 'down';
  private stats = { visibleNodes: 0, drawnNodes: 0, drawnEdges: 0, drawnLabels: 0, nodeBudget: 20_000, edgeBudget: 7_000, frameMs: 0 };

  constructor(private readonly container: HTMLElement, private readonly callbacks: CanvasCallbacks) {
    const context = this.element.getContext('2d', { alpha: false });
    if (!context) throw new Error('Canvas2D is unavailable in this WebView');
    this.context = context;
    this.element.tabIndex = 0;
    this.element.setAttribute('role', 'application');
    this.element.setAttribute('aria-label', 'Graph editor');
    this.element.setAttribute('aria-description', 'Drag nodes to move. Drag empty space to pan. Shift drag selects a region. Double click empty space to create. Use the entity list to select with the keyboard.');
    this.element.style.cssText = 'display:block;width:100%;height:100%;touch-action:none;outline:none;';
    container.append(this.element);
    const options = { signal: this.listeners.signal };
    this.element.addEventListener('pointerdown', this.pointerDown, options);
    this.element.addEventListener('pointermove', this.pointerMove, options);
    this.element.addEventListener('pointerup', this.pointerUp, options);
    this.element.addEventListener('pointercancel', this.pointerCancel, options);
    this.element.addEventListener('lostpointercapture', this.lostPointerCapture, options);
    this.element.addEventListener('wheel', this.wheel, { ...options, passive: false });
    this.element.addEventListener('dblclick', this.doubleClick, options);
    this.element.addEventListener('contextmenu', this.contextMenu, options);
    this.element.addEventListener('keydown', this.keyDown, options);
    this.element.addEventListener('keyup', this.keyUp, options);
    this.element.addEventListener('blur', () => { this.space = false; this.updateCursor(); }, options);
    this.resizeObserver = new ResizeObserver(this.resize);
    this.resizeObserver.observe(container);
    this.resize();
  }

  setData(entities: Map<string, Entity>, relationships: Map<string, Relationship>, collections: Map<string, Collection>): void {
    this.cancelLayout();
    this.cancelGesture();
    this.nodeLimit = 20_000; this.edgeLimit = 7_000;
    // Own map structure, but share immutable record objects. A root cache may mutate its maps before a delta arrives.
    this.entities = new Map(entities); this.relationships = new Map(relationships); this.collections = new Map(collections);
    this.nodes.clear(); this.edgeIndex.clear(); this.incident.clear(); this.groups.clear(); this.lanes.clear(); this.curves.clear();
    this.collectionMembership.clear(); this.collectionBounds.clear(); this.dirtyEdges.clear(); this.dirtyCollections.clear();
    for (const entity of entities.values()) this.nodes.set(entity.id, entity.x, entity.y);
    for (const edge of relationships.values()) this.addEdgeMembership(edge);
    for (const group of this.groups.values()) this.assignLanes(group);
    for (const id of this.relationships.keys()) this.updateCurve(id, false);
    const curves = this.curves;
    this.edgeIndex.load((function* (): Generator<readonly [string, Bounds]> { for (const [id, curve] of curves) yield [id, curveBounds(curve)]; })());
    for (const collection of collections.values()) this.addCollection(collection);
    this.refilter(); this.invalidate();
  }

  update(delta: Delta): void {
    // A result computed against an older topology/position snapshot must not overwrite a later edit.
    if (this.worker) this.cancelLayout();
    const patch = delta.patch, changedGroups = new Set<string>();
    for (const id of patch.removeRelationships ?? []) this.removeEdge(id, changedGroups);
    for (const id of patch.removeEntities ?? []) {
      for (const edgeId of [...this.incident.get(id) ?? []]) this.removeEdge(edgeId, changedGroups);
      this.entities.delete(id); this.nodes.remove(id); this.incident.delete(id); this.selected.delete(id);
      if (this.linkSource === id) this.linkSource = undefined;
      for (const collectionId of this.collectionMembership.get(id) ?? []) this.dirtyCollections.add(collectionId);
      this.collectionMembership.delete(id);
    }
    for (const entity of patch.entities ?? []) {
      const old = this.nodes.get(entity.id), moved = !old || old.x !== entity.x || old.y !== entity.y;
      this.entities.set(entity.id, entity); this.nodes.set(entity.id, entity.x, entity.y);
      if (moved) {
        for (const id of this.incident.get(entity.id) ?? []) this.dirtyEdges.add(id);
        for (const id of this.collectionMembership.get(entity.id) ?? []) this.dirtyCollections.add(id);
      }
    }
    for (const edge of patch.relationships ?? []) {
      this.removeEdge(edge.id, changedGroups, false); this.relationships.set(edge.id, edge); this.addEdgeMembership(edge);
      changedGroups.add(pairKey(edge.source, edge.target)); this.dirtyEdges.add(edge.id);
    }
    for (const key of changedGroups) {
      const group = this.groups.get(key);
      if (group) { this.assignLanes(group); for (const id of group) this.dirtyEdges.add(id); }
    }
    for (const id of patch.removeCollections ?? []) this.removeCollection(id);
    for (const collection of patch.collections ?? []) { this.removeCollection(collection.id); this.collections.set(collection.id, collection); this.addCollection(collection); }
    if (this.included) {
      for (const id of patch.removeEntities ?? []) this.included.delete(id);
      for (const entity of patch.entities ?? []) {
        if (this.matches(entity)) this.included.add(entity.id); else this.included.delete(entity.id);
        for (const id of this.collectionMembership.get(entity.id) ?? []) this.dirtyCollections.add(id);
      }
    }
    this.invalidate();
  }

  setSelection(ids: Set<string>, edgeId?: string): void {
    this.selected = new Set(ids); this.selectedEdge = edgeId; this.invalidate();
  }
  setFilter(query: string, kind: string): void {
    this.filterQuery = query.trim().toLocaleLowerCase(); this.filterKind = kind; this.refilter();
    for (const id of this.collections.keys()) this.dirtyCollections.add(id);
    this.invalidate();
  }
  private matches(entity: Entity): boolean {
    return (!this.filterKind || this.filterKind === 'all' || entity.kind === this.filterKind) &&
      (!this.filterQuery || `${entity.label}\n${entity.kind}\n${entity.notes}\n${JSON.stringify(entity.properties)}`.toLocaleLowerCase().includes(this.filterQuery));
  }
  private refilter(): void {
    if (!this.filterQuery && (!this.filterKind || this.filterKind === 'all')) { this.included = undefined; return; }
    this.included = new Set([...this.entities.values()].filter(entity => this.matches(entity)).map(entity => entity.id));
  }
  private isVisible = (id: string): boolean => !this.included || this.included.has(id);
  private edgeVisible(edge: Relationship): boolean { return this.isVisible(edge.source) && this.isVisible(edge.target); }
  private addEdgeMembership(edge: Relationship): void {
    for (const nodeId of new Set([edge.source, edge.target])) {
      let adjacent = this.incident.get(nodeId);
      if (!adjacent) { adjacent = new Set(); this.incident.set(nodeId, adjacent); }
      adjacent.add(edge.id);
    }
    const key = pairKey(edge.source, edge.target);
    let group = this.groups.get(key);
    if (!group) { group = new Set(); this.groups.set(key, group); }
    group.add(edge.id);
  }
  private removeEdge(id: string, changedGroups: Set<string>, clearSelection = true): void {
    const old = this.relationships.get(id);
    if (old) {
      this.incident.get(old.source)?.delete(id); this.incident.get(old.target)?.delete(id);
      if (!this.incident.get(old.source)?.size) this.incident.delete(old.source);
      if (!this.incident.get(old.target)?.size) this.incident.delete(old.target);
      const key = pairKey(old.source, old.target), group = this.groups.get(key);
      group?.delete(id); if (!group?.size) this.groups.delete(key); changedGroups.add(key);
    }
    this.relationships.delete(id); this.curves.delete(id); this.edgeIndex.remove(id); this.lanes.delete(id); this.dirtyEdges.delete(id);
    if (clearSelection && this.selectedEdge === id) this.selectedEdge = undefined;
  }
  private assignLanes(group: Set<string>): void {
    const ids = [...group].sort();
    ids.forEach((id, i) => this.lanes.set(id, (i - (ids.length - 1) / 2) * 30));
  }
  private updateCurve(id: string, index = true): void {
    const edge = this.relationships.get(id);
    if (!edge) return;
    const source = this.nodes.get(edge.source), target = this.nodes.get(edge.target);
    if (!source || !target) { this.curves.delete(id); this.edgeIndex.remove(id); return; }
    const lane = this.lanes.get(id) ?? 0;
    let curve: Curve;
    if (edge.source === edge.target) {
      const group = this.groups.get(pairKey(edge.source, edge.target));
      const span = 30 + (lane / 30 + ((group?.size ?? 1) - 1) / 2) * 15;
      curve = { kind: 2, x0: source.x + 4, y0: source.y - 5, x1: source.x - 4, y1: source.y - 5,
        cx: source.x + span * 1.3, cy: source.y - span * 2, dx: source.x - span * 1.3, dy: source.y - span * 2 };
    } else {
      const sign = edge.source < edge.target ? 1 : -1;
      const vx = (target.x - source.x) * sign, vy = (target.y - source.y) * sign, length = Math.max(1, Math.hypot(vx, vy));
      const cx = (source.x + target.x) / 2 - vy / length * lane, cy = (source.y + target.y) / 2 + vx / length * lane;
      curve = { kind: lane === 0 ? 0 : 1, x0: source.x, y0: source.y, x1: target.x, y1: target.y, cx, cy, dx: cx, dy: cy };
    }
    this.curves.set(id, curve); if (index) this.edgeIndex.set(id, curveBounds(curve));
  }
  private addCollection(collection: Collection): void {
    for (const id of collection.members) {
      let memberships = this.collectionMembership.get(id);
      if (!memberships) { memberships = new Set(); this.collectionMembership.set(id, memberships); }
      memberships.add(collection.id);
    }
    this.dirtyCollections.add(collection.id);
  }
  private removeCollection(id: string): void {
    for (const member of this.collections.get(id)?.members ?? []) {
      const memberships = this.collectionMembership.get(member); memberships?.delete(id);
      if (!memberships?.size) this.collectionMembership.delete(member);
    }
    this.collections.delete(id); this.collectionBounds.delete(id); this.dirtyCollections.delete(id);
  }
  private updateCollectionBounds(id: string): void {
    const collection = this.collections.get(id);
    if (!collection) return;
    let found = false;
    const b = { minX: Infinity, minY: Infinity, maxX: -Infinity, maxY: -Infinity };
    for (const member of collection.members) {
      const p = this.nodes.get(member);
      if (!p || !this.isVisible(member)) continue;
      found = true; b.minX = Math.min(b.minX, p.x); b.minY = Math.min(b.minY, p.y); b.maxX = Math.max(b.maxX, p.x); b.maxY = Math.max(b.maxY, p.y);
    }
    if (found) this.collectionBounds.set(id, { minX: b.minX - 26, minY: b.minY - 32, maxX: b.maxX + 26, maxY: b.maxY + 26 });
    else this.collectionBounds.delete(id);
  }

  getCamera(): Camera { return { ...this.camera }; }
  setCamera(camera: Camera): void {
    if (![camera.x, camera.y, camera.zoom].every(Number.isFinite)) return;
    this.camera = { x: camera.x, y: camera.y, zoom: clamp(camera.zoom, 0.015, 8) }; this.cameraChanged();
  }
  fit(): void {
    let bounds: Bounds | undefined;
    if (this.included) {
      for (const id of this.included) {
        const p = this.nodes.get(id); if (!p) continue;
        if (!bounds) bounds = { minX: p.x, minY: p.y, maxX: p.x, maxY: p.y };
        else { bounds.minX = Math.min(bounds.minX, p.x); bounds.minY = Math.min(bounds.minY, p.y); bounds.maxX = Math.max(bounds.maxX, p.x); bounds.maxY = Math.max(bounds.maxY, p.y); }
      }
    } else bounds = this.nodes.bounds();
    this.camera = bounds ? { x: (bounds.minX + bounds.maxX) / 2, y: (bounds.minY + bounds.maxY) / 2,
      zoom: clamp(Math.min((this.width - 100) / Math.max(100, bounds.maxX - bounds.minX), (this.height - 100) / Math.max(100, bounds.maxY - bounds.minY)), 0.015, 1.6) } : { x: 0, y: 0, zoom: 1 };
    this.cameraChanged();
  }
  zoomBy(factor: number): void {
    if (Number.isFinite(factor) && factor > 0) { this.camera.zoom = clamp(this.camera.zoom * factor, 0.015, 8); this.cameraChanged(); }
  }
  centerOn(id: string): void {
    const p = this.nodes.get(id);
    if (p) { this.camera.x = p.x; this.camera.y = p.y; this.camera.zoom = Math.max(0.7, this.camera.zoom); this.cameraChanged(); }
  }
  setTool(tool: 'select' | 'link'): void { this.tool = tool; this.linkSource = undefined; this.updateCursor(); this.invalidate(); }
  private cameraChanged(): void { this.invalidate(); this.callbacks.camera?.(); }
  private resize = (): void => {
    const bounds = this.container.getBoundingClientRect(); this.width = Math.max(1, bounds.width); this.height = Math.max(1, bounds.height);
    const ratio = Math.min(2.5, window.devicePixelRatio || 1);
    this.element.width = Math.ceil(this.width * ratio); this.element.height = Math.ceil(this.height * ratio); this.invalidate();
  };
  private world(clientX: number, clientY: number): Point {
    const bounds = this.element.getBoundingClientRect();
    return { id: '', x: (clientX - bounds.left - this.width / 2) / this.camera.zoom + this.camera.x, y: (clientY - bounds.top - this.height / 2) / this.camera.zoom + this.camera.y };
  }
  private viewport(padding = 0): Bounds {
    return { minX: this.camera.x - this.width / (2 * this.camera.zoom) - padding, minY: this.camera.y - this.height / (2 * this.camera.zoom) - padding,
      maxX: this.camera.x + this.width / (2 * this.camera.zoom) + padding, maxY: this.camera.y + this.height / (2 * this.camera.zoom) + padding };
  }
  private pickNode(point: Point): Point | undefined { return this.nodes.nearest(point.x, point.y, 11 / this.camera.zoom, this.isVisible); }
  private flushGeometry(): void {
    for (const id of this.dirtyEdges) this.updateCurve(id);
    this.dirtyEdges.clear();
    for (const id of this.dirtyCollections) this.updateCollectionBounds(id);
    this.dirtyCollections.clear();
  }
  private pickEdge(point: Point): string | undefined {
    this.flushGeometry();
    const radius = 7 / this.camera.zoom;
    let best: string | undefined, bestDistance = radius * radius;
    for (const id of this.edgeIndex.query({ minX: point.x - radius, minY: point.y - radius, maxX: point.x + radius, maxY: point.y + radius })) {
      const edge = this.relationships.get(id), curve = this.curves.get(id);
      if (!edge || !curve || !this.edgeVisible(edge)) continue;
      const distance = curveDistanceSquared(curve, point.x, point.y);
      if (distance < bestDistance) { bestDistance = distance; best = id; }
    }
    return best;
  }
  private select(ids: Set<string>, edgeId?: string): void { this.setSelection(ids, edgeId); this.callbacks.select([...ids], edgeId); }

  private pointerDown = (event: PointerEvent): void => {
    if (event.button !== 0 && event.button !== 1) return;
    if (this.gesture) return;
    event.preventDefault(); this.element.focus({ preventScroll: true }); this.element.setPointerCapture(event.pointerId);
    const point = this.world(event.clientX, event.clientY), node = this.pickNode(point);
    if (event.button === 1 || this.space || event.altKey) {
      this.gesture = { kind: 'pan', clientX: event.clientX, clientY: event.clientY, camera: { ...this.camera }, moved: false, clear: false };
    } else if (node) {
      if (this.tool === 'link') {
        if (this.linkSource) { this.callbacks.connect(this.linkSource, node.id); this.linkSource = undefined; }
        else { this.linkSource = node.id; this.select(new Set([node.id])); }
        this.invalidate(); return;
      }
      const ids = new Set(event.shiftKey || this.selected.has(node.id) ? this.selected : []);
      if (event.shiftKey && ids.has(node.id)) ids.delete(node.id); else ids.add(node.id);
      this.select(ids);
      const origins: Position[] = [];
      for (const id of ids) { const p = this.nodes.get(id); if (p) origins.push({ ...p }); }
      this.gesture = { kind: 'nodes', clientX: event.clientX, clientY: event.clientY, start: point, origins, moved: false };
    } else if (event.shiftKey) {
      this.gesture = { kind: 'box', start: point, end: point, original: new Set(this.selected) };
    } else {
      const edge = this.pickEdge(point);
      if (edge) { this.select(new Set(), edge); return; }
      this.linkSource = undefined;
      this.gesture = { kind: 'pan', clientX: event.clientX, clientY: event.clientY, camera: { ...this.camera }, moved: false, clear: true };
    }
    this.updateCursor(); this.invalidate();
  };
  private pointerMove = (event: PointerEvent): void => {
    const gesture = this.gesture;
    if (!gesture) return;
    const point = this.world(event.clientX, event.clientY);
    if (gesture.kind === 'pan') {
      const dx = event.clientX - gesture.clientX, dy = event.clientY - gesture.clientY;
      if (Math.hypot(dx, dy) > 3) gesture.moved = true;
      if (gesture.moved) { this.camera.x = gesture.camera.x - dx / this.camera.zoom; this.camera.y = gesture.camera.y - dy / this.camera.zoom; this.cameraChanged(); }
    } else if (gesture.kind === 'nodes') {
      if (Math.hypot(event.clientX - gesture.clientX, event.clientY - gesture.clientY) > 3) gesture.moved = true;
      if (gesture.moved) {
        const dx = point.x - gesture.start.x, dy = point.y - gesture.start.y;
        for (const origin of gesture.origins) this.movePoint(origin.id, origin.x + dx, origin.y + dy);
      }
    } else gesture.end = point;
    this.invalidate();
  };
  private movePoint(id: string, x: number, y: number): void {
    this.nodes.set(id, x, y);
    for (const edge of this.incident.get(id) ?? []) this.dirtyEdges.add(edge);
    for (const collection of this.collectionMembership.get(id) ?? []) this.dirtyCollections.add(collection);
  }
  private pointerUp = (event: PointerEvent): void => {
    const gesture = this.gesture; this.gesture = undefined;
    if (this.element.hasPointerCapture(event.pointerId)) this.element.releasePointerCapture(event.pointerId);
    if (gesture?.kind === 'nodes' && gesture.moved) {
      const positions: Position[] = [];
      for (const origin of gesture.origins) { const p = this.nodes.get(origin.id); if (p) positions.push({ ...p }); }
      if (positions.length) this.callbacks.move(positions);
    } else if (gesture?.kind === 'box') {
      const bounds = { minX: Math.min(gesture.start.x, gesture.end.x), minY: Math.min(gesture.start.y, gesture.end.y), maxX: Math.max(gesture.start.x, gesture.end.x), maxY: Math.max(gesture.start.y, gesture.end.y) };
      const ids = new Set(gesture.original);
      for (const p of this.nodes.query(bounds)) if (this.isVisible(p.id)) ids.add(p.id);
      this.select(ids);
    } else if (gesture?.kind === 'pan' && !gesture.moved && gesture.clear) this.select(new Set());
    this.updateCursor(); this.invalidate();
  };
  private cancelGesture(): void {
    const gesture = this.gesture;
    if (gesture?.kind === 'nodes') for (const p of gesture.origins) this.movePoint(p.id, p.x, p.y);
    this.gesture = undefined; this.updateCursor(); this.invalidate();
  }
  private pointerCancel = (): void => this.cancelGesture();
  private lostPointerCapture = (): void => { if (this.gesture) this.cancelGesture(); };
  private wheel = (event: WheelEvent): void => {
    event.preventDefault();
    const before = this.world(event.clientX, event.clientY);
    const delta = event.deltaY * (event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? this.height : 1);
    this.camera.zoom = clamp(this.camera.zoom * Math.exp(-delta * 0.0015), 0.015, 8);
    const after = this.world(event.clientX, event.clientY);
    this.camera.x += before.x - after.x; this.camera.y += before.y - after.y; this.cameraChanged();
  };
  private doubleClick = (event: MouseEvent): void => {
    if (this.tool !== 'select') return;
    const point = this.world(event.clientX, event.clientY);
    if (!this.pickNode(point) && !this.pickEdge(point)) this.callbacks.create(point.x, point.y);
  };
  private contextMenu = (event: MouseEvent): void => {
    event.preventDefault();
    const point = this.world(event.clientX, event.clientY), node = this.pickNode(point), edge = node ? undefined : this.pickEdge(point);
    if (node && !this.selected.has(node.id)) this.select(new Set([node.id]));
    else if (edge) this.select(new Set(), edge);
    this.callbacks.context(event.clientX, event.clientY, node?.id, edge);
  };
  private keyDown = (event: KeyboardEvent): void => {
    if (event.code === 'Space') { event.preventDefault(); this.space = true; this.updateCursor(); }
    if (event.key === 'Escape') { this.cancelGesture(); this.linkSource = undefined; this.invalidate(); }
    if (event.key.startsWith('Arrow')) {
      event.preventDefault();
      const amount = 50 / this.camera.zoom;
      if (event.key === 'ArrowLeft') this.camera.x -= amount;
      if (event.key === 'ArrowRight') this.camera.x += amount;
      if (event.key === 'ArrowUp') this.camera.y -= amount;
      if (event.key === 'ArrowDown') this.camera.y += amount;
      this.cameraChanged();
    }
  };
  private keyUp = (event: KeyboardEvent): void => { if (event.code === 'Space') { this.space = false; this.updateCursor(); } };
  private updateCursor(): void { this.element.style.cursor = this.gesture?.kind === 'pan' ? 'grabbing' : this.space ? 'grab' : this.tool === 'link' ? 'crosshair' : 'default'; }

  private invalidate(refinement = false): void {
    if (!refinement) { this.refinementAllowance = 3; this.refinementDirection = undefined; }
    if (!this.frame && !this.disposed) this.frame = requestAnimationFrame(this.draw);
  }
  private draw = (): void => {
    this.frame = 0;
    if (this.disposed) return;
    const started = performance.now(); this.flushGeometry();
    const ctx = this.context, zoom = this.camera.zoom, ratio = this.element.width / this.width;
    ctx.setTransform(ratio, 0, 0, ratio, 0, 0); ctx.fillStyle = '#16191d'; ctx.fillRect(0, 0, this.width, this.height);
    ctx.setTransform(ratio * zoom, 0, 0, ratio * zoom, ratio * (this.width / 2 - this.camera.x * zoom), ratio * (this.height / 2 - this.camera.y * zoom));
    const view = this.viewport(20 / zoom);
    ctx.font = `${11 / zoom}px system-ui, sans-serif`; ctx.textBaseline = 'bottom';
    for (const [id, bounds] of this.collectionBounds) {
      if (!intersects(view, bounds)) continue;
      const collection = this.collections.get(id); if (!collection) continue;
      const color = /^#[\da-f]{6}$/i.test(collection.color) ? collection.color : '#687986';
      ctx.fillStyle = `${color}12`; ctx.fillRect(bounds.minX, bounds.minY, bounds.maxX - bounds.minX, bounds.maxY - bounds.minY);
      ctx.strokeStyle = `${color}70`; ctx.lineWidth = 1 / zoom; ctx.strokeRect(bounds.minX, bounds.minY, bounds.maxX - bounds.minX, bounds.maxY - bounds.minY);
      if (zoom > 0.4) { ctx.fillStyle = color; ctx.fillText(collection.label.slice(0, 70), bounds.minX + 6 / zoom, bounds.minY - 3 / zoom); }
    }
    // Edges have an explicit LOD budget. Picking always queries the full index.
    const zoomEdgeBudget = zoom < 0.2 ? 1_500 : zoom < 0.5 ? 3_000 : 7_000;
    const edgeBudget = Math.min(zoomEdgeBudget, this.edgeLimit);
    const visibleEdges = this.edgeIndex.sample(view, edgeBudget);
    let drawnEdges = 0;
    // Bound path complexity: a single path containing thousands of crossing edges/arrowheads
    // is slower in tested Windows Canvas2D backends than modest geometry batches.
    ctx.beginPath(); ctx.strokeStyle = '#46535d'; ctx.lineWidth = 0.8 / zoom;
    for (const id of visibleEdges) {
      if (id === this.selectedEdge) continue;
      const edge = this.relationships.get(id), curve = this.curves.get(id);
      if (!edge || !curve || !this.edgeVisible(edge)) continue;
      this.traceCurve(curve); drawnEdges++;
      if (drawnEdges % 64 === 0) { ctx.stroke(); ctx.beginPath(); }
    }
    ctx.stroke();
    if (zoom > 0.55) {
      ctx.beginPath(); ctx.fillStyle = '#46535d'; let arrows = 0;
      for (const id of visibleEdges) {
        if (id === this.selectedEdge) continue;
        const edge = this.relationships.get(id), curve = this.curves.get(id);
        if (edge?.directed && curve && this.edgeVisible(edge)) {
          this.traceArrow(curve); arrows++;
          if (arrows % 64 === 0) { ctx.fill(); ctx.beginPath(); }
        }
      }
      ctx.fill();
    }
    if (this.selectedEdge) {
      const edge = this.relationships.get(this.selectedEdge), curve = this.curves.get(this.selectedEdge);
      if (edge && curve && this.edgeVisible(edge) && intersects(view, curveBounds(curve))) { this.drawEdge(edge, curve, true); drawnEdges++; }
    }
    const candidates = this.nodes.query(view);
    const points = this.included ? candidates.filter(point => this.isVisible(point.id)) : candidates;
    // Uniform sampling across the whole visible set, never a prefix. Selected nodes are always drawn.
    const nodeBudget = this.nodeLimit, stride = Math.max(1, Math.ceil(points.length / nodeBudget));
    const labelBudget = Math.max(40, Math.round(900 * Math.sqrt(nodeBudget / 20_000)));
    let drawnNodes = 0, labels = 0;
    const labelCells = new Set<string>();
    const radius = clamp(6 * zoom, 2, 9) / zoom;
    ctx.beginPath(); ctx.fillStyle = '#7f929e';
    for (let i = 0; i < points.length; i += stride) {
      const point = points[i]!;
      if (this.selected.has(point.id) || this.linkSource === point.id) continue;
      ctx.moveTo(point.x + radius, point.y); ctx.arc(point.x, point.y, radius, 0, Math.PI * 2); drawnNodes++;
      if (drawnNodes % 128 === 0) { ctx.fill(); ctx.beginPath(); }
    }
    ctx.fill();
    for (let i = 0; i < points.length; i++) {
      const point = points[i]!, selected = this.selected.has(point.id), linking = this.linkSource === point.id;
      if (i % stride !== 0 && !selected && !linking) continue;
      if (selected || linking) {
        ctx.beginPath(); ctx.arc(point.x, point.y, radius + 2 / zoom, 0, Math.PI * 2); ctx.fillStyle = '#8ac9d2'; ctx.fill();
        ctx.strokeStyle = '#d4f0f3'; ctx.lineWidth = 1.2 / zoom; ctx.stroke(); drawnNodes++;
      }
      if ((zoom >= 0.65 && labels < labelBudget) || selected || linking) {
        const entity = this.entities.get(point.id);
        if (entity) {
          ctx.font = `${12 / zoom}px system-ui, sans-serif`; ctx.textBaseline = 'middle';
          const label = entity.label.length > 48 ? `${entity.label.slice(0, 47)}…` : entity.label;
          const labelX = point.x + radius + 5 / zoom;
          const screenX = (labelX - this.camera.x) * zoom + this.width / 2;
          const screenY = (point.y - this.camera.y) * zoom + this.height / 2;
          const labelWidth = ctx.measureText(label).width * zoom + 4;
          const minCellX = Math.floor(screenX / 24), maxCellX = Math.floor((screenX + labelWidth) / 24);
          const minCellY = Math.floor((screenY - 9) / 18), maxCellY = Math.floor((screenY + 9) / 18);
          let occupied = false;
          for (let x = minCellX; x <= maxCellX; x++) for (let y = minCellY; y <= maxCellY; y++) {
            if (labelCells.has(`${x},${y}`)) occupied = true;
          }
          // Dense graphs must remain readable; selecting a small set reveals its labels directly.
          if (occupied && !linking && !(selected && this.selected.size <= 20)) continue;
          for (let x = minCellX; x <= maxCellX; x++) for (let y = minCellY; y <= maxCellY; y++) labelCells.add(`${x},${y}`);
          ctx.lineWidth = 3 / zoom; ctx.strokeStyle = '#16191d'; ctx.strokeText(label, labelX, point.y);
          ctx.fillStyle = selected ? '#edf3f5' : '#bcc5cc'; ctx.fillText(label, labelX, point.y); labels++;
        }
      }
    }
    if (this.gesture?.kind === 'box') {
      const { start, end } = this.gesture;
      ctx.fillStyle = '#80bec21c'; ctx.strokeStyle = '#8ac9d2'; ctx.lineWidth = 1 / zoom;
      ctx.fillRect(start.x, start.y, end.x - start.x, end.y - start.y); ctx.strokeRect(start.x, start.y, end.x - start.x, end.y - start.y);
    }
    const elapsed = performance.now() - started;
    this.stats = { visibleNodes: points.length, drawnNodes, drawnEdges, drawnLabels: labels, nodeBudget, edgeBudget, frameMs: elapsed };
    this.adaptDetail(elapsed, zoomEdgeBudget, edgeBudget, visibleEdges.length, points.length);
  };
  private drawEdge(edge: Relationship, curve: Curve, selected: boolean): void {
    const ctx = this.context, zoom = this.camera.zoom;
    ctx.beginPath(); this.traceCurve(curve);
    ctx.strokeStyle = selected ? '#9ed7df' : '#46535d'; ctx.lineWidth = (selected ? 2 : 0.8) / zoom; ctx.stroke();
    if (edge.directed && (zoom > 0.55 || selected)) {
      ctx.beginPath(); this.traceArrow(curve); ctx.fillStyle = ctx.strokeStyle; ctx.fill();
    }
    if (selected && (edge.label || edge.kind)) {
      const middle = at(curve, 0.5); ctx.font = `${11 / zoom}px system-ui, sans-serif`; ctx.textBaseline = 'bottom';
      ctx.strokeStyle = '#16191d'; ctx.lineWidth = 3 / zoom; const label = (edge.label || edge.kind).slice(0, 70);
      ctx.strokeText(label, middle.x, middle.y - 5 / zoom); ctx.fillStyle = '#dce8ed'; ctx.fillText(label, middle.x, middle.y - 5 / zoom);
    }
  }
  private traceCurve(curve: Curve): void {
    const ctx = this.context;
    ctx.moveTo(curve.x0, curve.y0);
    if (curve.kind === 0) ctx.lineTo(curve.x1, curve.y1);
    else if (curve.kind === 1) ctx.quadraticCurveTo(curve.cx, curve.cy, curve.x1, curve.y1);
    else ctx.bezierCurveTo(curve.cx, curve.cy, curve.dx, curve.dy, curve.x1, curve.y1);
  }
  private traceArrow(curve: Curve): void {
    const ctx = this.context, zoom = this.camera.zoom;
    const lastControlX = curve.kind === 2 ? curve.dx : curve.kind === 1 ? curve.cx : curve.x0;
    const lastControlY = curve.kind === 2 ? curve.dy : curve.kind === 1 ? curve.cy : curve.y0;
    const t = 1 - Math.min(0.28, 10 / zoom / Math.max(1, Math.hypot(curve.x1 - lastControlX, curve.y1 - lastControlY) * (curve.kind + 1)));
    const tip = at(curve, t), behind = at(curve, Math.max(0, t - 0.015)), angle = Math.atan2(tip.y - behind.y, tip.x - behind.x), size = 6 / zoom;
    ctx.moveTo(tip.x, tip.y); ctx.lineTo(tip.x - Math.cos(angle - 0.45) * size, tip.y - Math.sin(angle - 0.45) * size);
    ctx.lineTo(tip.x - Math.cos(angle + 0.45) * size, tip.y - Math.sin(angle + 0.45) * size); ctx.closePath();
  }

  private adaptDetail(elapsed: number, zoomEdgeBudget: number, edgeBudget: number, sampledEdges: number, visibleNodes: number): void {
    const target = 25;
    let nextNodes = this.nodeLimit, nextEdges = this.edgeLimit;
    if (elapsed > target * 1.3) {
      const factor = clamp(target / elapsed, 0.2, 0.8);
      nextNodes = Math.max(1_000, Math.floor(this.nodeLimit * factor));
      nextEdges = Math.max(250, Math.floor(edgeBudget * factor));
      this.refinementDirection = 'down';
    } else if (elapsed < target * 0.45 && this.refinementDirection !== 'down') {
      // Recover detail slowly when a new view is cheap. Hysteresis and a finite
      // refinement allowance prevent a stationary canvas from continuously redrawing.
      if (visibleNodes > this.nodeLimit) nextNodes = Math.min(20_000, Math.ceil(this.nodeLimit * 1.2));
      if (this.edgeLimit < zoomEdgeBudget && sampledEdges >= edgeBudget * 0.8) nextEdges = Math.min(7_000, Math.ceil(this.edgeLimit * 1.2));
      if (nextNodes !== this.nodeLimit || nextEdges !== this.edgeLimit) this.refinementDirection = 'up';
    }
    if (nextNodes === this.nodeLimit && nextEdges === this.edgeLimit) return;
    this.nodeLimit = nextNodes; this.edgeLimit = nextEdges;
    if (this.refinementAllowance > 0) { this.refinementAllowance--; this.invalidate(true); }
  }

  getRenderStats(): Readonly<typeof this.stats & { pendingFrame: boolean }> { return { ...this.stats, pendingFrame: this.frame !== 0 }; }

  layout(): Promise<Position[]> {
    this.cancelLayout();
    const ids = [...this.entities.keys()], positions = new Float64Array(ids.length * 2), indexes = new Map<string, number>();
    ids.forEach((id, i) => { indexes.set(id, i); const point = this.nodes.get(id)!; positions[i * 2] = point.x; positions[i * 2 + 1] = point.y; });
    const endpoints: number[] = [];
    for (const edge of this.relationships.values()) {
      const source = indexes.get(edge.source), target = indexes.get(edge.target);
      if (source !== undefined && target !== undefined) endpoints.push(source, target);
    }
    const edges = new Uint32Array(endpoints);
    return new Promise((resolve, reject) => {
      try {
        const worker = new Worker(new URL('./layout.worker.ts', import.meta.url), { type: 'module' });
        this.worker = worker; this.rejectLayout = reject;
        const cleanup = (): void => { worker.terminate(); if (this.worker === worker) { this.worker = undefined; this.rejectLayout = undefined; } };
        worker.onmessage = (event: MessageEvent<{ positions?: Float64Array; error?: string }>): void => {
          cleanup();
          if (event.data.error) { reject(new Error(event.data.error)); return; }
          const result = event.data.positions;
          if (!result || result.length !== ids.length * 2 || result.some(value => !Number.isFinite(value))) { reject(new Error('Layout returned invalid coordinates')); return; }
          resolve(ids.map((id, i) => ({ id, x: result[i * 2]!, y: result[i * 2 + 1]! })));
        };
        worker.onerror = (event): void => { cleanup(); reject(new Error(event.message || 'Layout worker failed')); };
        worker.postMessage({ positions, edges, iterations: ids.length > 10_000 ? 60 : 120 }, [positions.buffer, edges.buffer]);
      } catch (error) { this.worker = undefined; this.rejectLayout = undefined; reject(error); }
    });
  }
  cancelLayout(): void {
    this.worker?.terminate(); this.worker = undefined;
    this.rejectLayout?.(new DOMException('Layout cancelled', 'AbortError')); this.rejectLayout = undefined;
  }
  destroy(): void {
    this.disposed = true; this.cancelLayout(); this.listeners.abort(); this.resizeObserver.disconnect();
    if (this.frame) cancelAnimationFrame(this.frame);
    this.element.remove();
  }
}
