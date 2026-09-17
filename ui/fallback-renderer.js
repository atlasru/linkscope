// Minimal read-only renderer used when WebGL2 or shader initialization fails.
// Keeps entity management, search, inspector, map and export accessible.
export class FallbackRenderer {
  constructor(canvas, reason) {
    this.canvas = canvas;
    this.reason = reason;
    this.disabled = true;
    this.gl = null;
    this.model = null;
    this.count = 0;
    this.edgeCount = 0;
    this.camera = [0, 0];
    this.zoom = 1;
    this.timeRange = [-1e20, 1e20];
    this.layoutSteps = 0;
    this.dirty = false;
    this.continuous = false;
    this.resize();
  }
  resize() {
    const bounds = this.canvas.getBoundingClientRect();
    this.width = Math.max(1, bounds.width);
    this.height = Math.max(1, bounds.height);
  }
  setModel(model) {
    this.model = model;
    this.count = model.nodes.length;
    this.edgeCount = model.links.length;
    this.resize();
  }
  select() {}
  readPositions() {}
  startLayout() { throw new Error('GPU layout requires WebGL2.'); }
  filter(from, to) {
    this.timeRange = [Number.isFinite(from) ? from : -1e20, Number.isFinite(to) ? to : 1e20];
    this.edgeCount = (this.model?.edges || []).reduce((count, edge) => {
      const a = this.model.nodes[edge.a].timestamp;
      const b = this.model.nodes[edge.b].timestamp;
      return count + Number(a >= this.timeRange[0] && a <= this.timeRange[1] && b >= this.timeRange[0] && b <= this.timeRange[1]) * 2;
    }, 0);
  }
  fit() {
    if (!this.count) return;
    this.resize();
    let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
    for (let i = 0; i < this.model.positions.length; i += 2) {
      minX = Math.min(minX, this.model.positions[i]);
      maxX = Math.max(maxX, this.model.positions[i]);
      minY = Math.min(minY, this.model.positions[i + 1]);
      maxY = Math.max(maxY, this.model.positions[i + 1]);
    }
    this.camera = [(minX + maxX) / 2, (minY + maxY) / 2];
    this.zoom = Math.max(0.03, Math.min(this.width / (maxX - minX + 130), this.height / (maxY - minY + 130)));
  }
  screen(i) {
    return [
      (this.model.positions[i * 2] - this.camera[0]) * this.zoom + this.width / 2,
      (this.model.positions[i * 2 + 1] - this.camera[1]) * this.zoom + this.height / 2
    ];
  }
  pick(x, y) {
    let best = -1, distance = 144;
    for (let i = 0; i < this.count; i++) {
      const timestamp = this.model.nodes[i].timestamp;
      if (timestamp < this.timeRange[0] || timestamp > this.timeRange[1]) continue;
      const [sx, sy] = this.screen(i);
      const d = (sx - x) ** 2 + (sy - y) ** 2;
      if (d < distance) { distance = d; best = i; }
    }
    return best;
  }
}
