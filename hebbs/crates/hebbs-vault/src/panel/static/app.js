// HEBBS Memory Palace - Main application
//
// Pure vanilla JS. No frameworks, no build step.
// Manages state, API calls, tab routing, and component rendering.
// 6 tabs: Dashboard, Explorer, Recall, Graph, Timeline, Settings

import { MemoryGraph } from './graph.js';

// ═══════════════════════════════════════════════════════════════════════
//  State
// ═══════════════════════════════════════════════════════════════════════

const state = {
  activeTab: 'graph',
  tabsLoaded: {}, // track which tabs have been loaded

  vaults: [],
  activeVault: null,
  status: null,
  graph: null,
  graphData: null,
  selectedNodeId: null,
  memoryDetail: null,

  // Graph tab: Search + Controls
  searchQuery: '',
  searchResults: null,
  searchLatency: null,
  weights: { relevance: 0.5, recency: 0.2, importance: 0.2, reinforcement: 0.1 },
  strategies: ['similarity', 'temporal'],
  topK: 10,
  filters: { state: '', file_path: '', importance_min: 0, importance_max: 1 },
  decayMode: false,
  healthDetail: null,

  // Timeline (graph tab bottom bar removed, now standalone tab)
  timelineData: null,
  timelinePosition: 100,
  visibleNodeIds: null,
  forgottenData: null,

  // Config
  configData: null,
  configDirty: false,

  // Dashboard
  dashboardData: null,

  // Explorer
  explorerData: null,
  explorerPage: 1,
  explorerSearch: '',
  explorerFilterState: '',
  explorerFilterFile: '',
  explorerSort: 'created_at',

  // Recall tab
  recallQuery: '',
  recallWeights: { relevance: 0.5, recency: 0.2, importance: 0.2, reinforcement: 0.1 },
  recallStrategies: ['similarity', 'temporal'],
  recallTopK: 10,
  recallResults: null,
  recallLatency: null,

  // Notes tab
  notesFiles: null,
  notesActiveFile: null,
  notesFileContent: null,
  notesActiveMemory: null,

  // Queries tab (query audit log)
  queriesData: null,
  queriesStats: null,
  queriesFilterCaller: '',
  queriesFilterOp: '',
  queriesFilterTime: '',
  queriesSearch: '',
};

// ═══════════════════════════════════════════════════════════════════════
//  Helpers
// ═══════════════════════════════════════════════════════════════════════

function debounce(fn, ms) {
  let timer;
  return function (...args) {
    clearTimeout(timer);
    timer = setTimeout(() => fn.apply(this, args), ms);
  };
}

function escapeHtml(str) {
  const div = document.createElement('div');
  div.textContent = str;
  return div.innerHTML;
}

function formatTimestamp(us) {
  if (!us) return 'never';
  const date = new Date(us / 1000);
  const now = new Date();
  const diffMs = now - date;
  const diffMins = Math.floor(diffMs / 60000);
  const diffHours = Math.floor(diffMs / 3600000);
  const diffDays = Math.floor(diffMs / 86400000);

  if (diffMins < 1) return 'just now';
  if (diffMins < 60) return `${diffMins}m ago`;
  if (diffHours < 24) return `${diffHours}h ago`;
  if (diffDays < 30) return `${diffDays}d ago`;
  return date.toLocaleDateString();
}

// ═══════════════════════════════════════════════════════════════════════
//  API
// ═══════════════════════════════════════════════════════════════════════

async function api(path, opts) {
  const res = await fetch(path, opts);
  if (!res.ok) throw new Error(`API error: ${res.status}`);
  return res.json();
}

async function loadVaults() {
  state.vaults = await api('/api/panel/vaults');
  renderVaultDropdown();
}

async function loadStatus() {
  state.status = await api('/api/panel/status');
  renderHealthBadge();
}

async function loadGraph() {
  state.graphData = await api('/api/panel/graph');
  if (state.graphData.nodes.length === 0) {
    // Clear the canvas so stale nodes from a previous vault don't linger
    if (state.graph) state.graph.setData([], [], false, 0, {});
    showEmptyState();
    return;
  }
  hideEmptyState();
  state.graph.setData(
    state.graphData.nodes,
    state.graphData.edges,
    state.graphData.has_projection,
    state.graphData.n_clusters,
    state.graphData.cluster_labels || {}
  );
  populateFileFilter();
}

async function loadMemoryDetail(id) {
  state.memoryDetail = await api(`/api/panel/memories/${id}`);
  renderSidePanel();
}

// ═══════════════════════════════════════════════════════════════════════
//  Detail Drawer (shared across non-graph tabs)
// ═══════════════════════════════════════════════════════════════════════

let _drawers = {};

class DetailDrawer {
  constructor(tabPaneId) {
    this.tabPaneId = tabPaneId;
    this.el = document.querySelector(`#${tabPaneId} .detail-drawer`);
    this.contentEl = this.el.querySelector('.detail-drawer-content');
    this.breadcrumbsEl = this.el.querySelector('.detail-drawer-breadcrumbs');
    this.closeBtn = this.el.querySelector('.detail-drawer-close');
    this.stack = [];

    this.closeBtn.addEventListener('click', () => this.close());

    document.addEventListener('keydown', (e) => {
      if (e.key === 'Escape' && !this.el.classList.contains('hidden')) {
        e.stopPropagation();
        this.close();
      }
    });
  }

  async open(memoryId) {
    const detail = await api(`/api/panel/memories/${memoryId}`);
    const label = detail.heading_path.length > 0
      ? detail.heading_path[detail.heading_path.length - 1]
      : (detail.file_path ? detail.file_path.split('/').pop().replace('.md', '') : memoryId.slice(0, 12));

    const existingIdx = this.stack.findIndex(s => s.id === memoryId);
    if (existingIdx >= 0) {
      this.stack = this.stack.slice(0, existingIdx + 1);
    } else {
      this.stack.push({ id: memoryId, label });
    }

    this._renderBreadcrumbs();
    this._renderContent(detail);
    this.el.classList.remove('hidden');
  }

  close() {
    this.el.classList.add('hidden');
    this.stack = [];
  }

  _renderBreadcrumbs() {
    if (this.stack.length <= 1) {
      this.breadcrumbsEl.innerHTML = '';
      return;
    }
    const parts = this.stack.map((item, i) => {
      if (i === this.stack.length - 1) {
        return `<span class="bc-current">${escapeHtml(item.label)}</span>`;
      }
      return `<span class="bc-item" data-idx="${i}">${escapeHtml(item.label)}</span><span class="bc-sep">\u203A</span>`;
    });
    this.breadcrumbsEl.innerHTML = parts.join('');

    this.breadcrumbsEl.querySelectorAll('.bc-item').forEach(el => {
      el.addEventListener('click', () => {
        const idx = parseInt(el.dataset.idx);
        const target = this.stack[idx];
        this.open(target.id);
      });
    });
  }

  _renderContent(m) {
    const title = m.heading_path.length > 0
      ? m.heading_path[m.heading_path.length - 1]
      : (m.file_path ? m.file_path.split('/').pop().replace('.md', '') : 'Memory');

    let html = `
      <div class="panel-header">
        <span class="panel-kind ${m.kind}">${m.kind}</span>
        ${m.confidence !== null && m.confidence !== undefined
          ? `<span class="confidence-badge">Confidence: ${(m.confidence * 100).toFixed(0)}%</span>`
          : ''}
        <div class="panel-title">${escapeHtml(title)}</div>
        <div class="panel-file">${escapeHtml(m.file_path || m.memory_id)}</div>
      </div>
      <div class="panel-section">
        <div class="panel-section-title">Content</div>
        <div class="panel-content-preview">${escapeHtml(m.content.slice(0, 500))}${m.content.length > 500 ? '...' : ''}</div>
      </div>
      <div class="panel-section">
        <div class="panel-section-title">Score Breakdown</div>
        ${renderScoreRow('Recency', m.scores.recency, 'recency')}
        ${renderScoreRow('Importance', m.scores.importance, 'importance')}
        ${renderScoreRow('Reinforcement', m.scores.reinforcement, 'reinforcement')}
      </div>
      <div class="panel-section">
        <div class="panel-section-title">Metadata</div>
        ${renderMetaRow('Decay score', m.decay_score.toFixed(3))}
        ${renderMetaRow('Access count', m.access_count)}
        ${renderMetaRow('Created', formatTimestamp(m.created_at))}
        ${renderMetaRow('Last accessed', formatTimestamp(m.last_accessed_at))}
        ${m.state ? renderMetaRow('State', m.state) : ''}
      </div>
    `;

    if (m.source_ids && m.source_ids.length > 0) {
      html += `<div class="panel-section">
        <div class="panel-section-title">Source Memories</div>
        ${m.source_ids.map(sid => `
          <div class="edge-item" data-drawer-nav="${sid}">
            <span class="edge-type-badge">source</span>
            <span style="color:var(--text-secondary)">${sid.slice(0, 12)}...</span>
          </div>
        `).join('')}
      </div>`;
    }

    if (m.edges.length > 0) {
      html += `<div class="panel-section">
        <div class="panel-section-title">Edges</div>
        ${m.edges.map(e => `
          <div class="edge-item" data-drawer-nav="${e.target_id}">
            <span class="edge-type-badge">${e.type.replace('_', ' ')}</span>
            <span style="color:var(--text-secondary)">${e.target_id.slice(0, 12)}...</span>
            <span style="color:var(--text-muted);font-size:10px;margin-left:auto">${(e.confidence * 100).toFixed(0)}%</span>
          </div>
        `).join('')}
      </div>`;
    }

    if (m.neighbors && m.neighbors.length > 0) {
      html += `<div class="panel-section">
        <div class="panel-section-title">Similar Memories</div>
        ${m.neighbors.map(n => `
          <div class="neighbor-item" data-drawer-nav="${n.id}">
            <span style="color:var(--amber-bright);font-size:10px;font-family:var(--font-mono)">${(n.similarity * 100).toFixed(0)}%</span>
            <span style="color:var(--text-secondary);overflow:hidden;text-overflow:ellipsis;white-space:nowrap">${escapeHtml(n.label)}</span>
          </div>
        `).join('')}
      </div>`;
    }

    html += `<div class="detail-drawer-graph-link" data-graph-nav="${m.memory_id}">View on graph \u2192</div>`;

    this.contentEl.innerHTML = html;

    this.contentEl.querySelectorAll('[data-drawer-nav]').forEach(el => {
      el.style.cursor = 'pointer';
      el.addEventListener('click', (e) => {
        e.stopPropagation();
        this.open(el.dataset.drawerNav);
      });
    });

    const graphLink = this.contentEl.querySelector('[data-graph-nav]');
    if (graphLink) {
      graphLink.addEventListener('click', () => {
        const id = m.memory_id;
        this.close();
        switchTab('graph');
        state.selectedNodeId = id;
        if (state.graph) state.graph.selectNode(id);
        loadMemoryDetail(id);
      });
    }
  }
}

// ═══════════════════════════════════════════════════════════════════════
//  Tab Router
// ═══════════════════════════════════════════════════════════════════════

function switchTab(tabName) {
  state.activeTab = tabName;

  // Close any open detail drawers
  Object.values(_drawers).forEach(d => d.close());

  // Update tab button states
  document.querySelectorAll('.tab-btn').forEach(btn => {
    btn.classList.toggle('active', btn.dataset.tab === tabName);
  });

  // Show/hide panes
  document.querySelectorAll('.tab-pane').forEach(pane => {
    const isTarget = pane.id === `tab-${tabName}`;
    pane.style.display = isTarget ? 'flex' : 'none';
  });

  // Lazy-load tab data on first visit
  if (!state.tabsLoaded[tabName]) {
    state.tabsLoaded[tabName] = true;
    switch (tabName) {
      case 'dashboard': loadDashboard(); break;
      case 'notes': loadNotesTab(); break;
      case 'explorer': loadExplorer(); break;
      case 'recall': setupRecallTab(); break;
      case 'queries': loadQueriesTab(); break;
      case 'timeline': loadTimelineTab(); break;
      case 'settings': loadSettingsTab(); break;
      case 'graph':
        // Graph may need resize after hidden->visible
        if (state.graph) {
          setTimeout(() => state.graph._resize(), 50);
        }
        break;
    }
  }

  // Graph always needs resize on switch back
  if (tabName === 'graph' && state.graph) {
    setTimeout(() => state.graph._resize(), 50);
  }
}

// ═══════════════════════════════════════════════════════════════════════
//  Dashboard Tab
// ═══════════════════════════════════════════════════════════════════════

async function loadDashboard() {
  try {
    state.dashboardData = await api('/api/panel/dashboard');
    renderDashboard();
  } catch (err) {
    console.error('Dashboard load failed:', err);
    document.getElementById('dashboard-content').innerHTML =
      '<div class="dash-loading">Failed to load dashboard</div>';
  }
}

function renderDashboard() {
  const d = state.dashboardData;
  if (!d) return;

  const el = document.getElementById('dashboard-content');
  const syncClass = d.sync_percentage >= 95 ? '' : d.sync_percentage >= 80 ? 'warn' : 'danger';

  let html = `
    <div class="dash-stats-grid">
      <div class="dash-stat-card">
        <div class="dash-stat-value">${d.total_memories}</div>
        <div class="dash-stat-label">Memories</div>
      </div>
      <div class="dash-stat-card">
        <div class="dash-stat-value">${d.total_insights}</div>
        <div class="dash-stat-label">Insights</div>
      </div>
      <div class="dash-stat-card">
        <div class="dash-stat-value">${d.total_files}</div>
        <div class="dash-stat-label">Files</div>
      </div>
      <div class="dash-stat-card">
        <div class="dash-stat-value">${d.stale}</div>
        <div class="dash-stat-label">Stale</div>
      </div>
    </div>

    <div class="dash-section">
      <div class="dash-section-title">Health</div>
      <div class="dash-health-bar">
        <div class="dash-health-fill ${syncClass}" style="width:${d.sync_percentage}%"></div>
      </div>
      <div class="dash-health-stats">
        <span>${Math.round(d.sync_percentage)}% synced</span>
        <span>${d.synced} synced</span>
        <span>${d.stale} stale</span>
        <span>${d.orphaned} orphaned</span>
        ${d.stale > 0 ? `<button class="resync-btn" id="resync-btn">Re-sync ${d.stale} stale</button>` : ''}
      </div>
    </div>

    <div class="dash-section">
      <div class="dash-section-title">Top Memories (by composite score)</div>
      <div class="dash-top-list">
        ${d.top_memories.map(m => `
          <div class="dash-top-item" onclick="window._openDetail('${m.memory_id}')">
            <span class="dash-top-score">${m.composite_score.toFixed(2)}</span>
            <span class="dash-top-kind ${m.kind}">${m.kind}</span>
            <span class="dash-top-label">${escapeHtml(m.label)}</span>
            <span class="dash-top-file">${escapeHtml(m.file_path.split('/').pop())}</span>
          </div>
        `).join('')}
      </div>
    </div>

    <div class="dash-section">
      <div class="dash-section-title">Recent Activity</div>
      <div class="dash-top-list">
        ${d.recent_activity.map(m => `
          <div class="dash-recent-item" onclick="window._openDetail('${m.memory_id}')">
            <span class="dash-recent-time">${formatTimestamp(m.created_at)}</span>
            <span class="dash-top-kind ${m.kind}">${m.kind}</span>
            <span class="dash-recent-label">${escapeHtml(m.label)}</span>
            <span class="dash-top-file">${escapeHtml(m.file_path.split('/').pop())}</span>
          </div>
        `).join('')}
      </div>
    </div>

    <div class="dash-section">
      <div class="dash-section-title">Scoring Defaults</div>
      <div class="dash-scoring-grid">
        <div class="dash-scoring-item">
          <span class="dash-scoring-label">Relevance</span>
          <span class="dash-scoring-value">${d.scoring_defaults.w_relevance.toFixed(2)}</span>
        </div>
        <div class="dash-scoring-item">
          <span class="dash-scoring-label">Recency</span>
          <span class="dash-scoring-value">${d.scoring_defaults.w_recency.toFixed(2)}</span>
        </div>
        <div class="dash-scoring-item">
          <span class="dash-scoring-label">Importance</span>
          <span class="dash-scoring-value">${d.scoring_defaults.w_importance.toFixed(2)}</span>
        </div>
        <div class="dash-scoring-item">
          <span class="dash-scoring-label">Reinforcement</span>
          <span class="dash-scoring-value">${d.scoring_defaults.w_reinforcement.toFixed(2)}</span>
        </div>
      </div>
    </div>
  `;

  el.innerHTML = html;

  const resyncBtn = document.getElementById('resync-btn');
  if (resyncBtn) {
    resyncBtn.addEventListener('click', async () => {
      resyncBtn.disabled = true;
      resyncBtn.textContent = 'Re-syncing...';
      try {
        const result = await api('/api/panel/resync', { method: 'POST' });
        if (result.status === 'ok') {
          resyncBtn.textContent = `Done: ${result.sections_embedded} embedded`;
          setTimeout(() => loadDashboard(), 1000);
        } else {
          resyncBtn.textContent = 'Failed: ' + (result.message || 'unknown error');
          resyncBtn.disabled = false;
        }
      } catch (err) {
        resyncBtn.textContent = 'Re-sync failed';
        resyncBtn.disabled = false;
      }
    });
  }
}

// ═══════════════════════════════════════════════════════════════════════
//  Notes Tab
// ═══════════════════════════════════════════════════════════════════════

async function loadNotesTab() {
  // Load file list
  try {
    const data = await api('/api/panel/files');
    state.notesFiles = data.files;
    renderNotesFileList();
    setupNotesControls();
  } catch (err) {
    console.error('Notes tab load failed:', err);
  }
}

function setupNotesControls() {
  const searchInput = document.getElementById('notes-file-search');
  const debouncedFilter = debounce(() => {
    renderNotesFileList(searchInput.value.toLowerCase());
  }, 200);
  searchInput.addEventListener('input', debouncedFilter);

  document.getElementById('notes-memory-close').addEventListener('click', () => {
    state.notesActiveMemory = null;
    document.getElementById('notes-memory-panel').style.display = 'none';
    // Remove active state from memory list items
    document.querySelectorAll('.notes-memlist-item.active').forEach(el => el.classList.remove('active'));
  });
}

function renderNotesFileList(filter) {
  const listEl = document.getElementById('notes-file-list');
  const files = state.notesFiles || [];
  const filtered = filter
    ? files.filter(f => f.name.toLowerCase().includes(filter) || f.path.toLowerCase().includes(filter))
    : files;

  let html = '';
  for (const f of filtered) {
    const isActive = state.notesActiveFile === f.path;
    const staleIndicator = f.stale > 0 ? ` <span style="color:var(--error)">+${f.stale} stale</span>` : '';
    html += `
      <div class="notes-file-item${isActive ? ' active' : ''}" data-path="${escapeHtml(f.path)}">
        <div style="min-width:0;flex:1">
          <div class="notes-file-name">${escapeHtml(f.name)}</div>
          ${f.path.includes('/') ? `<div class="notes-file-path">${escapeHtml(f.path)}</div>` : ''}
        </div>
        <span class="notes-file-badge">${f.memory_count}${staleIndicator}</span>
      </div>
    `;
  }

  if (filtered.length === 0) {
    html = '<div style="padding:24px;text-align:center;color:var(--text-muted);font-size:13px">No files found</div>';
  }

  listEl.innerHTML = html;

  // Attach click handlers
  listEl.querySelectorAll('.notes-file-item').forEach(el => {
    el.addEventListener('click', () => {
      const path = el.dataset.path;
      openNoteFile(path);
    });
  });
}

async function openNoteFile(filePath) {
  state.notesActiveFile = filePath;
  state.notesActiveMemory = null;

  // Update sidebar active state
  document.querySelectorAll('.notes-file-item').forEach(el => {
    el.classList.toggle('active', el.dataset.path === filePath);
  });

  // Hide memory panel
  document.getElementById('notes-memory-panel').style.display = 'none';

  try {
    state.notesFileContent = await api(`/api/panel/files/${filePath}`);
    renderNoteContent();
  } catch (err) {
    console.error('Failed to load file:', err);
    document.getElementById('notes-empty').style.display = 'flex';
    document.getElementById('notes-content').style.display = 'none';
  }
}

function renderNoteContent() {
  const fc = state.notesFileContent;
  if (!fc) return;

  document.getElementById('notes-empty').style.display = 'none';
  document.getElementById('notes-content').style.display = 'flex';

  // File header with memory list
  const headerEl = document.getElementById('notes-file-header');
  const fileName = fc.path.split('/').pop();
  const syncedCount = fc.sections.filter(s => s.state === 'synced').length;
  const staleCount = fc.sections.filter(s => s.state !== 'synced').length;

  let memListHtml = '';
  if (fc.sections.length > 0) {
    memListHtml = `<div class="notes-memory-list">
      <div class="notes-memory-list-header" id="notes-memlist-toggle">
        <span>Memories in this file (${fc.sections.length})</span>
        <span class="notes-memlist-arrow">&#9660;</span>
      </div>
      <div class="notes-memory-list-items" id="notes-memlist-items">
        ${fc.sections.map(s => {
          const dotClass = s.state === 'synced' ? 'synced' : 'stale';
          const headingDisplay = s.heading_path.length > 0
            ? s.heading_path.join(' > ')
            : fileName.replace('.md', '');
          return `<div class="notes-memlist-item" data-memory-id="${s.memory_id}" data-heading-path="${escapeHtml(s.heading_path.join('/'))}">
            <span class="notes-memlist-dot ${dotClass}"></span>
            <span class="notes-memlist-label">${escapeHtml(headingDisplay)}</span>
            <span class="notes-memlist-score">${s.importance.toFixed(2)}</span>
            <span class="notes-memlist-state ${s.state}">${s.state}</span>
          </div>`;
        }).join('')}
      </div>
    </div>`;
  }

  headerEl.innerHTML = `
    <h2>${escapeHtml(fileName)}</h2>
    <div class="notes-file-meta">
      <span>${escapeHtml(fc.path)}</span>
      <span>${fc.sections.length} memories (${syncedCount} synced${staleCount > 0 ? `, ${staleCount} stale` : ''})</span>
    </div>
    ${memListHtml}
  `;

  // Toggle memory list collapse
  const toggleBtn = document.getElementById('notes-memlist-toggle');
  if (toggleBtn) {
    toggleBtn.addEventListener('click', () => {
      const items = document.getElementById('notes-memlist-items');
      const arrow = toggleBtn.querySelector('.notes-memlist-arrow');
      items.classList.toggle('collapsed');
      arrow.classList.toggle('collapsed');
    });
  }

  // Attach memory list item click handlers
  headerEl.querySelectorAll('.notes-memlist-item').forEach(el => {
    el.addEventListener('click', () => {
      const memId = el.dataset.memoryId;

      // Highlight this item
      headerEl.querySelectorAll('.notes-memlist-item.active').forEach(m => m.classList.remove('active'));
      el.classList.add('active');

      // Scroll to the corresponding heading in the rendered markdown
      const headingPath = el.dataset.headingPath;
      if (headingPath) {
        const renderedEl = document.getElementById('notes-rendered');
        const headings = renderedEl.querySelectorAll('h1, h2, h3, h4, h5, h6');
        const target = headingPath.split('/').pop();
        for (const h of headings) {
          if (h.textContent.trim() === target) {
            h.scrollIntoView({ behavior: 'smooth', block: 'start' });
            h.classList.add('notes-heading-highlight');
            setTimeout(() => h.classList.remove('notes-heading-highlight'), 2000);
            break;
          }
        }
      }

      // Open the memory detail panel
      openNoteMemory(memId);
    });
  });

  // Render clean markdown (no injected markers)
  const renderedEl = document.getElementById('notes-rendered');
  renderedEl.innerHTML = renderMarkdown(fc.content);
  renderedEl.scrollTop = 0;
}

// Minimal markdown renderer (no external deps)
function renderMarkdown(text) {
  let html = text;

  // Escape HTML entities
  html = html.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

  // Fenced code blocks (``` ... ```)
  html = html.replace(/```(\w*)\n([\s\S]*?)```/g, (_, lang, code) => {
    return `<pre><code class="language-${lang}">${code}</code></pre>`;
  });

  // Inline code
  html = html.replace(/`([^`\n]+)`/g, '<code>$1</code>');

  // Headings
  html = html.replace(/^#{6}\s+(.+)$/gm, '<h6>$1</h6>');
  html = html.replace(/^#{5}\s+(.+)$/gm, '<h5>$1</h5>');
  html = html.replace(/^#{4}\s+(.+)$/gm, '<h4>$1</h4>');
  html = html.replace(/^#{3}\s+(.+)$/gm, '<h3>$1</h3>');
  html = html.replace(/^#{2}\s+(.+)$/gm, '<h2>$1</h2>');
  html = html.replace(/^#{1}\s+(.+)$/gm, '<h1>$1</h1>');

  // Horizontal rules
  html = html.replace(/^---+$/gm, '<hr>');
  html = html.replace(/^\*\*\*+$/gm, '<hr>');

  // Blockquotes
  html = html.replace(/^&gt;\s?(.*)$/gm, '<blockquote><p>$1</p></blockquote>');
  // Merge adjacent blockquotes
  html = html.replace(/<\/blockquote>\n<blockquote>/g, '\n');

  // Bold + italic
  html = html.replace(/\*\*\*(.+?)\*\*\*/g, '<strong><em>$1</em></strong>');
  html = html.replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>');
  html = html.replace(/\*(.+?)\*/g, '<em>$1</em>');

  // Links
  html = html.replace(/\[([^\]]+)\]\(([^)]+)\)/g, '<a href="$2" target="_blank" rel="noopener">$1</a>');

  // Images
  html = html.replace(/!\[([^\]]*)\]\(([^)]+)\)/g, '<img src="$2" alt="$1">');

  // Wiki-links [[...]]
  html = html.replace(/\[\[([^\]]+)\]\]/g, '<a class="wiki-link">$1</a>');

  // Task lists
  html = html.replace(/^(\s*)- \[x\]\s+(.+)$/gm, '$1<li class="task-list-item"><input type="checkbox" checked disabled> $2</li>');
  html = html.replace(/^(\s*)- \[\s?\]\s+(.+)$/gm, '$1<li class="task-list-item"><input type="checkbox" disabled> $2</li>');

  // Unordered lists
  html = html.replace(/^(\s*)[-*+]\s+(.+)$/gm, '$1<li>$2</li>');

  // Ordered lists
  html = html.replace(/^(\s*)\d+\.\s+(.+)$/gm, '$1<li>$2</li>');

  // Wrap consecutive <li> in <ul>
  html = html.replace(/((?:<li[^>]*>.*<\/li>\n?)+)/g, '<ul>$1</ul>');

  // Tables
  html = html.replace(/^(\|.+\|)\n(\|[-| :]+\|)\n((?:\|.+\|\n?)+)/gm, (_, headerRow, sepRow, bodyRows) => {
    const headers = headerRow.split('|').filter(c => c.trim()).map(c => `<th>${c.trim()}</th>`).join('');
    const rows = bodyRows.trim().split('\n').map(row => {
      const cells = row.split('|').filter(c => c.trim()).map(c => `<td>${c.trim()}</td>`).join('');
      return `<tr>${cells}</tr>`;
    }).join('');
    return `<table><thead><tr>${headers}</tr></thead><tbody>${rows}</tbody></table>`;
  });

  // Paragraphs: wrap remaining bare lines
  html = html.replace(/^(?!<[a-z/]|$)(.+)$/gm, '<p>$1</p>');

  // Clean up empty paragraphs
  html = html.replace(/<p>\s*<\/p>/g, '');

  return html;
}

async function openNoteMemory(memoryId) {
  state.notesActiveMemory = memoryId;
  const panel = document.getElementById('notes-memory-panel');
  const contentEl = document.getElementById('notes-memory-content');
  const titleEl = document.getElementById('notes-memory-title');

  panel.style.display = 'flex';
  contentEl.innerHTML = '<div style="padding:24px;color:var(--text-muted)">Loading...</div>';

  try {
    const detail = await api(`/api/panel/memories/${memoryId}`);
    const heading = detail.heading_path.length > 0
      ? detail.heading_path[detail.heading_path.length - 1]
      : (detail.file_path ? detail.file_path.split('/').pop().replace('.md', '') : memoryId.slice(0, 12));

    titleEl.textContent = heading;

    let html = '';

    // Content
    html += `<div class="notes-mem-section">
      <div class="notes-mem-section-title">Content</div>
      <div class="notes-mem-text">${escapeHtml(detail.content)}</div>
    </div>`;

    // Scores
    html += `<div class="notes-mem-section">
      <div class="notes-mem-section-title">Scores</div>
      <div class="notes-mem-scores">
        <div class="notes-mem-score-item">
          <div class="notes-mem-score-value">${detail.importance.toFixed(2)}</div>
          <div class="notes-mem-score-label">Importance</div>
        </div>
        <div class="notes-mem-score-item">
          <div class="notes-mem-score-value">${detail.decay_score.toFixed(3)}</div>
          <div class="notes-mem-score-label">Decay Score</div>
        </div>
        <div class="notes-mem-score-item">
          <div class="notes-mem-score-value">${detail.access_count}</div>
          <div class="notes-mem-score-label">Accesses</div>
        </div>
        <div class="notes-mem-score-item">
          <div class="notes-mem-score-value">${formatTimestamp(detail.created_at)}</div>
          <div class="notes-mem-score-label">Created</div>
        </div>
      </div>
    </div>`;

    // Edges
    if (detail.edges && detail.edges.length > 0) {
      html += `<div class="notes-mem-section">
        <div class="notes-mem-section-title">Edges (${detail.edges.length})</div>
        <div class="notes-related-list">`;
      for (const edge of detail.edges) {
        html += `<div class="notes-related-item" data-nav-id="${edge.target_id}">
          <div class="notes-related-item-label">${edge.type} &rarr; ${edge.target_id.slice(0, 12)}</div>
          <div class="notes-related-item-meta"><span>confidence: ${edge.confidence.toFixed(2)}</span></div>
        </div>`;
      }
      html += '</div></div>';
    }

    // Similar memories (neighbors)
    if (detail.neighbors && detail.neighbors.length > 0) {
      html += `<div class="notes-mem-section">
        <div class="notes-mem-section-title">Related Memories (${detail.neighbors.length})</div>
        <div class="notes-related-list">`;
      for (const n of detail.neighbors) {
        html += `<div class="notes-related-item" data-nav-id="${n.id}">
          <div class="notes-related-item-label">${escapeHtml(n.label)}</div>
          <div class="notes-related-item-meta">
            <span>similarity: ${n.similarity.toFixed(3)}</span>
          </div>
        </div>`;
      }
      html += '</div></div>';
    }

    // Heading path / file info
    if (detail.file_path) {
      html += `<div class="notes-mem-section">
        <div class="notes-mem-section-title">Source</div>
        <div class="notes-mem-text" style="font-size:12px;">
          <div>${escapeHtml(detail.file_path)}</div>
          ${detail.heading_path.length > 0 ? `<div style="color:var(--text-muted)">${detail.heading_path.map(h => escapeHtml(h)).join(' > ')}</div>` : ''}
          <div style="margin-top:8px;font-family:var(--font-mono);font-size:10px;color:var(--text-muted)">${memoryId}</div>
        </div>
      </div>`;
    }

    contentEl.innerHTML = html;

    // Wire up related memory clicks
    contentEl.querySelectorAll('[data-nav-id]').forEach(el => {
      el.addEventListener('click', async () => {
        const targetId = el.dataset.navId;
        // Check if this memory belongs to a file we know about
        const targetDetail = await api(`/api/panel/memories/${targetId}`);
        if (targetDetail.file_path && targetDetail.file_path !== state.notesActiveFile) {
          // Navigate to that file, then open the memory
          await openNoteFile(targetDetail.file_path);
          // Highlight the memory list item
          setTimeout(() => {
            const listItem = document.querySelector(`.notes-memlist-item[data-memory-id="${targetId}"]`);
            if (listItem) {
              listItem.classList.add('active');
              listItem.scrollIntoView({ behavior: 'smooth', block: 'center' });
            }
          }, 100);
        }
        openNoteMemory(targetId);
      });
    });
  } catch (err) {
    console.error('Failed to load memory detail:', err);
    contentEl.innerHTML = '<div style="padding:24px;color:var(--error)">Failed to load memory</div>';
  }
}

// ═══════════════════════════════════════════════════════════════════════
//  Explorer Tab
// ═══════════════════════════════════════════════════════════════════════

async function loadExplorer() {
  setupExplorerControls();
  await fetchExplorerData();
}

function setupExplorerControls() {
  const searchInput = document.getElementById('explorer-search');
  const debouncedSearch = debounce(() => {
    state.explorerSearch = searchInput.value;
    state.explorerPage = 1;
    fetchExplorerData();
  }, 300);
  searchInput.addEventListener('input', debouncedSearch);

  document.getElementById('explorer-filter-state').addEventListener('change', (e) => {
    state.explorerFilterState = e.target.value;
    state.explorerPage = 1;
    fetchExplorerData();
  });

  document.getElementById('explorer-filter-file').addEventListener('change', (e) => {
    state.explorerFilterFile = e.target.value;
    state.explorerPage = 1;
    fetchExplorerData();
  });

  document.getElementById('explorer-sort').addEventListener('change', (e) => {
    state.explorerSort = e.target.value;
    state.explorerPage = 1;
    fetchExplorerData();
  });
}

async function fetchExplorerData() {
  try {
    const params = new URLSearchParams({
      page: state.explorerPage,
      per_page: 50,
      sort_by: state.explorerSort,
      sort_dir: 'desc',
    });
    if (state.explorerSearch) params.set('search', state.explorerSearch);
    if (state.explorerFilterState) params.set('filter_state', state.explorerFilterState);
    if (state.explorerFilterFile) params.set('filter_file', state.explorerFilterFile);

    state.explorerData = await api(`/api/panel/memories?${params}`);
    renderExplorer();

    // Populate file filter on first load
    if (state.graphData) {
      populateExplorerFileFilter();
    }
  } catch (err) {
    console.error('Explorer load failed:', err);
  }
}

function populateExplorerFileFilter() {
  if (!state.graphData || !state.graphData.nodes) return;
  const fileSet = new Set();
  for (const node of state.graphData.nodes) {
    if (node.file_path) fileSet.add(node.file_path);
  }
  const select = document.getElementById('explorer-filter-file');
  if (select.options.length > 1) return; // already populated
  for (const fp of [...fileSet].sort()) {
    const opt = document.createElement('option');
    opt.value = fp;
    opt.textContent = fp.split('/').pop();
    select.appendChild(opt);
  }
}

function renderExplorer() {
  const d = state.explorerData;
  if (!d) return;

  const countEl = document.getElementById('explorer-count');
  countEl.textContent = `${d.total} memories`;

  const listEl = document.getElementById('explorer-list');
  let html = `
    <div class="explorer-header">
      <span>Memory</span>
      <span>File</span>
      <span>Importance</span>
      <span>Decay</span>
      <span>State</span>
      <span>Created</span>
    </div>
  `;

  for (const m of d.memories) {
    html += `
      <div class="explorer-row" onclick="window._openDetail('${m.memory_id}')">
        <div class="explorer-row-label">
          <span class="explorer-row-title">${escapeHtml(m.label)}</span>
          <span class="explorer-row-file">${escapeHtml(m.content_preview.slice(0, 60))}</span>
        </div>
        <span class="explorer-row-file">${escapeHtml(m.file_path.split('/').pop())}</span>
        <span class="explorer-row-score">${m.importance.toFixed(2)}</span>
        <span class="explorer-row-score">${m.decay_score.toFixed(3)}</span>
        <span class="explorer-row-state ${m.state}">${m.state}</span>
        <span class="explorer-row-time">${formatTimestamp(m.created_at)}</span>
      </div>
    `;
  }

  listEl.innerHTML = html;

  // Pagination
  const pagEl = document.getElementById('explorer-pagination');
  if (d.total_pages <= 1) {
    pagEl.innerHTML = '';
    return;
  }
  pagEl.innerHTML = `
    <button class="explorer-page-btn" ${d.page <= 1 ? 'disabled' : ''} onclick="window._explorerPage(${d.page - 1})">Prev</button>
    <span class="explorer-page-info">Page ${d.page} of ${d.total_pages}</span>
    <button class="explorer-page-btn" ${d.page >= d.total_pages ? 'disabled' : ''} onclick="window._explorerPage(${d.page + 1})">Next</button>
  `;
}

window._explorerPage = (page) => {
  state.explorerPage = page;
  fetchExplorerData();
};

// ═══════════════════════════════════════════════════════════════════════
//  Recall Tab
// ═══════════════════════════════════════════════════════════════════════

function setupRecallTab() {
  const queryInput = document.getElementById('recall-query');
  const runBtn = document.getElementById('recall-run');

  runBtn.addEventListener('click', () => doRecall());
  queryInput.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') doRecall();
  });

  // Weight sliders
  const weightKeys = ['relevance', 'recency', 'importance', 'reinforcement'];
  for (const key of weightKeys) {
    const slider = document.getElementById(`recall-w-${key}`);
    const valEl = document.getElementById(`recall-w-${key}-val`);
    slider.addEventListener('input', () => {
      const val = parseInt(slider.value, 10) / 100;
      state.recallWeights[key] = val;
      valEl.textContent = val.toFixed(2);
    });
  }

  // Presets
  const presets = {
    relevance: { relevance: 1.0, recency: 0.0, importance: 0.0, reinforcement: 0.0 },
    recency: { relevance: 0.3, recency: 0.5, importance: 0.1, reinforcement: 0.1 },
    importance: { relevance: 0.2, recency: 0.1, importance: 0.6, reinforcement: 0.1 },
  };

  document.querySelectorAll('#tab-recall .preset-btn').forEach(btn => {
    btn.addEventListener('click', () => {
      const preset = presets[btn.dataset.preset];
      if (!preset) return;
      state.recallWeights = { ...preset };
      for (const key of weightKeys) {
        const slider = document.getElementById(`recall-w-${key}`);
        const valEl = document.getElementById(`recall-w-${key}-val`);
        slider.value = Math.round(state.recallWeights[key] * 100);
        valEl.textContent = state.recallWeights[key].toFixed(2);
      }
    });
  });

  // Strategy checkboxes
  const strategyIds = ['similarity', 'temporal', 'causal', 'analogical'];
  for (const strat of strategyIds) {
    const cb = document.getElementById(`recall-strat-${strat}`);
    cb.addEventListener('change', () => {
      if (cb.checked) {
        if (!state.recallStrategies.includes(strat)) state.recallStrategies.push(strat);
      } else {
        state.recallStrategies = state.recallStrategies.filter(s => s !== strat);
      }
    });
  }

  // Top-K
  document.getElementById('recall-topk').addEventListener('change', (e) => {
    state.recallTopK = parseInt(e.target.value, 10);
  });
}

async function doRecall() {
  const query = document.getElementById('recall-query').value.trim();
  if (!query) return;

  state.recallQuery = query;
  const t0 = performance.now();

  try {
    const body = {
      query,
      weights: state.recallWeights,
      strategies: state.recallStrategies,
      top_k: state.recallTopK,
    };
    const results = await api('/api/panel/recall', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });
    state.recallLatency = Math.round(performance.now() - t0);
    state.recallResults = results.results;
    renderRecallResults();
  } catch (err) {
    console.error('Recall failed:', err);
    document.getElementById('recall-results').innerHTML =
      `<div class="recall-empty">Recall failed: ${err.message}</div>`;
  }
}

function renderRecallResults() {
  const el = document.getElementById('recall-results');
  const results = state.recallResults;

  if (!results || results.length === 0) {
    el.innerHTML = '<div class="recall-empty">No results found.</div>';
    return;
  }

  let html = `<div class="recall-latency">${results.length} results in ${state.recallLatency}ms</div>`;

  for (const r of results) {
    const title = r.heading_path && r.heading_path.length > 0
      ? r.heading_path[r.heading_path.length - 1]
      : (r.file_path ? r.file_path.split('/').pop().replace('.md', '') : r.memory_id.slice(0, 16));

    html += `
      <div class="recall-result-card" onclick="window._openDetail('${r.memory_id}')">
        <div class="recall-result-header">
          <span class="recall-result-title">${escapeHtml(title)}</span>
          <span class="recall-result-score">${r.score.toFixed(4)}</span>
        </div>
        <div class="recall-result-file">${r.file_path ? escapeHtml(r.file_path) : r.kind}</div>
        <div class="recall-score-breakdown">
          <div class="recall-score-item"><span class="label">rel</span> <span class="val">${r.relevance.toFixed(2)}</span></div>
          <div class="recall-score-item"><span class="label">rec</span> <span class="val">${r.recency.toFixed(2)}</span></div>
          <div class="recall-score-item"><span class="label">imp</span> <span class="val">${r.importance.toFixed(2)}</span></div>
          <div class="recall-score-item"><span class="label">rnf</span> <span class="val">${r.reinforcement.toFixed(2)}</span></div>
        </div>
        <div class="recall-result-preview">${escapeHtml(r.content.slice(0, 200))}</div>
      </div>
    `;
  }

  el.innerHTML = html;
}

// ═══════════════════════════════════════════════════════════════════════
//  Queries Tab (Query Audit Log)
// ═══════════════════════════════════════════════════════════════════════

async function loadQueriesTab() {
  setupQueriesControls();
  await fetchQueriesData();
}

function setupQueriesControls() {
  const searchInput = document.getElementById('queries-search');
  const debouncedSearch = debounce(() => {
    state.queriesSearch = searchInput.value;
    fetchQueriesData();
  }, 300);
  searchInput.addEventListener('input', debouncedSearch);

  document.getElementById('queries-filter-op').addEventListener('change', (e) => {
    state.queriesFilterOp = e.target.value;
    fetchQueriesData();
  });

  document.getElementById('queries-filter-time').addEventListener('change', (e) => {
    state.queriesFilterTime = e.target.value;
    fetchQueriesData();
  });
}

async function fetchQueriesData() {
  try {
    const params = new URLSearchParams({ limit: '200' });
    if (state.queriesFilterOp) params.set('operation', state.queriesFilterOp);
    if (state.queriesFilterCaller) params.set('caller', state.queriesFilterCaller);
    if (state.queriesSearch) params.set('query_contains', state.queriesSearch);

    // Time range filter
    if (state.queriesFilterTime) {
      const now = Date.now() * 1000; // microseconds
      const ranges = { '1h': 3600, '24h': 86400, '7d': 604800 };
      const secs = ranges[state.queriesFilterTime];
      if (secs) params.set('since_us', String(now - secs * 1_000_000));
    }

    const [data, stats] = await Promise.all([
      api(`/api/panel/queries?${params}`),
      api('/api/panel/queries/stats'),
    ]);
    state.queriesData = data;
    state.queriesStats = stats;
    renderQueriesTab();
  } catch (err) {
    console.error('Queries load failed:', err);
    document.getElementById('queries-list').innerHTML =
      '<div class="recall-empty">Failed to load query log</div>';
  }
}

function renderQueriesTab() {
  renderCallerChips();
  renderQueriesStats();
  renderQueriesList();
}

function renderCallerChips() {
  const el = document.getElementById('queries-caller-chips');
  const stats = state.queriesStats;
  if (!stats || !stats.by_caller || stats.by_caller.length === 0) {
    el.innerHTML = '';
    return;
  }

  let html = `<span class="queries-chip ${!state.queriesFilterCaller ? 'active' : ''}" onclick="window._queriesFilterCaller('')">All</span>`;
  for (const c of stats.by_caller.sort((a, b) => b.count - a.count)) {
    const active = state.queriesFilterCaller === c.caller ? 'active' : '';
    html += `<span class="queries-chip ${active}" onclick="window._queriesFilterCaller('${escapeHtml(c.caller)}')">
      ${escapeHtml(c.caller)} <span class="queries-chip-count">${c.count}</span>
    </span>`;
  }
  el.innerHTML = html;
}

function renderQueriesStats() {
  const el = document.getElementById('queries-stats');
  const s = state.queriesStats;
  if (!s) { el.innerHTML = ''; return; }

  el.innerHTML = `
    <span><span class="queries-stat-val">${s.total_queries}</span> queries</span>
    <span>avg <span class="queries-stat-val">${(s.avg_latency_us / 1000).toFixed(1)}ms</span></span>
    <span>p99 <span class="queries-stat-val">${(s.p99_latency_us / 1000).toFixed(1)}ms</span></span>
    <span>max <span class="queries-stat-val">${(s.max_latency_us / 1000).toFixed(1)}ms</span></span>
  `;
}

function renderQueriesList() {
  const el = document.getElementById('queries-list');
  const data = state.queriesData;

  if (!data || !data.entries || data.entries.length === 0) {
    el.innerHTML = '<div class="recall-empty">No queries recorded yet. Queries appear here after recall or prime operations.</div>';
    return;
  }

  let html = '';
  for (const e of data.entries) {
    const time = formatTimestamp(e.timestamp_us);
    const timeStr = new Date(e.timestamp_us / 1000).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
    const latencyMs = (e.latency_us / 1000).toFixed(1);
    const queryPreview = e.query || (e.entity_id ? `entity: ${e.entity_id}` : '(no query)');

    const resultIds = (e.result_memory_ids || []).map(id =>
      `<span class="queries-result-id" onclick="event.stopPropagation(); window._openDetail('${id}')" title="${id}">${id.slice(0, 12)}</span>`
    ).join('');

    html += `
      <div class="queries-entry" onclick="this.classList.toggle('expanded')">
        <div class="queries-entry-header">
          <span class="queries-entry-time" title="${time}">${timeStr}</span>
          <span class="queries-entry-caller">${escapeHtml(e.caller)}</span>
          <span class="queries-entry-op ${e.operation}">${e.operation}</span>
          <span class="queries-entry-query">${escapeHtml(queryPreview)}</span>
        </div>
        <div class="queries-entry-meta">
          <span><span class="amber">${e.result_count}</span> results</span>
          <span>top: <span class="amber">${e.top_score.toFixed(2)}</span></span>
          <span>${latencyMs}ms</span>
          ${e.strategy ? `<span>${e.strategy}</span>` : ''}
          ${e.entity_id ? `<span>entity: ${e.entity_id}</span>` : ''}
        </div>
        <div class="queries-entry-detail">
          <div style="font-size:11px;color:var(--text-muted);margin-bottom:4px">Returned memories:</div>
          <div class="queries-result-ids">${resultIds || '<span style="color:var(--text-muted);font-size:11px">none</span>'}</div>
          ${e.result_memory_ids && e.result_memory_ids.length > 0 ? `
            <span class="queries-show-on-graph" onclick="event.stopPropagation(); window._showQueryOnGraph(${JSON.stringify(e.result_memory_ids)})">Show on graph</span>
          ` : ''}
        </div>
      </div>
    `;
  }

  el.innerHTML = html;
}

window._queriesFilterCaller = (caller) => {
  state.queriesFilterCaller = caller || '';
  fetchQueriesData();
};

window._showQueryOnGraph = (memoryIds) => {
  // Switch to graph tab and highlight these memories
  switchTab('graph');
  if (state.graph) {
    const results = memoryIds.map(id => ({ memory_id: id, score: 1.0 }));
    state.graph.setSearchResults(results);
  }
};

// ═══════════════════════════════════════════════════════════════════════
//  Timeline Tab
// ═══════════════════════════════════════════════════════════════════════

async function loadTimelineTab() {
  try {
    const [timeline, health, forgotten] = await Promise.all([
      api('/api/panel/timeline'),
      api('/api/panel/health'),
      api('/api/panel/timeline/forgotten').catch(() => null),
    ]);
    state.timelineData = timeline;
    state.healthDetail = health;
    state.forgottenData = forgotten;
    renderTimelineTab();
  } catch (err) {
    console.error('Timeline load failed:', err);
    document.getElementById('timeline-content').innerHTML =
      '<div class="dash-loading">Failed to load timeline</div>';
  }
}

function renderTimelineTab() {
  const t = state.timelineData;
  const h = state.healthDetail;
  if (!t) return;

  const el = document.getElementById('timeline-content');
  const dc = t.daily_counts || [];
  const maxDaily = Math.max(...dc.map(d => d.memories_added + d.insights_added), 1);

  let html = `
    <div class="dash-section">
      <div class="dash-section-title">Brain Growth</div>
      <div class="timeline-growth-section">
        <div class="timeline-growth-card">
          <div class="timeline-growth-value">${t.growth.total_memories}</div>
          <div class="timeline-growth-label">Total Memories</div>
        </div>
        <div class="timeline-growth-card">
          <div class="timeline-growth-value">${t.growth.total_insights}</div>
          <div class="timeline-growth-label">Total Insights</div>
        </div>
        <div class="timeline-growth-card">
          <div class="timeline-growth-value">${dc.length}</div>
          <div class="timeline-growth-label">Active Days</div>
        </div>
      </div>
    </div>

    <div class="dash-section">
      <div class="dash-section-title">Sparklines</div>
      <div class="timeline-sparkline-section">
        <div class="timeline-spark-card">
          <div class="timeline-spark-title">Memories per day</div>
          <canvas id="tl-spark-memories" class="timeline-spark-canvas" width="400" height="60"></canvas>
        </div>
        <div class="timeline-spark-card">
          <div class="timeline-spark-title">Insights per day</div>
          <canvas id="tl-spark-insights" class="timeline-spark-canvas" width="400" height="60"></canvas>
        </div>
      </div>
    </div>

    <div class="dash-section">
      <div class="dash-section-title">Daily Activity</div>
      <div class="timeline-daily-list">
        ${dc.slice().reverse().map(d => {
          const total = d.memories_added + d.insights_added;
          const pct = (total / maxDaily * 100).toFixed(0);
          return `
            <div class="timeline-day-row">
              <span class="timeline-day-date">${d.date}</span>
              <div class="timeline-day-bar"><div class="timeline-day-fill" style="width:${pct}%"></div></div>
              <span class="timeline-day-count">${total}</span>
            </div>
          `;
        }).join('')}
      </div>
    </div>
  `;

  // Decay candidates
  if (h && h.decay_candidates && h.decay_candidates.length > 0) {
    html += `
      <div class="dash-section timeline-decay-section">
        <div class="dash-section-title">Decay Candidates (${h.decay_candidates.length})</div>
        ${h.decay_candidates.map(c => `
          <div class="timeline-decay-item" onclick="window._openDetail('${c.memory_id}')">
            <span class="timeline-decay-label">${escapeHtml(c.label)}</span>
            <span class="timeline-decay-score">${c.decay_score.toFixed(4)}</span>
          </div>
        `).join('')}
      </div>
    `;
  }

  // Forgotten memories (tombstone log)
  const fg = state.forgottenData;
  if (fg && fg.total_forgotten > 0) {
    html += `
      <div class="dash-section timeline-forgotten-section">
        <div class="dash-section-title">
          Forgotten Memories
          <span class="timeline-forgotten-badge">${fg.total_forgotten}</span>
        </div>
        <div class="timeline-forgotten-list">
          ${fg.recent.map(f => `
            <div class="timeline-forgotten-item">
              <div class="timeline-forgotten-header">
                <span class="timeline-forgotten-id" title="${escapeHtml(f.memory_id)}">${f.memory_id.substring(0, 12)}...</span>
                <span class="timeline-forgotten-time">${escapeHtml(f.forgotten_at_human)}</span>
              </div>
              <div class="timeline-forgotten-criteria">${escapeHtml(f.criteria)}</div>
              ${f.entity_id ? `<div class="timeline-forgotten-entity">Entity: ${escapeHtml(f.entity_id)}</div>` : ''}
              ${f.cascade_count > 0 ? `<div class="timeline-forgotten-cascade">+${f.cascade_count} cascaded snapshots</div>` : ''}
            </div>
          `).join('')}
        </div>
      </div>
    `;
  }

  el.innerHTML = html;

  // Draw sparklines after DOM update
  setTimeout(() => {
    const memCanvas = document.getElementById('tl-spark-memories');
    const insCanvas = document.getElementById('tl-spark-insights');
    if (memCanvas) drawSparkline(memCanvas, dc.map(d => d.memories_added), '#F59E0B');
    if (insCanvas) drawSparkline(insCanvas, dc.map(d => d.insights_added), '#10B981');
  }, 50);
}

function drawSparkline(canvas, data, color) {
  if (!data || data.length === 0) return;
  const ctx = canvas.getContext('2d');
  const w = canvas.width;
  const h = canvas.height;
  ctx.clearRect(0, 0, w, h);

  const max = Math.max(...data, 1);
  const step = w / Math.max(data.length - 1, 1);

  ctx.beginPath();
  ctx.strokeStyle = color;
  ctx.lineWidth = 2;
  ctx.lineJoin = 'round';

  for (let i = 0; i < data.length; i++) {
    const x = i * step;
    const y = h - (data[i] / max) * (h - 6) - 3;
    if (i === 0) ctx.moveTo(x, y);
    else ctx.lineTo(x, y);
  }
  ctx.stroke();

  ctx.lineTo((data.length - 1) * step, h);
  ctx.lineTo(0, h);
  ctx.closePath();
  ctx.fillStyle = color.replace(')', ', 0.1)').replace('rgb', 'rgba');
  ctx.fill();
}

// ═══════════════════════════════════════════════════════════════════════
//  Settings Tab
// ═══════════════════════════════════════════════════════════════════════

async function loadSettingsTab() {
  try {
    state.configData = await api('/api/panel/config');
    state.configDirty = false;
    renderSettingsTab();
  } catch (err) {
    console.error('Settings load failed:', err);
    document.getElementById('settings-content').innerHTML =
      '<div class="dash-loading">Failed to load settings</div>';
  }
}

function renderSettingsTab() {
  if (!state.configData) return;
  const c = state.configData;
  const el = document.getElementById('settings-content');

  let html = `
    <div class="dash-section">
      <div class="dash-section-title">Vault Configuration</div>
      <div style="font-size:12px;color:var(--text-muted);margin-bottom:16px;font-family:var(--font-mono)">.hebbs/config.toml</div>
    </div>
    <div id="settings-status-msg"></div>

    <div class="config-section">
      <div class="config-section-title">Chunking</div>
      <div class="config-field">
        <label>Split on</label>
        <input type="text" id="cfg-split-on" value="${escapeHtml(c.chunking.split_on)}" data-section="chunking" data-key="split_on">
      </div>
      <div class="config-field-info">Heading level to split sections on (e.g. "##" for H2)</div>
      <div class="config-field">
        <label>Min section length</label>
        <input type="number" id="cfg-min-section" value="${c.chunking.min_section_length}" min="0" data-section="chunking" data-key="min_section_length">
      </div>
      <div class="config-field-info">Sections shorter than this merge with parent</div>
    </div>

    <div class="config-section">
      <div class="config-section-title">Embedding</div>
      <div class="config-field">
        <label>Model</label>
        <span class="config-field-readonly">${escapeHtml(c.embedding.model)}</span>
      </div>
      <div class="config-field">
        <label>Dimensions</label>
        <span class="config-field-readonly">${c.embedding.dimensions}</span>
      </div>
      <div class="config-field">
        <label>Batch size</label>
        <input type="number" id="cfg-batch-size" value="${c.embedding.batch_size}" min="1" data-section="embedding" data-key="batch_size">
      </div>
      <div class="config-field-info">Max sections per embedding batch call</div>
    </div>

    <div class="config-section">
      <div class="config-section-title">File Watcher</div>
      <div class="config-field">
        <label>Phase 1 debounce</label>
        <input type="number" id="cfg-p1-debounce" value="${c.watch.phase1_debounce_ms}" min="100" data-section="watch" data-key="phase1_debounce_ms">
      </div>
      <div class="config-field-info">Parse debounce in milliseconds</div>
      <div class="config-field">
        <label>Phase 2 debounce</label>
        <input type="number" id="cfg-p2-debounce" value="${c.watch.phase2_debounce_ms}" min="100" data-section="watch" data-key="phase2_debounce_ms">
      </div>
      <div class="config-field-info">Embed/index debounce in milliseconds</div>
      <div class="config-field">
        <label>Burst threshold</label>
        <input type="number" id="cfg-burst-thresh" value="${c.watch.burst_threshold}" min="1" data-section="watch" data-key="burst_threshold">
      </div>
      <div class="config-field-info">Events in quick succession that trigger burst mode</div>
      <div class="config-field">
        <label>Burst debounce</label>
        <input type="number" id="cfg-burst-debounce" value="${c.watch.burst_debounce_ms}" min="100" data-section="watch" data-key="burst_debounce_ms">
      </div>
      <div class="config-field-info">Extended debounce during burst (ms)</div>
    </div>

    <div class="config-section">
      <div class="config-section-title">Ignore Patterns</div>
      <div id="config-patterns-list" class="config-patterns-list">
        ${(c.watch.ignore_patterns || []).map((p, i) => `
          <div class="config-pattern-item">
            <button class="config-pattern-remove" data-idx="${i}">&times;</button>
            <span>${escapeHtml(p)}</span>
          </div>
        `).join('')}
      </div>
      <div class="config-add-pattern">
        <input type="text" id="cfg-new-pattern" placeholder="e.g. *.tmp">
        <button id="cfg-add-pattern-btn">Add</button>
      </div>
    </div>

    <div class="config-section">
      <div class="config-section-title">Output</div>
      <div class="config-field">
        <label>Insight directory</label>
        <input type="text" id="cfg-insight-dir" value="${escapeHtml(c.output.insight_dir)}" data-section="output" data-key="insight_dir">
      </div>
      <div class="config-field">
        <label>Exclude insight dir from reflect</label>
        <input type="checkbox" class="config-toggle" id="cfg-exclude-reflect" ${c.output.exclude_insight_dir_from_reflect ? 'checked' : ''}>
      </div>
    </div>

    <div class="config-section">
      <div class="config-section-title">Scoring Weights</div>
      <div class="config-field-info">Controls how memories are ranked during recall. Higher weight = more influence on ranking.</div>
      <div class="config-field">
        <label>Relevance</label>
        <input type="number" id="cfg-w-relevance" value="${c.scoring.w_relevance}" min="0" max="10" step="0.01">
      </div>
      <div class="config-field-info">Semantic match to query</div>
      <div class="config-field">
        <label>Recency</label>
        <input type="number" id="cfg-w-recency" value="${c.scoring.w_recency}" min="0" max="10" step="0.01">
      </div>
      <div class="config-field-info">How recent the memory is</div>
      <div class="config-field">
        <label>Importance</label>
        <input type="number" id="cfg-w-importance" value="${c.scoring.w_importance}" min="0" max="10" step="0.01">
      </div>
      <div class="config-field-info">Intrinsic value assigned to the memory</div>
      <div class="config-field">
        <label>Reinforcement</label>
        <input type="number" id="cfg-w-reinforcement" value="${c.scoring.w_reinforcement}" min="0" max="10" step="0.01">
      </div>
      <div class="config-field-info">How often the memory has been accessed</div>
    </div>

    <div class="config-section">
      <div class="config-section-title">Decay</div>
      <div class="config-field-info">Controls how memories fade over time without access.</div>
      <div class="config-field">
        <label>Half-life (days)</label>
        <input type="number" id="cfg-half-life" value="${c.decay.half_life_days}" min="1" step="1">
      </div>
      <div class="config-field-info">Memory strength halves every N days without access</div>
      <div class="config-field">
        <label>Auto-forget threshold</label>
        <input type="number" id="cfg-auto-forget" value="${c.decay.auto_forget_threshold}" min="0" max="1" step="0.001">
      </div>
      <div class="config-field-info">Memories below this decay score are candidates for removal</div>
      <div class="config-field">
        <label>Reinforcement cap</label>
        <input type="number" id="cfg-reinforcement-cap" value="${c.decay.reinforcement_cap}" min="1" step="1">
      </div>
      <div class="config-field-info">Max access count that affects reinforcement scoring</div>
    </div>

    <div id="settings-validation-errors" class="config-status error" style="display:none"></div>

    <div class="config-actions">
      <button id="cfg-save-btn" class="config-save-btn">Save</button>
      <button id="cfg-reset-btn" class="config-reset-btn">Reset to defaults</button>
      <button id="cfg-export-btn" class="config-export-btn">Export TOML</button>
    </div>
  `;

  el.innerHTML = html;
  setupSettingsEventListeners();
}

function setupSettingsEventListeners() {
  // Mark dirty on any input change
  document.querySelectorAll('#settings-content input').forEach(input => {
    input.addEventListener('input', () => { state.configDirty = true; });
  });

  // Remove pattern
  document.querySelectorAll('#settings-content .config-pattern-remove').forEach(btn => {
    btn.addEventListener('click', () => {
      const idx = parseInt(btn.dataset.idx, 10);
      state.configData.watch.ignore_patterns.splice(idx, 1);
      state.configDirty = true;
      renderSettingsTab();
    });
  });

  // Add pattern
  const addBtn = document.getElementById('cfg-add-pattern-btn');
  if (addBtn) {
    addBtn.addEventListener('click', () => {
      const input = document.getElementById('cfg-new-pattern');
      const val = input.value.trim();
      if (val) {
        state.configData.watch.ignore_patterns.push(val);
        state.configDirty = true;
        renderSettingsTab();
      }
    });
  }

  // Save
  const saveBtn = document.getElementById('cfg-save-btn');
  if (saveBtn) {
    saveBtn.addEventListener('click', async () => {
      await saveConfig();
    });
  }

  // Reset
  const resetBtn = document.getElementById('cfg-reset-btn');
  if (resetBtn) {
    resetBtn.addEventListener('click', async () => {
      try {
        const data = await api('/api/panel/config/reset', { method: 'POST' });
        state.configData = data;
        state.configDirty = false;
        renderSettingsTab();
        showSettingsStatus('Reset to factory defaults', 'success');
      } catch (err) {
        showSettingsStatus('Reset failed: ' + err.message, 'error');
      }
    });
  }

  // Export
  const exportBtn = document.getElementById('cfg-export-btn');
  if (exportBtn) {
    exportBtn.addEventListener('click', () => {
      window.open('/api/panel/config/export', '_blank');
    });
  }
}

async function saveConfig() {
  const body = {};
  const c = state.configData;

  const splitOn = document.getElementById('cfg-split-on')?.value;
  const minSection = document.getElementById('cfg-min-section')?.value;
  if (splitOn !== undefined || minSection !== undefined) {
    body.chunking = {};
    if (splitOn !== undefined) body.chunking.split_on = splitOn;
    if (minSection !== undefined) body.chunking.min_section_length = parseInt(minSection, 10);
  }

  const batchSize = document.getElementById('cfg-batch-size')?.value;
  if (batchSize !== undefined) {
    body.embedding = { batch_size: parseInt(batchSize, 10) };
  }

  const p1 = document.getElementById('cfg-p1-debounce')?.value;
  const p2 = document.getElementById('cfg-p2-debounce')?.value;
  const bt = document.getElementById('cfg-burst-thresh')?.value;
  const bd = document.getElementById('cfg-burst-debounce')?.value;
  body.watch = {
    phase1_debounce_ms: p1 ? parseInt(p1, 10) : c.watch.phase1_debounce_ms,
    phase2_debounce_ms: p2 ? parseInt(p2, 10) : c.watch.phase2_debounce_ms,
    burst_threshold: bt ? parseInt(bt, 10) : c.watch.burst_threshold,
    burst_debounce_ms: bd ? parseInt(bd, 10) : c.watch.burst_debounce_ms,
    ignore_patterns: c.watch.ignore_patterns,
  };

  const insightDir = document.getElementById('cfg-insight-dir')?.value;
  const excludeReflect = document.getElementById('cfg-exclude-reflect')?.checked;
  body.output = {
    insight_dir: insightDir || c.output.insight_dir,
    exclude_insight_dir_from_reflect: excludeReflect !== undefined ? excludeReflect : c.output.exclude_insight_dir_from_reflect,
  };

  // Scoring weights
  body.scoring = {
    w_relevance: parseFloat(document.getElementById('cfg-w-relevance')?.value) || c.scoring.w_relevance,
    w_recency: parseFloat(document.getElementById('cfg-w-recency')?.value) || c.scoring.w_recency,
    w_importance: parseFloat(document.getElementById('cfg-w-importance')?.value) || c.scoring.w_importance,
    w_reinforcement: parseFloat(document.getElementById('cfg-w-reinforcement')?.value) || c.scoring.w_reinforcement,
  };

  // Decay
  body.decay = {
    half_life_days: parseFloat(document.getElementById('cfg-half-life')?.value) || c.decay.half_life_days,
    auto_forget_threshold: parseFloat(document.getElementById('cfg-auto-forget')?.value),
    reinforcement_cap: parseInt(document.getElementById('cfg-reinforcement-cap')?.value, 10) || c.decay.reinforcement_cap,
  };
  // Handle 0 as valid for auto_forget_threshold
  if (isNaN(body.decay.auto_forget_threshold)) {
    body.decay.auto_forget_threshold = c.decay.auto_forget_threshold;
  }

  // Clear previous validation errors
  const errEl = document.getElementById('settings-validation-errors');
  if (errEl) { errEl.style.display = 'none'; errEl.textContent = ''; }

  try {
    const res = await fetch('/api/panel/config', {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });
    if (!res.ok) {
      if (res.status === 400) {
        const errorText = await res.text();
        try {
          const errors = JSON.parse(errorText);
          const msgs = Object.entries(errors).map(([field, msg]) => `${field}: ${msg}`).join('\n');
          if (errEl) {
            errEl.textContent = msgs;
            errEl.style.display = 'block';
          }
          showSettingsStatus('Validation failed', 'error');
        } catch {
          showSettingsStatus('Validation failed: ' + errorText, 'error');
        }
        return;
      }
      throw new Error(`API error: ${res.status}`);
    }
    const updated = await res.json();
    state.configData = updated;
    state.configDirty = false;
    showSettingsStatus('Configuration saved', 'success');
  } catch (err) {
    showSettingsStatus('Save failed: ' + err.message, 'error');
  }
}

function showSettingsStatus(msg, type) {
  const el = document.getElementById('settings-status-msg');
  if (el) {
    el.className = 'config-status ' + type;
    el.textContent = msg;
    setTimeout(() => { el.textContent = ''; el.className = ''; }, 3000);
  }
}

// ═══════════════════════════════════════════════════════════════════════
//  Graph Tab: Search
// ═══════════════════════════════════════════════════════════════════════

async function doSearch() {
  const query = state.searchQuery.trim();
  if (!query) {
    state.searchResults = null;
    state.searchLatency = null;
    state.graph.setSearchResults(null);
    document.getElementById('latency-badge').textContent = '';
    if (!state.memoryDetail) {
      const panel = document.getElementById('side-panel');
      panel.classList.add('hidden');
    }
    return;
  }

  const t0 = performance.now();
  try {
    const body = {
      query,
      weights: state.weights,
      strategies: state.strategies,
      top_k: state.topK,
      filters: {
        state: state.filters.state || undefined,
        file_path: state.filters.file_path || undefined,
        importance_min: state.filters.importance_min,
        importance_max: state.filters.importance_max,
      },
    };
    const results = await api('/api/panel/recall', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });
    const latency = Math.round(performance.now() - t0);

    state.searchResults = results.results;
    state.searchLatency = latency;
    state.graph.setSearchResults(state.searchResults);

    document.getElementById('latency-badge').textContent = `${latency}ms`;
    renderSearchResults();
  } catch (err) {
    console.error('Search failed:', err);
    state.searchResults = [];
    state.searchLatency = null;
    state.graph.setSearchResults(null);
    document.getElementById('latency-badge').textContent = 'err';
  }
}

function renderSearchResults() {
  const results = state.searchResults;
  if (!results || results.length === 0) return;

  const panel = document.getElementById('side-panel');
  const content = document.getElementById('side-panel-content');
  panel.classList.remove('hidden');

  let html = `
    <div class="panel-header">
      <div class="panel-title">Search Results</div>
      <div class="panel-file">${results.length} results in ${state.searchLatency}ms</div>
    </div>
  `;

  for (const r of results) {
    const scoreStr = (r.score !== undefined ? r.score : 0).toFixed(3);
    const hp = r.heading_path || [];
    const title = hp.length > 0 ? hp[hp.length - 1] : (r.file_path ? r.file_path.split('/').pop().replace('.md', '') : r.memory_id.slice(0, 16));
    const preview = r.content ? r.content.slice(0, 120) : '';
    const kind = r.kind || 'episode';
    html += `
      <div class="search-result-item" onclick="window._selectNode('${r.memory_id}')">
        <div style="display:flex;justify-content:space-between;align-items:center">
          <span class="search-result-title">${escapeHtml(title)}</span>
          <span class="search-result-score">${scoreStr}</span>
        </div>
        <div class="search-result-preview">${escapeHtml(preview)}</div>
        <div class="search-result-meta">
          <span>${kind}</span>
          ${r.file_path ? `<span>${escapeHtml(r.file_path)}</span>` : ''}
        </div>
      </div>
    `;
  }

  content.innerHTML = html;
}

// ═══════════════════════════════════════════════════════════════════════
//  Graph Tab: Health Detail
// ═══════════════════════════════════════════════════════════════════════

async function loadHealthDetail() {
  try {
    state.healthDetail = await api('/api/panel/health');
    renderHealthDetail();
  } catch (err) {
    console.error('Failed to load health detail:', err);
  }
}

function renderHealthDetail() {
  if (!state.healthDetail) return;

  const panel = document.getElementById('side-panel');
  const content = document.getElementById('side-panel-content');
  panel.classList.remove('hidden');

  const h = state.healthDetail;
  let html = `
    <div class="panel-header">
      <div class="panel-title">Vault Health</div>
    </div>
  `;

  if (h.stale_files && h.stale_files.length > 0) {
    html += `
      <div class="panel-section">
        <div class="health-detail-title">Stale Files (${h.stale_files.length})</div>
        ${h.stale_files.map(f => `
          <div class="health-item">
            <span style="color:var(--text-secondary)">${escapeHtml(f.path)}</span>
            <span style="color:var(--amber-bright);font-size:10px">${f.sections_stale} sections</span>
          </div>
        `).join('')}
      </div>
    `;
  }

  if (h.orphaned_memories && h.orphaned_memories.length > 0) {
    html += `
      <div class="panel-section">
        <div class="health-detail-title">Orphaned Memories (${h.orphaned_memories.length})</div>
        ${h.orphaned_memories.map(m => `
          <div class="health-item" onclick="window._selectNode('${m.memory_id}')">
            <span style="color:var(--text-secondary)">${escapeHtml(m.content_preview || m.memory_id.slice(0, 16))}</span>
            <button class="health-action-btn danger" onclick="event.stopPropagation()">Remove</button>
          </div>
        `).join('')}
      </div>
    `;
  }

  if (h.decay_candidates && h.decay_candidates.length > 0) {
    html += `
      <div class="panel-section">
        <div class="health-detail-title">Decay Candidates (${h.decay_candidates.length})</div>
        ${h.decay_candidates.map(m => `
          <div class="health-item" onclick="window._selectNode('${m.memory_id}')">
            <span style="color:var(--text-secondary)">${escapeHtml(m.label || m.memory_id.slice(0, 16))}</span>
            <span style="color:var(--error);font-size:10px;font-family:var(--font-mono)">${(m.decay_score || 0).toFixed(3)}</span>
          </div>
        `).join('')}
      </div>
    `;
  }

  if ((!h.stale_files || h.stale_files.length === 0) && (!h.orphaned_memories || h.orphaned_memories.length === 0) && (!h.decay_candidates || h.decay_candidates.length === 0)) {
    html += `<div class="panel-section"><div style="color:var(--success);font-size:13px">All clear. No health issues detected.</div></div>`;
  }

  content.innerHTML = html;
}

// ═══════════════════════════════════════════════════════════════════════
//  Graph Tab: Controls Setup
// ═══════════════════════════════════════════════════════════════════════

function setupGraphControls() {
  // Controls toggle
  const controlsEl = document.getElementById('controls');
  const toggleBtn = document.getElementById('controls-toggle');
  toggleBtn.addEventListener('click', () => {
    controlsEl.classList.toggle('collapsed');
    setTimeout(() => { if (state.graph) state.graph._resize(); }, 250);
  });

  // Weight sliders
  const weightKeys = ['relevance', 'recency', 'importance', 'reinforcement'];
  const debouncedSearch = debounce(() => { if (state.searchQuery) doSearch(); }, 150);

  for (const key of weightKeys) {
    const slider = document.getElementById(`w-${key}`);
    const valEl = document.getElementById(`w-${key}-val`);
    slider.addEventListener('input', () => {
      const val = parseInt(slider.value, 10) / 100;
      state.weights[key] = val;
      valEl.textContent = val.toFixed(2);
      debouncedSearch();
    });
  }

  // Presets
  const presets = {
    relevance: { relevance: 1.0, recency: 0.0, importance: 0.0, reinforcement: 0.0 },
    recency: { relevance: 0.3, recency: 0.5, importance: 0.1, reinforcement: 0.1 },
    importance: { relevance: 0.2, recency: 0.1, importance: 0.6, reinforcement: 0.1 },
  };

  document.querySelectorAll('#tab-graph .preset-btn').forEach(btn => {
    btn.addEventListener('click', () => {
      const preset = presets[btn.dataset.preset];
      if (!preset) return;
      state.weights = { ...preset };
      for (const key of weightKeys) {
        const slider = document.getElementById(`w-${key}`);
        const valEl = document.getElementById(`w-${key}-val`);
        slider.value = Math.round(state.weights[key] * 100);
        valEl.textContent = state.weights[key].toFixed(2);
      }
      if (state.searchQuery) doSearch();
    });
  });

  // Strategy checkboxes
  const strategyIds = ['similarity', 'temporal', 'causal', 'analogical'];
  for (const strat of strategyIds) {
    const cb = document.getElementById(`strat-${strat}`);
    cb.addEventListener('change', () => {
      if (cb.checked) {
        if (!state.strategies.includes(strat)) state.strategies.push(strat);
      } else {
        state.strategies = state.strategies.filter(s => s !== strat);
      }
      if (state.searchQuery) doSearch();
    });
  }

  // Top-K
  document.getElementById('top-k-select').addEventListener('change', (e) => {
    state.topK = parseInt(e.target.value, 10);
    if (state.searchQuery) doSearch();
  });

  // Filters
  document.getElementById('filter-state').addEventListener('change', (e) => {
    state.filters.state = e.target.value;
    if (state.searchQuery) doSearch();
  });

  document.getElementById('filter-file').addEventListener('change', (e) => {
    state.filters.file_path = e.target.value;
    if (state.searchQuery) doSearch();
  });

  const impMinSlider = document.getElementById('filter-imp-min');
  const impMaxSlider = document.getElementById('filter-imp-max');
  const debouncedFilter = debounce(() => { if (state.searchQuery) doSearch(); }, 150);

  impMinSlider.addEventListener('input', () => {
    state.filters.importance_min = parseInt(impMinSlider.value, 10) / 100;
    debouncedFilter();
  });
  impMaxSlider.addEventListener('input', () => {
    state.filters.importance_max = parseInt(impMaxSlider.value, 10) / 100;
    debouncedFilter();
  });

  // Search input
  const searchInput = document.getElementById('search-input');
  const debouncedSearchInput = debounce(() => {
    state.searchQuery = searchInput.value;
    doSearch();
  }, 300);
  searchInput.addEventListener('input', debouncedSearchInput);
  searchInput.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') {
      searchInput.value = '';
      state.searchQuery = '';
      state.searchResults = null;
      state.searchLatency = null;
      state.graph.setSearchResults(null);
      document.getElementById('latency-badge').textContent = '';
      renderSidePanel();
    }
  });

  // Decay toggle
  const decayToggle = document.getElementById('decay-toggle');
  decayToggle.addEventListener('click', () => {
    state.decayMode = !state.decayMode;
    decayToggle.classList.toggle('active', state.decayMode);
    decayToggle.textContent = state.decayMode ? 'Hide decay' : 'Show decay';
    state.graph.setDecayMode(state.decayMode);
    if (state.decayMode) {
      loadHealthDetail();
    }
  });

  // Health badge click
  document.getElementById('health-badge').addEventListener('click', () => {
    loadHealthDetail();
  });

  // Export PNG
  document.getElementById('export-png').addEventListener('click', () => {
    if (state.graph) state.graph.exportPNG();
  });

  // Export SVG
  document.getElementById('export-svg').addEventListener('click', () => {
    if (state.graph) state.graph.exportSVG();
  });
}

function populateFileFilter() {
  if (!state.graphData || !state.graphData.nodes) return;
  const fileSet = new Set();
  for (const node of state.graphData.nodes) {
    if (node.file_path) fileSet.add(node.file_path);
  }
  const select = document.getElementById('filter-file');
  select.innerHTML = '<option value="">All files</option>';
  for (const fp of [...fileSet].sort()) {
    const opt = document.createElement('option');
    opt.value = fp;
    opt.textContent = fp.split('/').pop();
    select.appendChild(opt);
  }
}

// ═══════════════════════════════════════════════════════════════════════
//  Graph Tab: Side Panel (memory detail)
// ═══════════════════════════════════════════════════════════════════════

function renderVaultDropdown() {
  const select = document.getElementById('vault-select');
  select.innerHTML = '';
  if (state.vaults.length === 0 && state.status) {
    const opt = document.createElement('option');
    opt.value = state.status.vault_path;
    opt.textContent = state.status.vault_path.split('/').pop() || state.status.vault_path;
    opt.selected = true;
    select.appendChild(opt);
    return;
  }
  for (const vault of state.vaults) {
    const opt = document.createElement('option');
    opt.value = vault.path;
    opt.textContent = vault.label || vault.path;
    opt.selected = vault.active;
    select.appendChild(opt);
  }
  select.addEventListener('change', async () => {
    const selectedPath = select.value;
    try {
      const resp = await fetch('/api/panel/vaults/switch', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ path: selectedPath }),
      });
      if (!resp.ok) {
        console.error('Failed to switch vault:', resp.statusText);
        return;
      }
      await Promise.all([loadStatus(), loadGraph()]);
    } catch (err) {
      console.error('Failed to switch vault:', err);
    }
  });
}

function renderHealthBadge() {
  const el = document.getElementById('health-text');
  if (!state.status) {
    el.textContent = 'Loading...';
    return;
  }
  const s = state.status;
  const parts = [];
  parts.push(`<span class="count">${s.memory_count}</span> memories`);
  if (s.insight_count > 0) {
    parts.push(`<span class="count">${s.insight_count}</span> insights`);
  }

  const syncClass = s.sync_percentage >= 95 ? 'sync-ok' : (s.sync_percentage >= 80 ? '' : 'stale');
  parts.push(`<span class="${syncClass}">${Math.round(s.sync_percentage)}% synced</span>`);

  if (s.stale > 0) {
    parts.push(`<span class="stale">${s.stale} stale</span>`);
  }
  if (s.orphaned > 0) {
    parts.push(`<span class="orphaned">${s.orphaned} orphaned</span>`);
  }

  el.innerHTML = parts.join('<span class="separator">|</span>');
}

function renderSidePanel() {
  const panel = document.getElementById('side-panel');
  const content = document.getElementById('side-panel-content');

  if (state.searchResults && state.searchResults.length > 0 && !state.memoryDetail) {
    renderSearchResults();
    return;
  }

  if (!state.memoryDetail) {
    panel.classList.add('hidden');
    return;
  }

  const m = state.memoryDetail;
  panel.classList.remove('hidden');

  const kindClass = m.kind;
  const title = m.heading_path.length > 0
    ? m.heading_path[m.heading_path.length - 1]
    : (m.file_path ? m.file_path.split('/').pop().replace('.md', '') : 'Memory');

  let html = `
    <div class="panel-header">
      <span class="panel-kind ${kindClass}">${m.kind}</span>
      ${m.confidence !== null && m.confidence !== undefined
        ? `<span class="confidence-badge">Confidence: ${(m.confidence * 100).toFixed(0)}%</span>`
        : ''}
      <div class="panel-title">${escapeHtml(title)}</div>
      <div class="panel-file">${escapeHtml(m.file_path || m.memory_id)}</div>
    </div>

    <div class="panel-section">
      <div class="panel-section-title">Content</div>
      <div class="panel-content-preview">${escapeHtml(m.content.slice(0, 500))}${m.content.length > 500 ? '...' : ''}</div>
    </div>

    <div class="panel-section">
      <div class="panel-section-title">Score Breakdown</div>
      ${renderScoreRow('Recency', m.scores.recency, 'recency')}
      ${renderScoreRow('Importance', m.scores.importance, 'importance')}
      ${renderScoreRow('Reinforcement', m.scores.reinforcement, 'reinforcement')}
    </div>

    <div class="panel-section">
      <div class="panel-section-title">Metadata</div>
      ${renderMetaRow('Decay score', m.decay_score.toFixed(3))}
      ${renderMetaRow('Access count', m.access_count)}
      ${renderMetaRow('Created', formatTimestamp(m.created_at))}
      ${renderMetaRow('Last accessed', formatTimestamp(m.last_accessed_at))}
      ${m.state ? renderMetaRow('State', m.state) : ''}
    </div>
  `;

  if (m.source_ids && m.source_ids.length > 0) {
    html += `
      <div class="panel-section">
        <div class="panel-section-title">Source Memories</div>
        ${m.source_ids.map(sid => `
          <div class="edge-item" data-id="${sid}" onclick="window._selectNode('${sid}')">
            <span class="edge-type-badge">source</span>
            <span style="color:var(--text-secondary)">${sid.slice(0, 12)}...</span>
          </div>
        `).join('')}
      </div>
    `;
  }

  if (m.edges.length > 0) {
    html += `
      <div class="panel-section">
        <div class="panel-section-title">Edges</div>
        ${m.edges.map(e => `
          <div class="edge-item" data-id="${e.target_id}" onclick="window._selectNode('${e.target_id}')">
            <span class="edge-type-badge">${e.type.replace('_', ' ')}</span>
            <span style="color:var(--text-secondary)">${e.target_id.slice(0, 12)}...</span>
            <span style="color:var(--text-muted);font-size:10px;margin-left:auto">${(e.confidence * 100).toFixed(0)}%</span>
          </div>
        `).join('')}
      </div>
    `;
  }

  if (m.neighbors.length > 0) {
    html += `
      <div class="panel-section">
        <div class="panel-section-title">Similar Memories</div>
        ${m.neighbors.map(n => `
          <div class="neighbor-item" data-id="${n.id}" onclick="window._selectNode('${n.id}')">
            <span style="color:var(--amber-bright);font-size:10px;font-family:var(--font-mono)">${(n.similarity * 100).toFixed(0)}%</span>
            <span style="color:var(--text-secondary);overflow:hidden;text-overflow:ellipsis;white-space:nowrap">${escapeHtml(n.label)}</span>
          </div>
        `).join('')}
      </div>
    `;
  }

  content.innerHTML = html;
}

function renderScoreRow(label, score, cssClass) {
  const pct = (score.raw * 100).toFixed(0);
  return `
    <div class="score-row">
      <span class="score-label">${label}</span>
      <div class="score-bar-container">
        <div class="score-bar ${cssClass}" style="width: ${pct}%"></div>
      </div>
      <span class="score-value">${score.raw.toFixed(2)} x ${score.weight.toFixed(2)} = ${score.weighted.toFixed(3)}</span>
    </div>
  `;
}

function renderMetaRow(label, value) {
  return `
    <div class="meta-row">
      <span class="meta-label">${label}</span>
      <span class="meta-value">${value}</span>
    </div>
  `;
}

function showEmptyState() {
  let el = document.querySelector('.empty-state');
  if (!el) {
    el = document.createElement('div');
    el.className = 'empty-state';
    document.getElementById('main').appendChild(el);
  }
  el.innerHTML = `
    <h2>Your memory palace is empty</h2>
    <p>Index some markdown files to see your memory graph:</p>
    <p style="margin-top: 8px"><code>hebbs init .</code> then <code>hebbs index .</code></p>
  `;
}

function hideEmptyState() {
  const el = document.querySelector('.empty-state');
  if (el) el.remove();
}

// ═══════════════════════════════════════════════════════════════════════
//  Init
// ═══════════════════════════════════════════════════════════════════════

async function init() {
  // Initialize graph
  const canvas = document.getElementById('graph-canvas');
  state.graph = new MemoryGraph(canvas);

  state.graph.onNodeClick = async (node) => {
    if (!node) {
      state.selectedNodeId = null;
      state.memoryDetail = null;
      renderSidePanel();
      return;
    }
    state.selectedNodeId = node.id;
    await loadMemoryDetail(node.id);
  };

  state.graph.start();

  // Close side panel button
  document.getElementById('side-panel-close').addEventListener('click', () => {
    state.selectedNodeId = null;
    state.memoryDetail = null;
    state.graph.selectNode(null);
    renderSidePanel();
  });

  // Global function for graph tab node clicks
  window._selectNode = async (id) => {
    state.selectedNodeId = id;
    state.graph.selectNode(id);
    await loadMemoryDetail(id);
  };

  // Global function for cross-tab node navigation (switch to graph and select)
  window._selectNodeTab = async (id) => {
    switchTab('graph');
    state.selectedNodeId = id;
    state.graph.selectNode(id);
    await loadMemoryDetail(id);
  };

  // Initialize detail drawers for non-graph tabs
  _drawers = {
    dashboard: new DetailDrawer('tab-dashboard'),
    explorer:  new DetailDrawer('tab-explorer'),
    recall:    new DetailDrawer('tab-recall'),
    queries:   new DetailDrawer('tab-queries'),
    timeline:  new DetailDrawer('tab-timeline'),
  };

  // Global function for in-context detail (opens drawer in current tab, or graph side panel)
  window._openDetail = async (id) => {
    const tab = state.activeTab;
    if (tab === 'graph') {
      state.selectedNodeId = id;
      state.graph.selectNode(id);
      await loadMemoryDetail(id);
      return;
    }
    const drawer = _drawers[tab];
    if (drawer) {
      await drawer.open(id);
    }
  };

  // Tab bar setup
  document.querySelectorAll('.tab-btn').forEach(btn => {
    btn.addEventListener('click', () => switchTab(btn.dataset.tab));
  });

  // Mark graph tab as loaded since it is default
  state.tabsLoaded['graph'] = true;

  // Set up graph controls
  setupGraphControls();

  // Load data
  try {
    await Promise.all([loadVaults(), loadStatus()]);
    // Re-render vault dropdown now that status is available (fallback for standalone mode)
    renderVaultDropdown();
    await loadGraph();
  } catch (err) {
    console.error('Failed to load data:', err);
    showEmptyState();
  }

  // Connect WebSocket for live events from the daemon
  connectWebSocket();
}

// ═══════════════════════════════════════════════════════════════════════
//  WebSocket: live events from daemon
// ═══════════════════════════════════════════════════════════════════════

let _wsRefreshTimer = null;

function connectWebSocket() {
  const proto = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
  const ws = new WebSocket(`${proto}//${window.location.host}/api/panel/ws`);

  ws.onmessage = (evt) => {
    let event;
    try {
      event = JSON.parse(evt.data);
    } catch {
      return;
    }

    switch (event.type) {
      case 'memory_created':
      case 'memory_forgotten':
      case 'ingest_complete':
        // Debounced graph refresh: coalesce rapid events into a single reload.
        if (_wsRefreshTimer) clearTimeout(_wsRefreshTimer);
        _wsRefreshTimer = setTimeout(async () => {
          _wsRefreshTimer = null;
          await loadGraph();
          await loadStatus();
          if (state.activeTab === 'dashboard') await loadDashboard();
          if (state.activeTab === 'notes' && state.notesActiveFile) await openNoteFile(state.notesActiveFile);
          if (state.activeTab === 'explorer') await loadExplorer();
        }, 500);
        break;

      case 'config_reloaded':
        if (state.activeTab === 'settings') {
          loadSettingsTab();
        }
        break;
    }
  };

  ws.onclose = () => {
    // Reconnect after a short delay.
    setTimeout(connectWebSocket, 2000);
  };

  ws.onerror = () => {
    ws.close();
  };
}

document.addEventListener('DOMContentLoaded', init);
