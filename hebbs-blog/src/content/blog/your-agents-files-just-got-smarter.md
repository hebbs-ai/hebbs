---
title: "Your Agent's Files Just Got Smarter"
description: "HEBBS now sits next to your markdown files as an invisible intelligence layer. No migration, no database imports. Your files stay yours. HEBBS makes them searchable, connected, and self-organizing."
pubDate: 2026-03-14
tags: ["engineering", "vault", "markdown", "agents"]
heroImage: "/images/your-agents-files-just-got-smarter.png"
ogImage: "/images/your-agents-files-just-got-smarter.png"
---

Your agent has a knowledge base. It is a folder full of markdown files: meeting notes, design docs, research, decisions, onboarding guides. Maybe hundreds. Maybe thousands. The agent can read any one of them if you point it to the right path. But it cannot answer "what do we know about vendor X across all our notes?" without reading every file, every time.

That changes today.

HEBBS now operates as an invisible cognitive layer over directories of markdown files. No database migration. No proprietary format. No import step that copies your content into a black box. Your files stay exactly where they are, in plain markdown, editable, git-tracked, human-readable. HEBBS creates a parallel index (`.hebbs/`) that makes them searchable by meaning, connected by relationships, and self-organizing over time.

Delete `.hebbs/` and you lose nothing. Your files are untouched. Run `hebbs index` and everything comes back.

## The Architecture: Two Planes

The design borrows from how git works. Git does not own your source code. It maintains a parallel data structure (`.git/`) that tracks changes, history, and branches. Delete `.git/` and your code is still there.

HEBBS works the same way:

```
vault/                              ← CONTENT PLANE (source of truth)
  notes/
    meeting-jan.md
    api-design.md
  insights/                         ← engine-generated, still just files
    01JABC-vendor-pattern.md
  .hebbs/                           ← COGNITION PLANE (rebuildable)
    manifest.json
    config.toml
    index/
```

**Content plane**: your markdown files. Always the truth. Always human-readable.

**Cognition plane**: the `.hebbs/` directory. Embeddings, graph edges, temporal indexes, and a manifest that maps files to memory IDs. Entirely derived from the content plane. Rebuildable from scratch.

For an agent, this means one critical property: **the knowledge base is portable and disposable at the index level.** Ship the files anywhere. Rebuild the index on any machine. The intelligence layer is cheap to recreate because the files contain everything that matters.

## How an Agent Uses This

Four commands. That is the entire surface area.

### Initialize a vault

```bash
hebbs-vault init --vault /path/to/knowledge-base
```

Creates `.hebbs/` with a default config. If the directory is a git repo, `.hebbs/` is automatically added to `.gitignore`.

### Index all files

```bash
hebbs-vault index --vault /path/to/knowledge-base
```

Two-phase pipeline. Phase 1 parses every `.md` file into heading-level sections, extracts frontmatter, wiki-links, and tags, and writes the manifest. Phase 2 embeds all sections and pushes them into the HNSW index. For 500 files, phase 1 takes under 2 seconds. Phase 2 depends on section count and embedding model, but it is batched (not one call per section) and runs in the background.

After indexing, every `##` heading in every file becomes an independently retrievable memory. A file with 5 sections becomes 5 memories, each with its own embedding, importance score, and graph edges. Sub-file granularity means your agent can recall a specific section from a 2,000-line design doc without loading the entire file.

### Watch for changes

```bash
hebbs-vault watch --vault /path/to/knowledge-base
```

A daemon that bridges the two planes in real time. When a file changes, the watcher:

1. Debounces (500ms after the last event, so rapid saves during editing do not trigger 10 re-indexes)
2. Runs phase 1: re-parses the changed file, diffs sections against the manifest, marks modified sections as content-stale
3. Waits for quiet (3 seconds with no new changes), then runs phase 2: batch-embeds all stale sections

Between phase 1 and phase 2, the system is in a content-stale state. This is intentional. Queries during this window return the current file content (read from the file at query time, not from the stored embedding). The ranking might be slightly off because the embedding is from the previous version, but the content is always fresh. The ranking self-corrects when phase 2 completes.

For agents writing files in bursts (creating 30 notes in 3 seconds), the watcher detects the burst, extends the phase 2 debounce to 10 seconds, and processes everything in a single batch. One embed call for 30 files, not 30 individual calls.

### Query the vault

```bash
hebbs-vault recall --vault /path/to/knowledge-base \
  -q "vendor evaluation performance" -k 5
```

Same recall strategies as the core engine: similarity, temporal, causal, analogical. Same composite scoring: relevance, recency, importance, reinforcement. The difference is that content is read from the actual file at query time, not from the engine's stored copy. If you edit a file and query before re-indexing, you get the edited content. Always fresh.

Results include file paths and heading paths, so the agent can cite sources:

```
[0.87] notes/meeting-jan.md > Vendor Evaluation
[0.69] notes/q3-review.md > Vendor Performance
[0.61] projects/hebbs-architecture.md > Overview
```

## Wiki-Links Become Graph Edges

If your files contain `[[wiki-links]]`, HEBBS parses them and creates `RELATED_TO` edges between the corresponding memories. A note that links to `[[api-design]]` gets a graph edge to every section in `api-design.md`. Section-level links work too: `[[api-design#authentication]]` creates an edge to that specific section.

This means causal recall works across files. An agent can ask "what is related to the API authentication decision?" and get results that traverse the wiki-link graph, surfacing connected notes the agent never explicitly queried for.

Bidirectional links (file A links to file B, file B links to file A) create bidirectional edges. The graph builds itself from your existing link structure.

## Insights as Files

When the reflect pipeline generates insights, they are written as markdown files into the vault:

```markdown
---
hebbs-kind: insight
hebbs-sources:
  - notes/meeting-jan.md#vendor-evaluation
  - notes/q3-review.md#vendor-performance
hebbs-confidence: 0.82
hebbs-created: 2026-03-14T20:00:00Z
---

The initial positive vendor assessment was based on a single
project. By Q3, three missed deadlines revealed a pattern.
```

The insight is a file. The watcher picks it up, indexes it, and it participates in future recalls, primes, and reflects. The agent can read it. A human can read it. Either can edit it or delete it. The `hebbs-sources` field uses human-readable file paths with section anchors, not opaque memory IDs.

This closes the loop: files produce memories, memories produce insights, insights become files, files produce memories. The system compounds knowledge without any step requiring human intervention.

## The Rebuild Guarantee

`hebbs-vault rebuild` deletes `.hebbs/` entirely and recreates it from scratch. Same files, same sections, same embeddings, same graph edges. The only things lost are decay scores, access counts, and reinforcement signals, all of which are cognition-plane-only state that rebuilds naturally over time through usage.

This is the acid test of the architecture. If rebuild produces a functionally equivalent index, the files are truly the source of truth. The engine is disposable. The files are permanent.

For agents, this means you can version-control the knowledge base (just the files, not `.hebbs/`), share it across machines, fork it for different contexts, and rebuild the index wherever you need it. The intelligence layer is a function of the content, not a separate artifact that needs to be maintained.

## What This Means for Agent Memory

Before this, HEBBS said "give me your data, I will make it smart." Now it says "keep your data, I will sit next to it and make it smart."

An agent can:

- **Index a project's docs folder** and instantly recall any concept across hundreds of files
- **Watch the folder** and stay current as files change, without re-indexing manually
- **Write insights back** as markdown files that humans can review, edit, or reject
- **Rebuild the index** on any machine from the same set of files
- **Cite sources** with real file paths, not database IDs
- **Use wiki-links** as a relationship graph without any additional configuration

The files were always the knowledge base. Now they are a smart knowledge base.

## Getting Started

```bash
# Build from source
cd hebbs && cargo build -p hebbs-vault --release

# Initialize and index
./target/release/hebbs-vault init --vault ~/notes
./target/release/hebbs-vault index --vault ~/notes

# Start watching
./target/release/hebbs-vault watch --vault ~/notes

# Query
./target/release/hebbs-vault recall --vault ~/notes \
  -q "what do we know about deployment" -k 5
```

The vault is ready. Your agent's files just got smarter.
