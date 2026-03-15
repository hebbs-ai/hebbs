# How the HEBBS Brain Works

## Previously: Two Brains

HEBBS had two separate binaries with two separate storage locations:

```
~/.hebbs/                              <- Brain 1 (server)
  data/                                <- RocksDB used by hebbs-server
  config.toml                          <- server config

<project>/.hebbs/                      <- Brain 2 (vault)
  index/db/                            <- Separate RocksDB used by hebbs-vault
  manifest.json                        <- file-to-memory mapping
  config.toml                          <- vault config
```

`hebbs-cli` talked to Brain 1 via gRPC. `hebbs-vault` used Brain 2 directly. A memory stored through the server was invisible to the vault. A file indexed by the vault was invisible to the CLI. Same engine code, two storage locations, two brains.

---

## Now: One Brain

One binary (`hebbs`). One RocksDB. One recall searches everything.

### Per-project brain (recommended)

```
my-app/
  src/
  docs/
  memories/                            <- agent-written memories (plain .md files)
    insights/                          <- consolidated knowledge (also .md files)
  .hebbs/                              <- THE BRAIN (gitignored, rebuildable)
    index/
      db/                              <- single RocksDB instance
    manifest.json                      <- maps files to memory IDs, tracks source (file/agent)
    config.toml                        <- vault config (embedding model, mode, etc.)
```

Every project gets its own brain. The brain lives next to the code. It indexes project docs, agent memories, and insights in one unified store.

### Global brain (fallback)

```
~/.hebbs/                              <- global brain (when no project vault exists)
  index/
    db/                                <- RocksDB
  manifest.json
  config.toml
```

Used when there is no project-level `.hebbs/` directory. Good for general-purpose memories that are not tied to any specific project.

---

## Brain Discovery

When you run any `hebbs` command, the binary finds the brain through this priority chain:

```
1. --vault flag or HEBBS_VAULT env var
   -> Use that path directly

2. Walk up from current directory looking for .hebbs/
   -> cd ~/projects/my-app/src/utils
   -> checks ~/projects/my-app/src/utils/.hebbs/  (no)
   -> checks ~/projects/my-app/src/.hebbs/         (no)
   -> checks ~/projects/my-app/.hebbs/             (yes, use this)

3. Fall back to ~/.hebbs/
   -> Global brain, always exists as last resort

4. Nothing found
   -> "No brain found. Run: hebbs init <path>"
```

This means:
- Inside a project directory, commands automatically use that project's brain
- Outside any project, commands use the global brain
- The developer never specifies a path in normal usage

### Remote mode override

If `--endpoint` or `HEBBS_ENDPOINT` is set, the brain is remote. No local `.hebbs/` is needed. The same commands talk to a server instead of a local RocksDB:

```
1. --endpoint flag or HEBBS_ENDPOINT env var
   -> Remote mode: gRPC/REST client to that endpoint
   -> Skip all local discovery

2. (otherwise fall through to local discovery above)
```

Three remote scenarios:

| Scenario | Endpoint | Use case |
|----------|----------|----------|
| Local server | `localhost:6380` | Multiple agents sharing one brain on one machine |
| Self-hosted | `https://hebbs.internal.co:6380` | Team sharing one brain on internal infra |
| HEBBS Cloud | `https://api.hebbs.ai` | Managed, synced, paid |

Config is the only difference:

```toml
# Local mode (default, no config needed)
[engine]
mode = "local"

# Remote mode
[engine]
mode = "remote"
endpoint = "https://api.hebbs.ai"
api_key = "hb_..."
```

---

## The .hebbs/ Directory

Everything inside `.hebbs/` is derived from files. It is disposable.

```
.hebbs/
  index/
    db/                    <- RocksDB: embeddings, temporal index, graph edges, memory metadata
  manifest.json            <- which files are indexed, their sections, checksums, byte offsets
  config.toml              <- embedding model, chunking strategy, mode (local/remote)
```

### manifest.json

Tracks every indexed file and its sections:

```json
{
  "files": {
    "docs/api-design.md": {
      "sections": [
        {
          "memory_id": "01JABC...",
          "heading_path": ["Pagination"],
          "source": "file",
          "byte_start": 142,
          "byte_end": 890,
          "state": "synced"
        }
      ]
    },
    "memories/2026-03-14-dark-mode.md": {
      "sections": [
        {
          "memory_id": "01JDEF...",
          "heading_path": [],
          "source": "agent",
          "byte_start": 0,
          "byte_end": 150,
          "state": "synced"
        }
      ]
    }
  }
}
```

Both file-backed and agent-stored memories live in the same manifest, same index, same RocksDB. The `source` field distinguishes them for filtering (`--source file`, `--source agent`), but recall searches everything by default.

---

## Rebuild Guarantee

Delete `.hebbs/` at any time. Run `hebbs index .` and the entire brain rebuilds from files:

```sh
rm -rf .hebbs/
hebbs init .
hebbs index .
# Full brain restored. Every memory, every preference, every insight.
```

This works because:
- Agent-stored memories are written as `.md` files in the vault (not hidden in RocksDB)
- Insights from reflection are written as `.md` files
- The engine is just an index over files. Files are the truth. The index is disposable.

New machine? Same story:

```sh
git clone git@github.com:user/my-app.git
cd my-app
hebbs init .
hebbs index .
# Full brain on a fresh machine.
```

---

## Summary

| Before | After |
|--------|-------|
| Two binaries (`hebbs-cli`, `hebbs-vault`) | One binary (`hebbs`) |
| Two RocksDB instances | One RocksDB |
| Server brain at `~/.hebbs/data/` | Gone. Merged into vault brain. |
| Vault brain at `<project>/.hebbs/index/db/` | The one brain. |
| `recall` from CLI misses vault memories | `recall` searches everything |
| `recall` from vault misses server memories | `recall` searches everything |
| Agent memories stored only in RocksDB | Agent memories written as `.md` files, then indexed |
| Rebuild destroys agent memories | Rebuild restores everything from files |
