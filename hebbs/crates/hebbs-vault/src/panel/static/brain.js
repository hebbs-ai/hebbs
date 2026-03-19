// HEBBS Memory Palace - Bioluminescent Brain Renderer (Canvas 2D)
//
// A living, breathing visualization of the knowledge graph.
// Nodes are glowing orbs with radial gradients. Edges are organic
// bezier tendrils. Ambient drift via inline 2D simplex noise keeps
// the scene alive even when idle.
//
// Public API mirrors MemoryGraph for drop-in tab switching.

// ═══════════════════════════════════════════════════════════════════════
//  Inline 2D Simplex Noise (no dependencies)
// ═══════════════════════════════════════════════════════════════════════

const F2 = 0.5 * (Math.sqrt(3) - 1);
const G2 = (3 - Math.sqrt(3)) / 6;
const _grad = [[1,1],[-1,1],[1,-1],[-1,-1],[1,0],[-1,0],[0,1],[0,-1]];
const _perm = new Uint8Array(512);
(function initPerm() {
  const p = new Uint8Array(256);
  for (let i = 0; i < 256; i++) p[i] = i;
  for (let i = 255; i > 0; i--) {
    const j = (i * 16807 + 1) & 0xff;
    const tmp = p[i]; p[i] = p[j]; p[j] = tmp;
  }
  for (let i = 0; i < 512; i++) _perm[i] = p[i & 255];
})();

function simplex2(x, y) {
  const s = (x + y) * F2;
  const i = Math.floor(x + s), j = Math.floor(y + s);
  const t = (i + j) * G2;
  const x0 = x - (i - t), y0 = y - (j - t);
  const i1 = x0 > y0 ? 1 : 0, j1 = x0 > y0 ? 0 : 1;
  const x1 = x0 - i1 + G2, y1 = y0 - j1 + G2;
  const x2 = x0 - 1 + 2 * G2, y2 = y0 - 1 + 2 * G2;
  const ii = i & 255, jj = j & 255;

  let n0 = 0, n1 = 0, n2 = 0;
  let t0 = 0.5 - x0*x0 - y0*y0;
  if (t0 > 0) { t0 *= t0; const g = _grad[_perm[ii + _perm[jj]] & 7]; n0 = t0*t0*(g[0]*x0+g[1]*y0); }
  let t1 = 0.5 - x1*x1 - y1*y1;
  if (t1 > 0) { t1 *= t1; const g = _grad[_perm[ii+i1 + _perm[jj+j1]] & 7]; n1 = t1*t1*(g[0]*x1+g[1]*y1); }
  let t2 = 0.5 - x2*x2 - y2*y2;
  if (t2 > 0) { t2 *= t2; const g = _grad[_perm[ii+1 + _perm[jj+1]] & 7]; n2 = t2*t2*(g[0]*x2+g[1]*y2); }
  return 70 * (n0 + n1 + n2);
}

// ═══════════════════════════════════════════════════════════════════════
//  MemoryBrain
// ═══════════════════════════════════════════════════════════════════════

export class MemoryBrain {
  constructor(canvas) {
    this.canvas = canvas;
    this.ctx = canvas.getContext('2d');
    this.nodes = [];
    this.edges = [];
    this.nodeMap = new Map();

    // View transform
    this.offsetX = 0;
    this.offsetY = 0;
    this.scale = 1;

    // Interaction
    this.hoveredNode = null;
    this.selectedNode = null;
    this.dragNode = null;
    this.isDragging = false;
    this.isPanning = false;
    this.lastMouse = { x: 0, y: 0 };

    // Callbacks
    this.onNodeClick = null;
    this.onNodeHover = null;

    // State
    this.decayMode = false;
    this.visibleNodeIds = null;
    this.running = false;
    this._rafId = null;
    this._time = 0;

    // Spreading activation
    this._activation = null;       // Map<id, {dist, score}>
    this._activationProgress = 1;
    this._activationTarget = 1;
    this._activationStart = 0;
    this._clearing = false;

    // Physics (for non-UMAP mode)
    this.hasProjection = false;
    this.alpha = 1.0;
    this.alphaDecay = 0.005;
    this.alphaMin = 0.001;

    // Noise time offset per node (seeded on setData)
    this._noiseSeeds = new Map();

    this._setupEvents();
    this._resize();
    window.addEventListener('resize', () => this._resize());
  }

  // ── Data ──────────────────────────────────────────────────────────────

  setData(nodes, edges, hasProjection, nClusters, clusterLabels) {
    this.nodeMap.clear();
    this._noiseSeeds.clear();
    this.hasProjection = !!hasProjection;

    const phi = (1 + Math.sqrt(5)) / 2;
    nodes.forEach((n, i) => {
      if (n.x == null || n.y == null) {
        const theta = 2 * Math.PI * i / phi;
        const r = Math.sqrt(i + 1) * 30;
        n.x = Math.cos(theta) * r;
        n.y = Math.sin(theta) * r;
      }
      n.vx = 0;
      n.vy = 0;
      if (n.pinned) { n.fx = n.x; n.fy = n.y; }
      else { n.fx = null; n.fy = null; }
      this.nodeMap.set(n.id, n);
      this._noiseSeeds.set(n.id, Math.random() * 1000);
    });

    this.nodes = nodes;
    this.edges = edges.map(e => ({
      ...e,
      sourceNode: this.nodeMap.get(e.source),
      targetNode: this.nodeMap.get(e.target),
      phase: Math.random() * Math.PI * 2,
      curvature: 8 + Math.random() * 12,
    })).filter(e => e.sourceNode && e.targetNode);

    if (this.hasProjection) {
      this.alpha = 0.3;
      this.alphaDecay = 0.02;
    } else {
      this.alpha = 1.0;
      this.alphaDecay = 0.005;
    }

    this._centerView();
  }

  mergeData(nodes, edges, hasProjection, nClusters, clusterLabels) {
    this.hasProjection = !!hasProjection;

    const oldPositions = new Map();
    for (const n of this.nodes) {
      oldPositions.set(n.id, { x: n.x, y: n.y, vx: n.vx, vy: n.vy, fx: n.fx, fy: n.fy });
    }

    this.nodeMap.clear();
    const phi = (1 + Math.sqrt(5)) / 2;
    let newCount = 0;

    nodes.forEach((n, i) => {
      const old = oldPositions.get(n.id);
      if (old) {
        n.x = old.x; n.y = old.y;
        n.vx = old.vx; n.vy = old.vy;
        n.fx = old.fx; n.fy = old.fy;
      } else if (n.x != null && n.y != null) {
        n.vx = 0; n.vy = 0; n.fx = null; n.fy = null;
        newCount++;
      } else {
        const theta = 2 * Math.PI * (this.nodes.length + newCount) / phi;
        const r = Math.sqrt(this.nodes.length + newCount + 1) * 30;
        n.x = Math.cos(theta) * r;
        n.y = Math.sin(theta) * r;
        n.vx = 0; n.vy = 0; n.fx = null; n.fy = null;
        newCount++;
      }
      this.nodeMap.set(n.id, n);
      if (!this._noiseSeeds.has(n.id)) {
        this._noiseSeeds.set(n.id, Math.random() * 1000);
      }
    });

    this.nodes = nodes;
    this.edges = edges.map(e => ({
      ...e,
      sourceNode: this.nodeMap.get(e.source),
      targetNode: this.nodeMap.get(e.target),
      phase: Math.random() * Math.PI * 2,
      curvature: 8 + Math.random() * 12,
    })).filter(e => e.sourceNode && e.targetNode);

    if (newCount > 0) {
      this.alpha = Math.max(this.alpha, 0.3);
    }
  }

  // ── Lifecycle ─────────────────────────────────────────────────────────

  start() {
    if (this.running) return;
    this.running = true;
    this._time = performance.now();
    this._tick();
  }

  stop() {
    this.running = false;
    if (this._rafId) {
      cancelAnimationFrame(this._rafId);
      this._rafId = null;
    }
  }

  // ── Public methods ────────────────────────────────────────────────────

  selectNode(id) {
    this.selectedNode = id ? this.nodeMap.get(id) : null;
  }

  setSearchResults(results) {
    if (!results || results.length === 0) {
      if (this._activation) {
        // Fade out
        this._clearing = true;
        this._activationStart = performance.now();
        this._activationTarget = 0;
        this._activationProgress = 1;
      } else {
        this._activation = null;
      }
      return;
    }

    // Build BFS distance map from matched nodes
    const matchedIds = new Set(results.map(r => r.memory_id));
    const scoreMap = new Map(results.map(r => [r.memory_id, r.score]));
    const dist = new Map();

    // BFS bounded at 3 hops
    const adj = new Map();
    for (const n of this.nodes) adj.set(n.id, []);
    for (const e of this.edges) {
      adj.get(e.source)?.push(e.target);
      adj.get(e.target)?.push(e.source);
    }

    const queue = [];
    for (const id of matchedIds) {
      dist.set(id, 0);
      queue.push(id);
    }
    let qi = 0;
    while (qi < queue.length) {
      const cur = queue[qi++];
      const d = dist.get(cur);
      if (d >= 3) continue;
      for (const nb of (adj.get(cur) || [])) {
        if (!dist.has(nb)) {
          dist.set(nb, d + 1);
          queue.push(nb);
        }
      }
    }

    this._activation = new Map();
    for (const [id, d] of dist) {
      this._activation.set(id, { dist: d, score: scoreMap.get(id) || 0 });
    }

    this._clearing = false;
    this._activationStart = performance.now();
    this._activationProgress = 0;
    this._activationTarget = 1;
  }

  setDecayMode(enabled) {
    this.decayMode = !!enabled;
  }

  setVisibleNodes(nodeIds) {
    this.visibleNodeIds = nodeIds;
  }

  exportPNG() {
    const exportCanvas = document.createElement('canvas');
    const w = 2400, h = 1260;
    exportCanvas.width = w;
    exportCanvas.height = h;
    const ctx = exportCanvas.getContext('2d');

    ctx.fillStyle = '#050510';
    ctx.fillRect(0, 0, w, h);

    const dpr = window.devicePixelRatio || 1;
    const srcW = this.canvas.width / dpr;
    const srcH = this.canvas.height / dpr;
    const s = Math.min(w / srcW, h / srcH);

    ctx.save();
    ctx.translate(w / 2, h / 2);
    ctx.scale(this.scale * s, this.scale * s);
    ctx.translate(this.offsetX, this.offsetY);

    const now = performance.now();
    this._drawEdges(ctx, now, s);
    this._drawNodes(ctx, now, s);

    ctx.restore();

    ctx.font = 'bold 28px -apple-system, system-ui, sans-serif';
    ctx.fillStyle = '#4ECDC4';
    ctx.fillText('HEBBS Memory Palace', 40, 50);
    ctx.font = '18px -apple-system, system-ui, sans-serif';
    ctx.fillStyle = '#9CA3AF';
    ctx.fillText(`${this.nodes.length} memories`, 40, 80);

    exportCanvas.toBlob((blob) => {
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = 'hebbs-brain.png';
      a.click();
      URL.revokeObjectURL(url);
    }, 'image/png');
  }

  // ── Coordinate transforms ────────────────────────────────────────────

  _screenToWorld(sx, sy) {
    const dpr = window.devicePixelRatio || 1;
    const cx = this.canvas.width / (2 * dpr);
    const cy = this.canvas.height / (2 * dpr);
    return {
      x: (sx - cx) / this.scale - this.offsetX,
      y: (sy - cy) / this.scale - this.offsetY,
    };
  }

  _worldToScreen(wx, wy) {
    const dpr = window.devicePixelRatio || 1;
    const cx = this.canvas.width / (2 * dpr);
    const cy = this.canvas.height / (2 * dpr);
    return {
      x: (wx + this.offsetX) * this.scale + cx,
      y: (wy + this.offsetY) * this.scale + cy,
    };
  }

  _resize() {
    const dpr = window.devicePixelRatio || 1;
    const rect = this.canvas.getBoundingClientRect();
    this.canvas.width = rect.width * dpr;
    this.canvas.height = rect.height * dpr;
    this.ctx.scale(dpr, dpr);
  }

  _centerView() {
    if (this.nodes.length === 0) return;
    let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
    for (const n of this.nodes) {
      minX = Math.min(minX, n.x);
      minY = Math.min(minY, n.y);
      maxX = Math.max(maxX, n.x);
      maxY = Math.max(maxY, n.y);
    }
    this.offsetX = -(minX + maxX) / 2;
    this.offsetY = -(minY + maxY) / 2;

    const dpr = window.devicePixelRatio || 1;
    const canvasW = this.canvas.width / dpr;
    const canvasH = this.canvas.height / dpr;
    const graphW = maxX - minX + 100;
    const graphH = maxY - minY + 100;
    this.scale = Math.min(canvasW / graphW, canvasH / graphH, 2);
  }

  _findNodeAt(sx, sy) {
    const world = this._screenToWorld(sx, sy);
    let closest = null;
    let minDist = Infinity;
    for (const node of this.nodes) {
      const radius = this._nodeRadius(node) + 6;
      const dx = world.x - node.x;
      const dy = world.y - node.y;
      const dist = Math.sqrt(dx * dx + dy * dy);
      if (dist < radius && dist < minDist) {
        closest = node;
        minDist = dist;
      }
    }
    return closest;
  }

  // ── Node sizing ───────────────────────────────────────────────────────

  _nodeRadius(node) {
    if (node.kind === 'document' || node.kind === 'episode') {
      return 8 + (node.importance || 0) * 14;
    }
    return 3 + (node.importance || 0) * 8;
  }

  _nodeBrightness(node, now) {
    const recency = node.recency || 0;
    const access = Math.min((node.access_count || 0) / 10, 1);
    const decay = this.decayMode ? (node.decay_score || 1) : 1;
    const base = 0.2 + 0.8 * (recency * 0.5 + access * 0.3 + decay * 0.2);

    // Global breathing: 4s cycle, +/-10%
    const globalBreath = Math.sin(now / 4000) * 0.10;
    // Per-node breathing: desynchronized
    const seed = this._noiseSeeds.get(node.id) || 0;
    const imp = node.importance || 0.5;
    const nodeBreath = Math.sin(now / (1500 + imp * 1000) + seed) * 0.05;

    return Math.max(0.08, Math.min(1.0, base + globalBreath + nodeBreath));
  }

  // ── Activation glow multiplier ────────────────────────────────────────

  _activationGlow(node, now) {
    if (!this._activation) return 1;

    const elapsed = now - this._activationStart;
    let progress;

    if (this._clearing) {
      // Fading out over 500ms
      progress = Math.min(elapsed / 500, 1);
      if (progress >= 1) {
        this._activation = null;
        this._clearing = false;
        return 1;
      }
      // Invert: go from highlighted back to normal
      const fadeOut = 1 - progress;
      const info = this._activation.get(node.id);
      if (!info) return 0.03 + 0.97 * (1 - fadeOut);
      if (info.dist === 0) return 1;
      if (info.dist === 1) return 0.3 * fadeOut + (1 - fadeOut);
      if (info.dist === 2) return 0.1 * fadeOut + (1 - fadeOut);
      return 0.03 + 0.97 * (1 - fadeOut);
    }

    // Activating over 1000ms
    progress = Math.min(elapsed / 1000, 1);
    const info = this._activation.get(node.id);

    if (!info) {
      // Not in activation neighborhood: dim
      return 1 - progress * 0.97; // dims to 0.03
    }

    if (info.dist === 0) {
      // Matched node: full glow with midpoint pulse
      const pulse = Math.sin(progress * Math.PI) * 0.3;
      return Math.min(1, (info.score || 0.8) + pulse);
    }
    if (info.dist === 1) return 0.3 * progress + (1 - progress);
    if (info.dist === 2) return 0.1 * progress + (1 - progress);
    return 0.05 * progress + (1 - progress);
  }

  // ── Rendering ─────────────────────────────────────────────────────────

  _tick() {
    if (!this.running) return;
    const now = performance.now();

    // Physics step (light force-directed when no UMAP)
    if (!this.hasProjection && this.alpha > this.alphaMin) {
      this._simulateForces();
    }

    // Ambient drift via simplex noise
    this._applyDrift(now);

    // Draw
    this._draw(now);

    this._rafId = requestAnimationFrame(() => this._tick());
  }

  _simulateForces() {
    const nodes = this.nodes;
    const n = nodes.length;

    // Repulsion
    for (let i = 0; i < n; i++) {
      for (let j = i + 1; j < n; j++) {
        const a = nodes[i], b = nodes[j];
        let dx = b.x - a.x, dy = b.y - a.y;
        let d2 = dx * dx + dy * dy;
        if (d2 < 1) d2 = 1;
        const force = 1000 * this.alpha / d2;
        const fx = dx / Math.sqrt(d2) * force;
        const fy = dy / Math.sqrt(d2) * force;
        a.vx -= fx; a.vy -= fy;
        b.vx += fx; b.vy += fy;
      }
    }

    // Spring attraction along edges
    for (const edge of this.edges) {
      const a = edge.sourceNode, b = edge.targetNode;
      if (!a || !b) continue;
      const dx = b.x - a.x, dy = b.y - a.y;
      const d = Math.sqrt(dx * dx + dy * dy) || 1;
      const targetLen = 140;
      const force = (d - targetLen) * 0.01 * this.alpha;
      const fx = dx / d * force;
      const fy = dy / d * force;
      a.vx += fx; a.vy += fy;
      b.vx -= fx; b.vy -= fy;
    }

    // Damping and position update
    for (const node of nodes) {
      if (node.fx != null) { node.x = node.fx; node.y = node.fy; node.vx = 0; node.vy = 0; continue; }
      node.vx *= 0.92;
      node.vy *= 0.92;
      node.x += node.vx;
      node.y += node.vy;
    }

    this.alpha = Math.max(this.alpha - this.alphaDecay, this.alphaMin);
  }

  _applyDrift(now) {
    const t = now * 0.0003;
    for (const node of this.nodes) {
      if (node.fx != null) continue;
      const seed = this._noiseSeeds.get(node.id) || 0;
      const dx = simplex2(node.x * 0.005 + seed, t) * 0.15;
      const dy = simplex2(node.y * 0.005 + seed + 100, t + 50) * 0.15;
      node.x += dx;
      node.y += dy;
    }
  }

  _draw(now) {
    const ctx = this.ctx;
    const dpr = window.devicePixelRatio || 1;
    const w = this.canvas.width / dpr;
    const h = this.canvas.height / dpr;

    // Clear with void background
    ctx.fillStyle = '#050510';
    ctx.fillRect(0, 0, w, h);

    ctx.save();
    ctx.translate(w / 2, h / 2);
    ctx.scale(this.scale, this.scale);
    ctx.translate(this.offsetX, this.offsetY);

    this._drawEdges(ctx, now, 1);
    this._drawNodes(ctx, now, 1);

    ctx.restore();

    // Tooltip
    if (this.hoveredNode && !this.isDragging) {
      this._drawTooltip(ctx, this.hoveredNode);
    }
  }

  _drawEdges(ctx, now, exportScale) {
    for (const edge of this.edges) {
      const a = edge.sourceNode, b = edge.targetNode;
      if (!a || !b) continue;

      // Visibility check
      if (this.visibleNodeIds && !this.visibleNodeIds.has(a.id) && !this.visibleNodeIds.has(b.id)) continue;

      const mx = (a.x + b.x) / 2;
      const my = (a.y + b.y) / 2;

      // Wavering control point
      const dx = b.x - a.x, dy = b.y - a.y;
      const len = Math.sqrt(dx * dx + dy * dy) || 1;
      const nx = -dy / len, ny = dx / len;
      const waver = Math.sin(now * 0.001 + edge.phase) * edge.curvature;
      const cpx = mx + nx * waver;
      const cpy = my + ny * waver;

      // Activation dimming for edges
      let edgeAlpha = 1;
      if (this._activation) {
        const ga = this._activationGlow(a, now);
        const gb = this._activationGlow(b, now);
        edgeAlpha = Math.max(ga, gb);
      }

      // Edge type styling
      let color, widthWide, widthNarrow;
      if (edge.type === 'proposition_of') {
        color = [100, 200, 180];
        widthWide = 3;
        widthNarrow = 1;
      } else if (edge.type === 'contradicts') {
        color = [200, 80, 80];
        widthWide = 3;
        widthNarrow = 1;
      } else {
        // similarity
        color = [60, 120, 160];
        widthWide = 2;
        widthNarrow = 0.5;
      }

      const baseAlpha = edge.type === 'similarity'
        ? 0.06 + (edge.weight || 0) * 0.12
        : 0.15 + (edge.weight || 0) * 0.25;

      // Two-pass glow: wide faint + narrow bright
      ctx.beginPath();
      ctx.moveTo(a.x, a.y);
      ctx.quadraticCurveTo(cpx, cpy, b.x, b.y);
      ctx.strokeStyle = `rgba(${color[0]},${color[1]},${color[2]},${baseAlpha * 0.4 * edgeAlpha})`;
      ctx.lineWidth = widthWide / exportScale;
      ctx.stroke();

      ctx.beginPath();
      ctx.moveTo(a.x, a.y);
      ctx.quadraticCurveTo(cpx, cpy, b.x, b.y);
      ctx.strokeStyle = `rgba(${color[0]},${color[1]},${color[2]},${baseAlpha * edgeAlpha})`;
      ctx.lineWidth = widthNarrow / exportScale;
      ctx.stroke();
    }
  }

  _drawNodes(ctx, now, exportScale) {
    for (const node of this.nodes) {
      if (this.visibleNodeIds && !this.visibleNodeIds.has(node.id)) continue;

      const radius = this._nodeRadius(node);
      const brightness = this._nodeBrightness(node, now);
      const activationGlow = this._activationGlow(node, now);
      const glow = brightness * activationGlow;

      // Colors based on node type
      let coreR, coreG, coreB, haloR, haloG, haloB;
      if (node.kind === 'document' || node.kind === 'episode') {
        // Warm amber
        coreR = 245; coreG = 166; coreB = 35;
        haloR = 200; haloG = 130; haloB = 20;
      } else {
        // Cool blue-green
        coreR = 78; coreG = 205; coreB = 196;
        haloR = 50; haloG = 160; haloB = 150;
      }

      // Hover brightening
      let hoverMul = 1;
      if (this.hoveredNode === node) {
        hoverMul = 1.5;
      } else if (this.hoveredNode && this._isNeighbor(this.hoveredNode, node)) {
        hoverMul = 1.2;
      }

      const finalGlow = Math.min(1, glow * hoverMul);
      const haloRadius = radius * 2.5;
      const coreRadius = radius * 0.3;

      // Outer halo (large soft glow)
      const haloGrad = ctx.createRadialGradient(node.x, node.y, coreRadius, node.x, node.y, haloRadius);
      haloGrad.addColorStop(0, `rgba(${haloR},${haloG},${haloB},${0.4 * finalGlow})`);
      haloGrad.addColorStop(0.5, `rgba(${haloR},${haloG},${haloB},${0.1 * finalGlow})`);
      haloGrad.addColorStop(1, `rgba(${haloR},${haloG},${haloB},0)`);
      ctx.beginPath();
      ctx.arc(node.x, node.y, haloRadius, 0, Math.PI * 2);
      ctx.fillStyle = haloGrad;
      ctx.fill();

      // Core orb
      const coreGrad = ctx.createRadialGradient(node.x, node.y, 0, node.x, node.y, radius);
      coreGrad.addColorStop(0, `rgba(${coreR},${coreG},${coreB},${0.9 * finalGlow})`);
      coreGrad.addColorStop(0.6, `rgba(${coreR},${coreG},${coreB},${0.5 * finalGlow})`);
      coreGrad.addColorStop(1, `rgba(${coreR},${coreG},${coreB},${0.1 * finalGlow})`);
      ctx.beginPath();
      ctx.arc(node.x, node.y, radius, 0, Math.PI * 2);
      ctx.fillStyle = coreGrad;
      ctx.fill();

      // Bright core dot
      ctx.beginPath();
      ctx.arc(node.x, node.y, coreRadius, 0, Math.PI * 2);
      ctx.fillStyle = `rgba(255,255,255,${0.7 * finalGlow})`;
      ctx.fill();

      // Selected ring
      if (this.selectedNode === node) {
        ctx.beginPath();
        ctx.arc(node.x, node.y, radius + 3, 0, Math.PI * 2);
        ctx.strokeStyle = `rgba(255,255,255,${0.6 * finalGlow})`;
        ctx.lineWidth = 1.5;
        ctx.stroke();
      }
    }
  }

  _drawTooltip(ctx, node) {
    const screen = this._worldToScreen(node.x, node.y);
    const label = node.label || node.id.slice(0, 12);
    const text = label.length > 60 ? label.slice(0, 57) + '...' : label;

    ctx.font = '12px -apple-system, system-ui, sans-serif';
    const metrics = ctx.measureText(text);
    const tw = metrics.width + 16;
    const th = 28;
    const tx = screen.x - tw / 2;
    const ty = screen.y - this._nodeRadius(node) * this.scale - 12 - th;

    ctx.fillStyle = 'rgba(5,5,16,0.9)';
    ctx.beginPath();
    ctx.roundRect(tx, ty, tw, th, 6);
    ctx.fill();
    ctx.strokeStyle = 'rgba(78,205,196,0.3)';
    ctx.lineWidth = 1;
    ctx.stroke();

    ctx.fillStyle = '#E5E5E5';
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    ctx.fillText(text, screen.x, ty + th / 2);
    ctx.textAlign = 'start';
    ctx.textBaseline = 'alphabetic';
  }

  _isNeighbor(a, b) {
    for (const e of this.edges) {
      if ((e.source === a.id && e.target === b.id) ||
          (e.target === a.id && e.source === b.id)) return true;
    }
    return false;
  }

  // ── Event handling ────────────────────────────────────────────────────

  _setupEvents() {
    const canvas = this.canvas;

    canvas.addEventListener('mousemove', (e) => {
      const rect = canvas.getBoundingClientRect();
      const mx = e.clientX - rect.left;
      const my = e.clientY - rect.top;

      if (this.dragNode) {
        this.isDragging = true;
        const world = this._screenToWorld(mx, my);
        this.dragNode.fx = world.x;
        this.dragNode.fy = world.y;
        this.dragNode.x = world.x;
        this.dragNode.y = world.y;
        this.alpha = Math.max(this.alpha, 0.1);
        return;
      }

      if (this.isPanning) {
        const dx = (mx - this.lastMouse.x) / this.scale;
        const dy = (my - this.lastMouse.y) / this.scale;
        this.offsetX += dx;
        this.offsetY += dy;
        this.lastMouse = { x: mx, y: my };
        return;
      }

      const node = this._findNodeAt(mx, my);
      if (node !== this.hoveredNode) {
        this.hoveredNode = node;
        canvas.style.cursor = node ? 'pointer' : 'grab';
        if (this.onNodeHover) this.onNodeHover(node);
      }
    });

    canvas.addEventListener('mousedown', (e) => {
      const rect = canvas.getBoundingClientRect();
      const mx = e.clientX - rect.left;
      const my = e.clientY - rect.top;
      this.lastMouse = { x: mx, y: my };

      const node = this._findNodeAt(mx, my);
      if (node) {
        this.dragNode = node;
        this.isDragging = false;
        canvas.style.cursor = 'grabbing';
      } else {
        this.isPanning = true;
        canvas.style.cursor = 'grabbing';
      }
    });

    canvas.addEventListener('mouseup', (e) => {
      if (this.dragNode && !this.isDragging) {
        if (this.onNodeClick) this.onNodeClick(this.dragNode);
      } else if (!this.isPanning && !this.isDragging) {
        const rect = canvas.getBoundingClientRect();
        const mx = e.clientX - rect.left;
        const my = e.clientY - rect.top;
        const node = this._findNodeAt(mx, my);
        if (!node && this.onNodeClick) this.onNodeClick(null);
      }

      if (this.dragNode) {
        this.dragNode.fx = this.dragNode.x;
        this.dragNode.fy = this.dragNode.y;
      }

      this.dragNode = null;
      this.isDragging = false;
      this.isPanning = false;
      canvas.style.cursor = this.hoveredNode ? 'pointer' : 'grab';
    });

    canvas.addEventListener('mouseleave', () => {
      this.isPanning = false;
      this.dragNode = null;
      this.isDragging = false;
      this.hoveredNode = null;
      canvas.style.cursor = 'grab';
    });

    canvas.addEventListener('wheel', (e) => {
      e.preventDefault();
      const rect = canvas.getBoundingClientRect();
      const mx = e.clientX - rect.left;
      const my = e.clientY - rect.top;

      const worldBefore = this._screenToWorld(mx, my);
      const delta = e.deltaY > 0 ? 0.9 : 1.1;
      this.scale = Math.max(0.05, Math.min(20, this.scale * delta));
      const worldAfter = this._screenToWorld(mx, my);

      this.offsetX += worldAfter.x - worldBefore.x;
      this.offsetY += worldAfter.y - worldBefore.y;
    }, { passive: false });
  }
}
