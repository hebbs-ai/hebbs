# TASK-21: End-to-End User Experience

This is what the user sees. Not what happens inside. Not our problems. Theirs.

---

## Who is this person?

They have a folder of markdown files. Meeting notes, project docs, research, decisions, journals. Maybe 20 files, maybe 2000. They heard HEBBS gives their AI agent memory across conversations. They want to try it. They have 3 minutes of patience.

---

## Moment 1: Install + First Run

```
brew install hebbs
```

That's it. No Rust toolchain. No Docker. No Python env. One command.

```
$ cd ~/work/my-project
$ hebbs init
```

No flags. No `--provider`. No `--model`. No `--api-key-env`.

HEBBS asks what it needs, once:

```
$ hebbs init

  Welcome to HEBBS.

  Found 23 markdown files in this directory.

  HEBBS needs an LLM for deep understanding of your notes.
  You can use a cloud provider or a local model via Ollama.

  [1] Ollama (local, free, private)
  [2] Gemini (fast, cheap)
  [3] Anthropic
  [4] OpenAI

  Choice: 1

  Checking Ollama... found at localhost:11434
  Available models: gemma3:1b, qwen3:4b

  Recommended: gemma3:1b (fastest for this task)
  Use gemma3:1b? [Y/n]: y

  Downloading embedding model (1.2 GB, one-time)...
  ████████████████████░░░░░  83%  1.0 GB / 1.2 GB  12.4 MB/s

  Done. Indexing your files in the background.

  ┌─────────────────────────────────────────────┐
  │  Your vault is live.                        │
  │                                             │
  │  Try:  hebbs recall "what did we decide?"   │
  │                                             │
  │  Indexing: 4/23 files ready (18%)           │
  │  Run `hebbs status` to see progress.        │
  └─────────────────────────────────────────────┘
```

**Time to first prompt: 15 seconds** (after model download).

They don't wait for indexing. They try it immediately.

---

## Moment 2: First Recall (While Still Indexing)

```
$ hebbs recall "what database are we using"

  PostgreSQL is the primary database for transactional data.
  Redis for caching and session management.
  TimescaleDB for time-series metrics.

  Source: project-decisions.md > Database

  ℹ 18% of your vault is indexed. More results may appear as indexing completes.
```

It works. Even partially indexed, it found something useful. The note is honest -- not an error, not a warning. Just information.

They try again 30 seconds later:

```
$ hebbs recall "who owns what"

  Alice Chen - Backend Lead. Owns search infrastructure and APIs.
  Bob Tanaka - Frontend Engineer. Handles React, dashboard, mobile.
  Carol Singh - DevOps. Manages EKS clusters, CI/CD pipelines.
  David Park - Product Manager. Owns roadmap and customer relationships.
  Eve Okafor - Data Scientist. Recommendation models and analytics.

  Sources: team.md, meeting-notes.md > Sprint Planning

  ℹ 64% of your vault is indexed.
```

More results now. The percentage goes up. The system is alive, working in the background.

---

## Moment 3: Indexing Complete

No fanfare. No notification they didn't ask for. Next time they run any command:

```
$ hebbs recall "on-call process"

  On-call rotation is weekly, Monday to Monday.
  P0: acknowledge within 5 minutes, resolve within 30 minutes.
  P1: acknowledge within 15 minutes.
  Blameless postmortem required within 48 hours.
  Schedule managed in PagerDuty. Carol Singh coordinates.

  Sources: engineering-handbook.md > On-Call

  ✓ Vault fully indexed (23 files, 87 memories)
```

The checkmark appears once. After that, no status line at all. Clean output.

---

## Moment 4: They Edit a File

They open `budget.md` and change the infrastructure budget from $5,000 to $8,000. Save the file. They don't run any command. They don't even think about HEBBS.

Next time they ask:

```
$ hebbs recall "infrastructure budget"

  The infrastructure budget is $8,000 per tenant per month.
  This covers compute, storage, and network costs across all environments.

  Source: budget.md > Budget
  Updated: 2 minutes ago (was $5,000, automatically revised)
```

HEBBS noticed the change, re-indexed the file, detected the revision, updated the memory. The user sees "Updated: 2 minutes ago" and knows it's current. They didn't do anything.

---

## Moment 5: Contradictions Surface Naturally

They add a new file `q2-planning.md` that says "Infrastructure budget reduced to $3,000 per tenant."

Next recall:

```
$ hebbs recall "budget"

  ⚠ Conflicting information found:

  budget.md says: $8,000 per tenant per month
  q2-planning.md says: $3,000 per tenant per month

  Both sources are current. You may want to resolve this.

  To see all contradictions: hebbs contradictions
```

Not an error. Not a crash. A gentle heads-up that something doesn't add up. The user decides what's true. HEBBS just noticed.

---

## Moment 6: Status When They Want It

```
$ hebbs status

  Vault: ~/work/my-project
  Files: 24 (all indexed)
  Memories: 91
  Last change: budget.md (3 min ago, auto-indexed)
  Contradictions: 1 unresolved

  Everything is up to date.
```

Short. Clean. Answers "is everything working?" in one glance.

During indexing, it shows more:

```
$ hebbs status

  Vault: ~/work/my-project
  Indexing: 14/24 files (58%)
  Currently processing: engineering-handbook.md

  ████████████████░░░░░░░░░  58%

  Memories created: 43
  Contradictions found: 1 (auto-resolved)
  Estimated time remaining: ~40 seconds
```

---

## Moment 7: Agent Integration (The Real Product)

The user's AI agent (Claude, Cursor, Copilot) has HEBBS as a tool. The agent uses it automatically. The user never types `hebbs recall` -- the agent does.

What the agent sees in SKILL.md:

```
hebbs recall "relevant query"    -- search memory, always fast
hebbs remember "important fact"  -- store something the user said
hebbs status                     -- check if vault is ready
```

Three commands. That's the entire API for agents. Everything else is invisible.

The agent starts a conversation:

```
User: "What's our deployment process?"

Agent: [internally calls: hebbs recall "deployment process"]
Agent: [gets results from engineering-handbook.md + project-decisions.md]

Agent: "Your deployments happen on Tuesdays and Thursdays on AWS EKS
        via GitHub Actions. Staging auto-deploys on PR merge to develop.
        Production requires manual approval. Carol manages the pipeline."
```

The user didn't mention HEBBS. The agent just knew. That's the product.

---

## Moment 8: New Files, No Ceremony

User creates a new file. Saves it. Doesn't tell HEBBS. Minutes later:

```
User: "What did the customer say about pricing?"

Agent: [hebbs recall "customer pricing feedback"]

Agent: "In the March 20 customer feedback session, two customers
        reported slow search on mobile. Three requested dark mode.
        No specific pricing feedback was captured in your notes."
```

The new file was automatically picked up. No reindex command. No restart. The system just works.

---

## What the User Never Sees

- "ONNX Runtime", "HNSW index", "RocksDB", "phase2_ingest"
- Stack traces or Rust error messages
- "daemon.sock", "daemon.pid", "daemon.log"
- Embedding dimensions or model architecture details
- "section", "proposition", "entity" (our internal concepts)
- Any mention of "background workers" or "work queue"
- Configuration files unless they go looking

---

## What the User Feels

1. **Fast.** Every command responds in under 2 seconds. Always.
2. **Alive.** The system notices file changes without being told.
3. **Honest.** When results are partial, it says so. When there's a conflict, it surfaces it.
4. **Quiet.** No unnecessary output. No notifications they didn't ask for.
5. **Trustworthy.** Sources are always shown. They can verify.
6. **Invisible.** When used through an agent, they forget it's there. Their agent just knows things.

---

## Non-Negotiable UX Rules

1. **No command should ever hang.** If work takes time, it happens in the background. The terminal returns.
2. **No jargon in user-facing output.** "Files" not "sections". "Memories" not "HNSW vectors". "Conflicts" not "contradictions with bidirectional CONTRADICTS edges".
3. **Always show sources.** Every recall result must say where it came from.
4. **Partial is better than blocked.** 30% indexed with results beats 100% indexed after 5 minutes of nothing.
5. **Errors are sentences, not stack traces.** "Could not reach Ollama at localhost:11434. Is it running?" not "ureq::Error::Transport(...)".
6. **One install command. One init command. Zero config files to edit.**
7. **The default should be right.** Model selection, batch sizes, thresholds -- we pick. They override only if they want to.

---

## Moment 9: The Panel (Their Window Into Memory)

Init already told them:

```
  Your vault is live. Panel: http://localhost:6381
```

They click it. Or `hebbs panel` opens it.

### What they see on first visit (still indexing):

A dark, calm interface. Center of screen: their knowledge graph growing in real time. Nodes appear one by one as files get indexed. Edges form between related memories. A gentle progress indicator at the top:

```
┌──────────────────────────────────────────────────────────────────┐
│  Indexing... 14/23 files  ████████████░░░░░░░░  61%             │
│  Currently: engineering-handbook.md > Deployments                │
└──────────────────────────────────────────────────────────────────┘
```

They watch their notes become a living graph. Nodes cluster by topic. The engineering docs form one cluster, meeting notes another, research a third. They can see the shape of their knowledge.

### What they see after indexing completes:

The progress bar fades. The graph settles. Top bar becomes:

```
  23 files  ·  87 memories  ·  3 insights  ·  All synced
```

### Live file changes:

They edit `budget.md` in their editor. The panel, without refresh:

- The `budget.md` node pulses briefly (amber glow)
- A toast slides in from the bottom: "budget.md updated. 1 memory revised."
- If a contradiction was found: the edge between two nodes turns red, and the toast says: "Conflict detected: budget $8K vs $3K"

They delete a file. The nodes fade out and disappear. Toast: "old-notes.md removed. 5 memories forgotten."

They add a new file. New nodes fade in, edges connect to existing memories. Toast: "api-design.md indexed. 4 new memories."

### The tabs they actually use:

**Graph** (default) -- Their knowledge as a living map. They click a node, see the memory content, source file, when it was created, what it's connected to. They don't need to understand graph theory. It's just "here's what I know, and how things connect."

**Recall** -- They type a question, see answers ranked by relevance with source files. Like a search engine for their own brain. Instant results.

**Timeline** -- "What did I learn this week?" A chronological view. They see which days they added the most knowledge, how their vault grew over time.

**Dashboard** -- One-glance health check. Files synced, contradictions to review, insights generated. Three numbers and a green checkmark when everything is current.

### What they DON'T see in the panel:

- Memory IDs or ULIDs
- Embedding dimensions or HNSW parameters
- RocksDB column families
- "Phase 1" or "Phase 2"
- JSON blobs
- Daemon status or PID files
- Config TOML

### The panel during contradictions:

They wrote conflicting information in two files. The graph shows it:

- Two nodes connected by a red edge
- Click the edge: "budget.md says $8,000/month. q2-planning.md says $3,000/month. Detected 2 minutes ago."
- Two buttons: "Keep newer" / "I'll fix it in the file"
- If they fix the file, HEBBS detects the change, resolves the contradiction, the red edge disappears. No panel interaction needed.

### The panel during reflection:

After enough memories accumulate, HEBBS generates insights. The panel shows them:

- Insight nodes are visually different (hexagon vs circle)
- They glow when new
- Click one: "Based on 12 memories about deployments, architecture, and on-call: your team operates with a strong separation between staging (automated) and production (manual approval), with Carol as the infrastructure single point of contact."
- Source memories listed below. The user can verify every claim.

### What makes them come back to the panel:

1. **It's beautiful.** The graph is not a debugging tool. It's a visualization of their knowledge that they're proud to show people.
2. **It's alive.** Things move, update, glow. It feels like a living system, not a static dashboard.
3. **It answers "what do I know?"** -- a question no other tool answers visually.
4. **It surfaces problems.** Contradictions, stale files, gaps in knowledge -- shown gently, not as errors.
5. **It's optional.** Power users live in the panel. Others never open it. Both are fine.

---

## Gap Between Today and This

| Today | Target |
|-------|--------|
| `hebbs init` takes 15 flags | `hebbs init` asks interactively, 3 questions max |
| Init blocks 5-10 minutes | Init returns in 2 seconds |
| Recall fails if not indexed | Recall works immediately, shows completeness |
| Errors show Rust types | Errors are human sentences |
| Status shows internal counters | Status shows "everything is up to date" or a progress bar |
| No mention of sources | Every result shows source file + heading |
| Contradictions require CLI commands | Contradictions surface naturally in recall results |
| Agent needs 20+ commands in SKILL.md | Agent needs 3 commands |
| Model download is silent | Progress bar with speed and ETA |
| No event stream | `hebbs subscribe` for tools that want live updates |
| Panel shows internal IDs and counters | Panel shows human-readable content with sources |
| Panel graph is static on load | Graph grows live as files are indexed, nodes pulse on changes |
| No live file change feedback in panel | Toasts for file add/edit/delete with memory counts |
| Contradictions require CLI to review | Red edges in graph, click to review, resolve from panel |
| Panel feels like a debug tool | Panel feels like a knowledge visualization you'd show someone |
| No indexing progress in panel | Top bar shows file-by-file progress during indexing |
| Panel requires manual refresh | WebSocket pushes all changes in real time |
