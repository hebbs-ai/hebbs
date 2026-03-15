# Testing the Memory Palace Control Panel (TASK-16)

## Setup

### 1. Init a vault

```bash
cd ~/some-folder-with-markdown-files
hebbs init .
```

Creates `.hebbs/` with `config.toml` and an empty manifest.

### 2. Index your files

```bash
hebbs index .
```

Parses all `.md` files, embeds sections, stores memories in RocksDB. You'll see output like `11 sections embedded, 11 new`.

### 3. Open the panel

```bash
hebbs panel .
```

Opens `http://127.0.0.1:6381` in your browser automatically. Ctrl+C to stop.

If port 6381 is taken: `hebbs panel . --port 6387`

---

## Quick test with a fresh vault

If you don't have markdown files handy:

```bash
mkdir /tmp/test-panel && cd /tmp/test-panel

cat > notes.md << 'EOF'
# Project Notes

## Architecture
The backend uses Rust with axum for HTTP handling.

## Frontend
React with TypeScript for the UI layer.

## Deployment
Docker containers on AWS ECS with Terraform.
EOF

hebbs init .
hebbs index .
hebbs panel .
```

---

## What to test in the browser

### Phase 1: Graph + Side Panel

| Feature | How to test |
|---------|------------|
| Graph layout | Memory nodes should appear centered. Drag to pan, scroll to zoom. Hover for tooltips. |
| Node click | Click any node -- side panel opens with content, score breakdown (recency/importance/reinforcement), edges, similar neighbors. |
| Vault switching | If you have multiple vaults (`hebbs init` in different dirs), the dropdown in the header lists them all. |
| Health badge | Bottom-right shows memory count + sync %. Click it for health detail. |

### Phase 2: Search + Controls

| Feature | How to test |
|---------|------------|
| Search | Type in the search bar. Matching nodes glow amber, non-matches fade. Latency badge shows ms. Side panel lists ranked results. |
| Controls bar | Click "CONTROLS" to expand. Drag weight sliders, toggle strategies, change top-k, pick presets -- search re-runs live. |
| Filters | Filter by state (synced/stale), file, or importance range. |
| Decay mode | Click "Show decay" -- at-risk nodes turn red. Side panel shows stale files, orphaned memories, decay candidates. |

### Phase 3: Timeline

| Feature | How to test |
|---------|------------|
| Timeline scrubber | Bottom bar. Drag scrubber left to see the vault at an earlier point in time (nodes hide/show). |
| Sparklines | Two small charts at bottom-left show daily memory and insight growth. |
| Stats | Bottom-right of timeline shows total memories and today's delta. |

### Phase 4: Config + Export

| Feature | How to test |
|---------|------------|
| Config editor | Click the gear icon (top-right). Edit chunking, watcher debounce, ignore patterns. |
| Save config | Click "Save" -- writes to `.hebbs/config.toml`. Verify with `cat .hebbs/config.toml`. |
| Reset config | Click "Reset to defaults" -- restores factory settings. |
| Export config | Click "Export TOML" -- downloads the config file. |
| PNG export | Click "Export PNG" -- downloads a 2400x1260 image of your memory graph. |
