// HEBBS Memory Palace - Force-directed graph renderer (Canvas 2D)
//
// Implements a spring-based force simulation with:
// - Repulsion between all nodes (Barnes-Hut for >200 nodes)
// - Attraction along edges (spring forces)
// - Gravity toward center
// - Damping for convergence
//
// Visual encoding:
// - Circle = episode memory, hexagon = insight
// - Size proportional to importance
// - Color interpolated from dim amber to bright amber by recency
// - Brightness (alpha) modulated by reinforcement

export class MemoryGraph {
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

    // Search / decay / timeline overlay state
    this.searchHighlight = null;  // Map<memory_id, score> or null
    this.decayMode = false;
    this.visibleNodeIds = null;   // Set<memory_id> or null

    // Physics
    this.running = true;
    this.alpha = 1.0; // cooling factor
    this.alphaDecay = 0.005;
    this.alphaMin = 0.001;

    this._setupEvents();
    this._resize();
    window.addEventListener('resize', () => this._resize());
  }

  setData(nodes, edges, hasProjection, nClusters, clusterLabels) {
    this.nodeMap.clear();
    this.hasProjection = !!hasProjection;
    this.nClusters = nClusters || 0;
    this.clusterLabels = clusterLabels || {};

    // Initialize node positions
    const phi = (1 + Math.sqrt(5)) / 2;
    nodes.forEach((n, i) => {
      if (n.x != null && n.y != null) {
        // Server-provided UMAP positions
      } else {
        // Fallback: Fibonacci spiral
        const theta = 2 * Math.PI * i / phi;
        const r = Math.sqrt(i + 1) * 30;
        n.x = Math.cos(theta) * r;
        n.y = Math.sin(theta) * r;
      }
      n.vx = 0;
      n.vy = 0;
      // Pinned nodes stay fixed at their persisted position
      if (n.pinned) {
        n.fx = n.x;
        n.fy = n.y;
      } else {
        n.fx = null;
        n.fy = null;
      }
      this.nodeMap.set(n.id, n);
    });

    this.nodes = nodes;
    this.edges = edges.map(e => ({
      ...e,
      sourceNode: this.nodeMap.get(e.source),
      targetNode: this.nodeMap.get(e.target),
    })).filter(e => e.sourceNode && e.targetNode);

    // Build cluster groups for rendering
    this._buildClusters();

    // When UMAP positions exist, use minimal force simulation
    if (this.hasProjection) {
      this.alpha = 0.3;
      this.alphaDecay = 0.02;
    } else {
      this.alpha = 1.0;
      this.alphaDecay = 0.005;
    }
    this.running = true;

    // Center view
    this._centerView();
  }

  _buildClusters() {
    this.clusterHulls = [];
    if (this.nClusters === 0) return;

    // Group nodes by cluster
    const groups = new Map();
    for (const node of this.nodes) {
      if (node.cluster == null || node.cluster < 0) continue;
      if (!groups.has(node.cluster)) groups.set(node.cluster, []);
      groups.get(node.cluster).push(node);
    }

    // Cluster colors (muted, translucent)
    const colors = [
      'rgba(245, 158, 11, 0.06)',  // amber
      'rgba(59, 130, 246, 0.06)',  // blue
      'rgba(16, 185, 129, 0.06)', // emerald
      'rgba(168, 85, 247, 0.06)', // purple
      'rgba(239, 68, 68, 0.06)',  // red
      'rgba(14, 165, 233, 0.06)', // sky
      'rgba(251, 146, 60, 0.06)', // orange
      'rgba(34, 197, 94, 0.06)',  // green
    ];

    for (const [clusterId, nodes] of groups) {
      if (nodes.length < 3) continue;
      this.clusterHulls.push({
        clusterId,
        nodes,
        color: colors[clusterId % colors.length],
        label: this.clusterLabels[clusterId] || null,
      });
    }
  }

  start() {
    const tick = () => {
      if (this.running && this.alpha > this.alphaMin) {
        this._simulate();
        this.alpha = Math.max(this.alpha - this.alphaDecay, 0);
      }
      this._render();
      requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  }

  selectNode(id) {
    this.selectedNode = id ? this.nodeMap.get(id) : null;
  }

  setSearchResults(results) {
    // results: array of { memory_id, score } or null to clear
    this.searchHighlight = results
      ? new Map(results.map(r => [r.memory_id, r.score]))
      : null;
  }

  setDecayMode(enabled) {
    this.decayMode = !!enabled;
  }

  setVisibleNodes(nodeIds) {
    // nodeIds: Set of memory IDs, or null to show all
    this.visibleNodeIds = nodeIds;
  }

  exportPNG() {
    // Render at 2x for crisp output
    const dpr = window.devicePixelRatio || 1;
    const exportCanvas = document.createElement('canvas');
    const w = 1200 * 2;
    const h = 630 * 2;
    exportCanvas.width = w;
    exportCanvas.height = h;
    const ctx = exportCanvas.getContext('2d');

    // Black background
    ctx.fillStyle = '#0A0A0B';
    ctx.fillRect(0, 0, w, h);

    // Replicate the current view transform scaled to the export canvas
    const srcW = this.canvas.width / dpr;
    const srcH = this.canvas.height / dpr;
    const scaleX = w / srcW;
    const scaleY = h / srcH;
    const s = Math.min(scaleX, scaleY);

    ctx.save();
    ctx.translate(w / 2, h / 2);
    ctx.scale(this.scale * s, this.scale * s);
    ctx.translate(this.offsetX, this.offsetY);

    // Draw edges
    for (const edge of this.edges) {
      const a = edge.sourceNode, b = edge.targetNode;
      ctx.beginPath();
      ctx.moveTo(a.x, a.y);
      ctx.lineTo(b.x, b.y);
      if (edge.type === 'contradicts') {
        ctx.setLineDash([6, 3]);
        ctx.strokeStyle = `rgba(239, 68, 68, ${0.4 + edge.weight * 0.4})`;
        ctx.lineWidth = 1.5;
      } else if (edge.type === 'similarity') {
        ctx.setLineDash([4, 4]);
        ctx.strokeStyle = `rgba(107, 114, 128, ${0.15 + edge.weight * 0.2})`;
        ctx.lineWidth = 0.5;
      } else {
        ctx.setLineDash([]);
        ctx.strokeStyle = `rgba(120, 53, 15, ${0.3 + edge.weight * 0.4})`;
        ctx.lineWidth = 1;
      }
      ctx.stroke();
      ctx.setLineDash([]);
    }

    // Draw nodes
    for (const node of this.nodes) {
      const radius = 4 + node.importance * 12;
      const r = Math.round(120 + node.recency * 125);
      const g = Math.round(53 + node.recency * 105);
      const bv = Math.round(15 + node.recency * -4);
      const alpha = 0.5 + node.reinforcement * 0.5;

      if (node.kind === 'insight') {
        this._drawHexagon(ctx, node.x, node.y, radius + 1);
        ctx.fillStyle = `rgba(16, 185, 129, ${alpha})`;
        ctx.fill();
      } else {
        ctx.beginPath();
        ctx.arc(node.x, node.y, radius, 0, Math.PI * 2);
        ctx.fillStyle = `rgba(${r}, ${g}, ${bv}, ${alpha})`;
        ctx.fill();
      }

      // Labels for important nodes
      if (node.importance > 0.6 && node.label) {
        ctx.font = '10px -apple-system, system-ui, sans-serif';
        ctx.textAlign = 'center';
        ctx.fillStyle = '#E5E5E5';
        ctx.fillText(node.label, node.x, node.y - radius - 5);
      }
    }

    ctx.restore();

    // Title overlay
    ctx.font = 'bold 28px -apple-system, system-ui, sans-serif';
    ctx.fillStyle = '#F59E0B';
    ctx.fillText('HEBBS Memory Palace', 40, 50);

    // Stats
    ctx.font = '18px -apple-system, system-ui, sans-serif';
    ctx.fillStyle = '#9CA3AF';
    ctx.fillText(`${this.nodes.length} memories`, 40, 80);

    // Trigger download
    exportCanvas.toBlob((blob) => {
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = 'hebbs-memory-palace.png';
      a.click();
      URL.revokeObjectURL(url);
    }, 'image/png');
  }

  exportSVG() {
    if (this.nodes.length === 0) return;

    // Compute viewBox from node positions
    let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
    for (const n of this.nodes) {
      const r = 4 + n.importance * 12 + 2;
      minX = Math.min(minX, n.x - r);
      minY = Math.min(minY, n.y - r);
      maxX = Math.max(maxX, n.x + r);
      maxY = Math.max(maxY, n.y + r);
    }
    const pad = 60;
    minX -= pad; minY -= pad; maxX += pad; maxY += pad;
    const vw = maxX - minX;
    const vh = maxY - minY;

    const esc = (s) => String(s).replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;');

    let svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="${minX} ${minY} ${vw} ${vh}" width="${Math.round(vw)}" height="${Math.round(vh)}">\n`;

    // Background
    svg += `<rect x="${minX}" y="${minY}" width="${vw}" height="${vh}" fill="#0A0A0B"/>\n`;

    // Cluster hulls
    if (this.clusterHulls) {
      for (const cluster of this.clusterHulls) {
        const hull = this._convexHull(cluster.nodes);
        if (hull.length < 3) continue;

        const cx = hull.reduce((s, p) => s + p.x, 0) / hull.length;
        const cy = hull.reduce((s, p) => s + p.y, 0) / hull.length;
        const hullPad = 25;

        let d = '';
        for (let i = 0; i < hull.length; i++) {
          const p = hull[i];
          const dx = p.x - cx, dy = p.y - cy;
          const dist = Math.sqrt(dx * dx + dy * dy) || 1;
          const px = p.x + (dx / dist) * hullPad;
          const py = p.y + (dy / dist) * hullPad;
          d += (i === 0 ? 'M' : 'L') + `${px},${py} `;
        }
        d += 'Z';

        const fillColor = cluster.color;
        const strokeColor = fillColor.replace('0.06', '0.15');
        svg += `<path d="${d}" fill="${fillColor}" stroke="${strokeColor}" stroke-width="1"/>\n`;

        // Cluster label
        if (cluster.label) {
          let topY = Infinity, topX = cx;
          for (const p of hull) {
            const dx = p.x - cx, dy = p.y - cy;
            const dist = Math.sqrt(dx * dx + dy * dy) || 1;
            const py = p.y + (dy / dist) * hullPad;
            if (py < topY) { topY = py; topX = p.x + (dx / dist) * hullPad; }
          }
          svg += `<text x="${cx}" y="${topY - 8}" text-anchor="middle" font-family="system-ui, sans-serif" font-size="11" fill="${fillColor.replace('0.06', '0.5')}">${esc(cluster.label)}</text>\n`;
        }
      }
    }

    // Edges
    for (const edge of this.edges) {
      const a = edge.sourceNode, b = edge.targetNode;
      let stroke, dashArray, width;
      if (edge.type === 'contradicts') {
        stroke = '#EF4444';
        dashArray = ' stroke-dasharray="6,3"';
        width = 1.5;
      } else if (edge.type === 'similarity') {
        stroke = '#6B7280';
        dashArray = ' stroke-dasharray="4,4"';
        width = 0.5;
      } else {
        stroke = '#78350F';
        dashArray = '';
        width = 1;
      }
      svg += `<line x1="${a.x}" y1="${a.y}" x2="${b.x}" y2="${b.y}" stroke="${stroke}" stroke-width="${width}"${dashArray} opacity="0.6"/>\n`;
    }

    // Nodes
    for (const node of this.nodes) {
      const radius = 4 + node.importance * 12;
      const r = Math.round(120 + node.recency * 125);
      const g = Math.round(53 + node.recency * 105);
      const bv = Math.round(15 + node.recency * -4);
      const alpha = 0.5 + node.reinforcement * 0.5;

      if (node.kind === 'insight') {
        // Hexagon
        let points = '';
        for (let i = 0; i < 6; i++) {
          const angle = (Math.PI / 3) * i - Math.PI / 6;
          const px = node.x + (radius + 1) * Math.cos(angle);
          const py = node.y + (radius + 1) * Math.sin(angle);
          points += `${px},${py} `;
        }
        svg += `<polygon points="${points.trim()}" fill="rgba(16, 185, 129, ${alpha})"/>\n`;
      } else {
        svg += `<circle cx="${node.x}" cy="${node.y}" r="${radius}" fill="rgba(${r}, ${g}, ${bv}, ${alpha})"/>\n`;
      }

      // Labels for important nodes
      if (node.importance > 0.6 && node.label) {
        svg += `<text x="${node.x}" y="${node.y - radius - 5}" text-anchor="middle" font-family="-apple-system, system-ui, sans-serif" font-size="10" fill="#E5E5E5">${esc(node.label)}</text>\n`;
      }
    }

    // Title and stats
    svg += `<text x="${minX + 40}" y="${minY + 30}" font-family="-apple-system, system-ui, sans-serif" font-size="28" font-weight="bold" fill="#F59E0B">HEBBS Memory Palace</text>\n`;
    svg += `<text x="${minX + 40}" y="${minY + 55}" font-family="-apple-system, system-ui, sans-serif" font-size="18" fill="#9CA3AF">${this.nodes.length} memories</text>\n`;

    svg += '</svg>';

    // Trigger download
    const blob = new Blob([svg], { type: 'image/svg+xml' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = 'hebbs-memory-palace.svg';
    a.click();
    URL.revokeObjectURL(url);
  }

  // ── Physics simulation ───────────────────────────────────────────────

  _simulate() {
    const nodes = this.nodes;
    const edges = this.edges;
    const alpha = this.alpha;
    const projected = this.hasProjection;

    // When UMAP positions exist, use minimal forces (just light edge attraction)
    // When no projection, use full force-directed layout
    const repulsion = projected ? 0 : 800;
    const springLen = projected ? 0 : 120;
    const springK = projected ? 0.005 : 0.03;
    const gravity = projected ? 0 : 0.01;
    const damping = projected ? 0.5 : 0.85;

    // Repulsion (skip entirely when UMAP-projected)
    if (repulsion > 0) {
      for (let i = 0; i < nodes.length; i++) {
        for (let j = i + 1; j < nodes.length; j++) {
          const a = nodes[i], b = nodes[j];
          let dx = b.x - a.x;
          let dy = b.y - a.y;
          let dist = Math.sqrt(dx * dx + dy * dy) || 1;
          let force = repulsion / (dist * dist);
          let fx = (dx / dist) * force * alpha;
          let fy = (dy / dist) * force * alpha;
          a.vx -= fx;
          a.vy -= fy;
          b.vx += fx;
          b.vy += fy;
        }
      }
    }

    // Attraction along edges (spring force)
    if (springK > 0) {
      for (const edge of edges) {
        const a = edge.sourceNode, b = edge.targetNode;
        let dx = b.x - a.x;
        let dy = b.y - a.y;
        let dist = Math.sqrt(dx * dx + dy * dy) || 1;
        let displacement = dist - springLen;
        let force = springK * displacement * alpha;
        let fx = (dx / dist) * force;
        let fy = (dy / dist) * force;
        a.vx += fx;
        a.vy += fy;
        b.vx -= fx;
        b.vy -= fy;
      }
    }

    // Gravity toward center
    if (gravity > 0) {
      for (const node of nodes) {
        node.vx -= node.x * gravity * alpha;
        node.vy -= node.y * gravity * alpha;
      }
    }

    // Apply velocity with damping
    for (const node of nodes) {
      if (node.fx !== null) {
        node.x = node.fx;
        node.y = node.fy;
        node.vx = 0;
        node.vy = 0;
        continue;
      }
      node.vx *= damping;
      node.vy *= damping;
      node.x += node.vx;
      node.y += node.vy;
    }
  }

  // ── Rendering ────────────────────────────────────────────────────────

  _render() {
    const ctx = this.ctx;
    const w = this.canvas.width;
    const h = this.canvas.height;
    const dpr = window.devicePixelRatio || 1;
    const now = performance.now();

    ctx.clearRect(0, 0, w, h);
    ctx.save();

    // Apply view transform (coordinates in CSS pixels, ctx already scaled by dpr)
    const cx = w / (2 * dpr);
    const cy = h / (2 * dpr);
    ctx.translate(cx, cy);
    ctx.scale(this.scale, this.scale);
    ctx.translate(this.offsetX, this.offsetY);

    const searchActive = this.searchHighlight !== null;
    const visFiltering = this.visibleNodeIds !== null;

    // Build a set of visible node IDs for edge filtering
    const visSet = visFiltering ? this.visibleNodeIds : null;

    // Draw cluster hulls (behind everything else)
    if (this.clusterHulls && !searchActive && !visFiltering) {
      for (const cluster of this.clusterHulls) {
        const hull = this._convexHull(cluster.nodes);
        if (hull.length < 3) continue;

        // Expand hull outward by padding for visual breathing room
        const cx = hull.reduce((s, p) => s + p.x, 0) / hull.length;
        const cy = hull.reduce((s, p) => s + p.y, 0) / hull.length;
        const pad = 25;

        ctx.beginPath();
        for (let i = 0; i < hull.length; i++) {
          const p = hull[i];
          const dx = p.x - cx, dy = p.y - cy;
          const dist = Math.sqrt(dx * dx + dy * dy) || 1;
          const px = p.x + (dx / dist) * pad;
          const py = p.y + (dy / dist) * pad;
          if (i === 0) ctx.moveTo(px, py);
          else ctx.lineTo(px, py);
        }
        ctx.closePath();
        ctx.fillStyle = cluster.color;
        ctx.fill();

        // Subtle border
        ctx.strokeStyle = cluster.color.replace('0.06', '0.15');
        ctx.lineWidth = 1;
        ctx.stroke();

        // Cluster label at top of hull
        if (cluster.label) {
          // Find topmost point of the expanded hull
          let topY = Infinity, topX = cx;
          for (const p of hull) {
            const dx = p.x - cx, dy = p.y - cy;
            const dist = Math.sqrt(dx * dx + dy * dy) || 1;
            const py = p.y + (dy / dist) * pad;
            if (py < topY) { topY = py; topX = p.x + (dx / dist) * pad; }
          }
          ctx.save();
          ctx.font = `${11 / this.scale}px system-ui, sans-serif`;
          ctx.textAlign = 'center';
          ctx.fillStyle = cluster.color.replace('0.06', '0.5');
          ctx.fillText(cluster.label, cx, topY - 8 / this.scale);
          ctx.restore();
        }
      }
    }

    // Draw edges
    for (const edge of this.edges) {
      const a = edge.sourceNode, b = edge.targetNode;

      // Timeline filtering: hide edges where either endpoint is hidden
      if (visSet && (!visSet.has(a.id) || !visSet.has(b.id))) continue;

      // Search mode: fade non-matching edges
      let edgeAlphaMultiplier = 1;
      if (searchActive) {
        const aMatch = this.searchHighlight.has(a.id);
        const bMatch = this.searchHighlight.has(b.id);
        if (!aMatch && !bMatch) {
          edgeAlphaMultiplier = 0.1;
        }
      }

      ctx.beginPath();
      ctx.moveTo(a.x, a.y);
      ctx.lineTo(b.x, b.y);

      if (edge.type === 'contradicts') {
        ctx.setLineDash([6, 3]);
        ctx.strokeStyle = `rgba(239, 68, 68, ${(0.4 + edge.weight * 0.4) * edgeAlphaMultiplier})`;
        ctx.lineWidth = 1.5;
      } else if (edge.type === 'similarity') {
        ctx.setLineDash([4, 4]);
        ctx.strokeStyle = `rgba(107, 114, 128, ${(0.15 + edge.weight * 0.2) * edgeAlphaMultiplier})`;
        ctx.lineWidth = 0.5;
      } else {
        ctx.setLineDash([]);
        ctx.strokeStyle = `rgba(120, 53, 15, ${(0.3 + edge.weight * 0.4) * edgeAlphaMultiplier})`;
        ctx.lineWidth = 1;
      }
      ctx.stroke();
      ctx.setLineDash([]);
    }

    // Draw nodes
    for (const node of this.nodes) {
      // Timeline filtering: completely hide non-visible nodes
      if (visSet && !visSet.has(node.id)) continue;

      const radius = 4 + node.importance * 12;
      const isHovered = this.hoveredNode === node;
      const isSelected = this.selectedNode === node;

      // Color based on recency: dim amber -> bright amber
      const r = Math.round(120 + node.recency * 125); // 120-245
      const g = Math.round(53 + node.recency * 105);  // 53-158
      const b_val = Math.round(15 + node.recency * -4); // 15-11
      let baseAlpha = 0.5 + node.reinforcement * 0.5;

      // Search highlighting
      const isSearchMatch = searchActive && this.searchHighlight.has(node.id);
      if (searchActive && !isSearchMatch) {
        baseAlpha = 0.2; // fade non-matching
      }

      // Decay mode overlay
      const decayScore = node.decay_score !== undefined ? node.decay_score : 1;
      let decayPulse = false;
      if (this.decayMode && decayScore < 0.05) {
        decayPulse = true;
      }

      if (node.kind === 'insight') {
        // Hexagonal shape for insights
        this._drawHexagon(ctx, node.x, node.y, radius + 1);

        if (this.decayMode && decayScore < 0.1) {
          const redAlpha = decayPulse ? 0.4 + 0.3 * Math.sin(now / 300) : 0.4;
          ctx.fillStyle = `rgba(239, 68, 68, ${redAlpha})`;
        } else if (searchActive && !isSearchMatch) {
          ctx.fillStyle = `rgba(107, 114, 128, 0.2)`;
        } else {
          ctx.fillStyle = `rgba(16, 185, 129, ${baseAlpha})`;
        }
        ctx.fill();

        // Glow for search matches
        if (isSearchMatch) {
          const score = this.searchHighlight.get(node.id);
          ctx.strokeStyle = '#F59E0B';
          ctx.lineWidth = 2 + score * 2;
          ctx.shadowColor = '#F59E0B';
          ctx.shadowBlur = 8 + score * 12;
          ctx.stroke();
          ctx.shadowBlur = 0;
        } else {
          ctx.strokeStyle = '#F59E0B';
          ctx.lineWidth = isSelected ? 3 : (isHovered ? 2.5 : 1.5);
          ctx.shadowColor = '#F59E0B';
          ctx.shadowBlur = isSelected ? 15 : (isHovered ? 12 : 6);
          ctx.stroke();
          ctx.shadowBlur = 0;
        }
      } else {
        // Circle for episodes
        ctx.beginPath();
        ctx.arc(node.x, node.y, radius, 0, Math.PI * 2);

        if (this.decayMode && decayScore < 0.1) {
          const redAlpha = decayPulse ? 0.4 + 0.3 * Math.sin(now / 300) : 0.4;
          ctx.fillStyle = `rgba(239, 68, 68, ${redAlpha})`;
          ctx.fill();
        } else if (isSelected) {
          ctx.fillStyle = '#F59E0B';
          ctx.shadowColor = '#F59E0B';
          ctx.shadowBlur = 20;
          ctx.fill();
          ctx.shadowBlur = 0;
          ctx.strokeStyle = '#FDE68A';
          ctx.lineWidth = 2;
          ctx.stroke();
        } else if (isSearchMatch) {
          const score = this.searchHighlight.get(node.id);
          ctx.fillStyle = `rgba(245, 158, 11, ${0.5 + score * 0.5})`;
          ctx.shadowColor = '#F59E0B';
          ctx.shadowBlur = 8 + score * 12;
          ctx.fill();
          ctx.shadowBlur = 0;
          ctx.strokeStyle = `rgba(245, 158, 11, 0.8)`;
          ctx.lineWidth = 1.5;
          ctx.stroke();
        } else if (searchActive && !isSearchMatch) {
          ctx.fillStyle = `rgba(107, 114, 128, 0.2)`;
          ctx.fill();
        } else if (isHovered) {
          ctx.fillStyle = `rgba(${r}, ${g}, ${b_val}, ${Math.min(baseAlpha + 0.2, 1)})`;
          ctx.shadowColor = `rgba(${r}, ${g}, ${b_val}, 0.5)`;
          ctx.shadowBlur = 12;
          ctx.fill();
          ctx.shadowBlur = 0;
          ctx.strokeStyle = `rgba(245, 158, 11, 0.6)`;
          ctx.lineWidth = 1.5;
          ctx.stroke();
        } else {
          ctx.fillStyle = `rgba(${r}, ${g}, ${b_val}, ${baseAlpha})`;
          ctx.fill();
        }
      }

      // Label for hovered/selected or large nodes
      if ((isHovered || isSelected) && node.label) {
        ctx.font = '11px -apple-system, system-ui, sans-serif';
        ctx.textAlign = 'center';
        ctx.fillStyle = '#E5E5E5';
        ctx.fillText(node.label, node.x, node.y - radius - 6);
      }
    }

    ctx.restore();

    // Render tooltip for hovered node (in screen space)
    this._renderTooltip();
  }

  _drawHexagon(ctx, x, y, r) {
    ctx.beginPath();
    for (let i = 0; i < 6; i++) {
      const angle = (Math.PI / 3) * i - Math.PI / 6;
      const px = x + r * Math.cos(angle);
      const py = y + r * Math.sin(angle);
      if (i === 0) ctx.moveTo(px, py);
      else ctx.lineTo(px, py);
    }
    ctx.closePath();
  }

  _renderTooltip() {
    let existing = document.querySelector('.tooltip');
    if (!this.hoveredNode) {
      if (existing) existing.remove();
      return;
    }

    const node = this.hoveredNode;
    if (!existing) {
      existing = document.createElement('div');
      existing.className = 'tooltip';
      document.body.appendChild(existing);
    }

    const kindLabel = node.kind === 'insight' ? '<span class="kind-badge">INSIGHT</span>' : '';
    existing.innerHTML = `${kindLabel}${node.label}`;

    const screenPos = this._worldToScreen(node.x, node.y);
    existing.style.left = (screenPos.x + 15) + 'px';
    existing.style.top = (screenPos.y - 10) + 'px';
  }

  // ── Convex hull (Graham scan) ────────────────────────────────────────

  _convexHull(nodes) {
    if (nodes.length < 3) return nodes.map(n => ({ x: n.x, y: n.y }));

    const points = nodes.map(n => ({ x: n.x, y: n.y }));

    // Find lowest y (leftmost if tie)
    let pivot = 0;
    for (let i = 1; i < points.length; i++) {
      if (points[i].y < points[pivot].y ||
          (points[i].y === points[pivot].y && points[i].x < points[pivot].x)) {
        pivot = i;
      }
    }
    [points[0], points[pivot]] = [points[pivot], points[0]];

    const p0 = points[0];
    points.slice(1).sort((a, b) => {
      const cross = (a.x - p0.x) * (b.y - p0.y) - (b.x - p0.x) * (a.y - p0.y);
      if (Math.abs(cross) < 1e-10) {
        const da = (a.x - p0.x) ** 2 + (a.y - p0.y) ** 2;
        const db = (b.x - p0.x) ** 2 + (b.y - p0.y) ** 2;
        return da - db;
      }
      return -cross;
    });

    const stack = [points[0], points[1]];
    for (let i = 2; i < points.length; i++) {
      while (stack.length > 1) {
        const a = stack[stack.length - 2];
        const b = stack[stack.length - 1];
        const c = points[i];
        const cross = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
        if (cross <= 0) stack.pop();
        else break;
      }
      stack.push(points[i]);
    }
    return stack;
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

  _findNodeAt(sx, sy) {
    const world = this._screenToWorld(sx, sy);
    let closest = null;
    let minDist = Infinity;
    for (const node of this.nodes) {
      const radius = 4 + node.importance * 12 + 4; // hit area slightly larger
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

  // ── Event handling ───────────────────────────────────────────────────

  _resize() {
    const dpr = window.devicePixelRatio || 1;
    const rect = this.canvas.getBoundingClientRect();
    this.canvas.width = rect.width * dpr;
    this.canvas.height = rect.height * dpr;
    this.ctx.scale(dpr, dpr);
  }

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
        this.running = true;
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

      const node = this._findNodeAt(mx, my);
      if (node) {
        this.dragNode = node;
        this.isDragging = false;
      } else {
        this.isPanning = true;
        this.lastMouse = { x: mx, y: my };
        canvas.style.cursor = 'grabbing';
      }
    });

    canvas.addEventListener('mouseup', (e) => {
      if (this.dragNode && !this.isDragging) {
        // Click, not drag
        this.selectedNode = this.dragNode;
        if (this.onNodeClick) this.onNodeClick(this.dragNode);
      }
      if (this.dragNode) {
        if (this.hasProjection && this.isDragging) {
          // Keep node pinned where it was dropped and persist
          this.dragNode.fx = this.dragNode.x;
          this.dragNode.fy = this.dragNode.y;
          this.dragNode.pinned = true;
          fetch(`/api/panel/positions/${this.dragNode.id}`, {
            method: 'PUT',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ x: this.dragNode.x, y: this.dragNode.y }),
          }).catch(() => {});
        } else {
          this.dragNode.fx = null;
          this.dragNode.fy = null;
        }
      }
      this.dragNode = null;
      this.isPanning = false;
      this.isDragging = false;
      canvas.style.cursor = this.hoveredNode ? 'pointer' : 'grab';
    });

    canvas.addEventListener('mouseleave', () => {
      this.hoveredNode = null;
      this.isPanning = false;
      if (this.dragNode) {
        this.dragNode.fx = null;
        this.dragNode.fy = null;
      }
      this.dragNode = null;
      const tooltip = document.querySelector('.tooltip');
      if (tooltip) tooltip.remove();
    });

    // Double-click to unpin a pinned node
    canvas.addEventListener('dblclick', (e) => {
      const rect = canvas.getBoundingClientRect();
      const mx = e.clientX - rect.left;
      const my = e.clientY - rect.top;
      const node = this._findNodeAt(mx, my);
      if (node && node.pinned && this.hasProjection) {
        node.pinned = false;
        node.fx = null;
        node.fy = null;
        this.alpha = Math.max(this.alpha, 0.1);
        this.running = true;
        fetch(`/api/panel/positions/${node.id}/unpin`, { method: 'POST' }).catch(() => {});
      }
    });

    canvas.addEventListener('wheel', (e) => {
      e.preventDefault();
      const rect = canvas.getBoundingClientRect();
      const mx = e.clientX - rect.left;
      const my = e.clientY - rect.top;

      const zoomFactor = e.deltaY > 0 ? 0.9 : 1.1;
      const newScale = Math.max(0.1, Math.min(10, this.scale * zoomFactor));

      // Zoom toward mouse position
      const world = this._screenToWorld(mx, my);
      this.scale = newScale;
      const newWorld = this._screenToWorld(mx, my);
      this.offsetX += newWorld.x - world.x;
      this.offsetY += newWorld.y - world.y;
    }, { passive: false });

    // Keyboard: Escape to deselect
    document.addEventListener('keydown', (e) => {
      if (e.key === 'Escape') {
        this.selectedNode = null;
        if (this.onNodeClick) this.onNodeClick(null);
      }
    });
  }
}
