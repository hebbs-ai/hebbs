# TASK-25: Panel Live Status and First-Run Experience

The Memory Palace panel should never show an empty or confusing state. Today, opening the panel during or before indexing shows an empty graph with 0 memories. The user has no idea what's happening, whether the daemon is working, or what to do next.

---

## Problem

1. **First open after init**: Panel shows empty graph, 0 memories. No indication that indexing hasn't happened yet or is in progress.
2. **During indexing**: No live progress. User sees stale section counts and 0% sync with no explanation.
3. **Wrong vault on open**: `hebbs panel` from a project directory doesn't auto-select that vault. Falls back to the first vault the daemon opened (often the global brain).
4. **No activity visibility**: The daemon is doing work (watching files, running decay, detecting contradictions, generating insights) but none of this is visible to the user.

## Goal

The panel is the single place a user goes to understand what HEBBS is doing. It should show live status, indexing progress, and a clear first-run experience.

---

## Design

### Status Bar (always visible, top of panel)

```
Daemon: running    Vault: sales-workspace [v]    Files: 15    Memories: 237    LLM: openai/gpt-4o-mini
```

- Daemon health indicator (green dot = running, red = stopped)
- Active vault name with dropdown switcher
- File count, memory count, LLM provider -- live updated
- During indexing: "Indexing: 8/15 files (53%)" replaces the static counts

### First-Run State (before any indexing)

When the panel opens and the vault has 0 indexed memories:

- Show a centered message: "Your vault has 15 files ready to index."
- Action button or instruction: "Run `hebbs index .` to build your brain."
- Or if we implement auto-index: show live progress of the daemon indexing

Do NOT show an empty graph. Empty graphs are confusing.

### Indexing Progress View

When indexing is in progress (either from `hebbs index` or daemon auto-index):

- Show a file list with status indicators:
  - Pending (gray)
  - Parsing (yellow, spinner)
  - Extracting propositions (blue)
  - Done (green checkmark)
- Memory count ticking up in real-time as sections are embedded
- Current file being processed: "Extracting: clients/acme-corp.md"
- Progress bar: "Phase 2/2: 8/15 files complete"

When indexing completes, auto-transition to the brain visualization.

### Activity Log (new tab or sidebar)

A reverse-chronological feed of daemon events:

```
[14:32:05] Indexed clients/acme-corp.md (4 sections, 12 propositions)
[14:32:08] Indexed meetings/2026-03-12-acme-technical-review.md (5 sections, 8 propositions)
[14:32:15] Contradiction detected: budget $80K vs $60K (GreenLeaf)
[14:33:01] Insight generated: "Budget reductions are common in late-stage deals..."
[14:35:00] Decay sweep: 2 memories below threshold
[15:01:00] File changed: pipeline/q1-2026-pipeline.md (re-indexed)
```

Source: the daemon already logs these events. The panel needs a WebSocket or SSE connection to stream them.

### Vault Auto-Selection

`hebbs panel` should use the same vault discovery logic as other commands:
1. Explicit `--vault` flag
2. Walk up from CWD to find `.hebbs/`
3. Fall back to global brain

Pass the resolved vault path to the daemon so the panel opens with the correct vault active from frame one.

---

## Implementation Notes

### Data flow for live progress

The daemon already tracks indexing state in `IndexingSnapshot` (mod.rs:60). The panel needs an endpoint to read it:

```
GET /api/panel/indexing -> { "in_progress": true, "phase": 2, "files_done": 8, "total_files": 15, "current_file": "clients/acme-corp.md" }
```

Or better: a WebSocket/SSE stream that pushes progress events to the panel in real-time.

### Panel event system

The daemon already has `broadcast::Sender<PanelEvent>`. Extend it with:
- `IndexingStarted { total_files }`
- `IndexingFileComplete { file, sections, propositions }`
- `IndexingComplete { total_memories }`
- `ContradictionDetected { memory_a, memory_b, score }`
- `InsightGenerated { content, confidence }`
- `DecaySweep { forgotten_count }`
- `FileChanged { path }`

### Vault auto-selection fix

In `hebbs panel` command handler (hebbs.rs:2068), when `vault_path` is None, fall back to `resolve_vault_path(None, cli.vault.as_ref(), cli.global)` to discover the CWD vault.

---

## Scope

### Files Affected
- `hebbs-vault/src/panel/routes.rs` -- new indexing status endpoint, activity log endpoint
- `hebbs-vault/src/panel/static/app.js` -- first-run view, indexing progress UI, activity log tab
- `hebbs-vault/src/panel/static/panel.css` -- styling for new states
- `hebbs-vault/src/panel/mod.rs` -- extend PanelEvent enum
- `hebbs-vault/src/daemon/mod.rs` -- emit new event types during indexing/decay/contradiction
- `hebbs-vault/src/bin/hebbs.rs` -- fix vault auto-selection in panel command

### Dependencies
- None. This is self-contained panel work.

## Acceptance Criteria

1. Opening panel on a fresh vault (0 memories) shows a clear first-run message, not an empty graph.
2. During indexing, panel shows live progress (file being processed, percentage, memory count increasing).
3. `hebbs panel` from a project directory auto-selects that project's vault.
4. Activity log shows recent daemon events (indexing, contradictions, insights, decay).
5. When indexing completes, panel auto-transitions to the brain visualization.
