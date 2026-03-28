# HEBBS Enterprise: Future Tasks

Capabilities that are parked for now. Tracked here for prioritization after launch.

---

## High Priority

### Edge linking in SDK/API docs
**What:** Document typed edges (`caused_by`, `related_to`, `followed_by`, `revised_from`, `contradicts`) in the SDK and API specs. Show when/how to use them. Causal recall depends on agents creating edges when storing memories.
**Why priority:** Without edges, causal recall strategy is underutilized. Customers who need "what led to this?" queries won't get good results.
**Engine support:** Already exists. Just needs SDK/API documentation + examples.

---

## Medium Priority

### Global/shared brain across workspaces
**What:** A shared knowledge base accessible from all workspaces. Company-wide facts ("SLA is 99.9%", "founded in 2019") shouldn't be duplicated per workspace.
**Engine support:** The engine has a global brain concept (`~/.hebbs/`, `--global` flag). Needs to be exposed in the enterprise platform, SDK, and API. Each workspace recall could optionally include global results.
**Why parked:** Customers can work around this by training each workspace independently with shared docs. The global brain adds architectural complexity to the Docker deployment (shared volume between workspaces).
**When to build:** When a customer has 3+ workspaces and complains about duplicating knowledge.

### Reflection control
**What:** Let customers configure reflection schedule and trigger it manually. Currently reflection runs in the daemon background on a fixed interval. Customers have no visibility into when it runs or control over timing.
**What's needed:**
- Expose reflection interval in platform config UI
- Add manual trigger button in dashboard ("Run reflection now")
- Show last reflection time and next scheduled run in workspace status
- Expose `POST /v1/reflect` in platform API (proxy to engine's reflect endpoint)
**Engine support:** Engine has `hebbs reflect` command and reflect API. Just needs platform exposure.

### Fact extraction recipe
**What:** A concrete, copy-paste prompt template for extracting atomic facts from conversations before calling `remember()`. Current docs say "your agent should extract facts" but don't give a production-ready recipe.
**What's needed:**
- A tested prompt template that works with gpt-4o-mini / Claude
- Guidance on: how many facts per turn, importance scoring rules, when to skip extraction
- Example with real conversation → extracted facts
- Could be a skill (like the tune skill) or just SDK documentation

### Subscribe (real-time memory events)
**What:** Agents can listen for new memories in real-time via SSE/WebSocket. Useful for cross-agent knowledge propagation (when agent A stores something, agent B learns it immediately).
**Engine support:** Already exists (`POST /v1/subscribe`, SSE stream). Needs SDK/API documentation.

---

## Lower Priority

### Cross-workspace recall
**What:** A single recall call that searches across multiple workspaces. Today: each workspace is isolated, agent makes N separate calls to search N workspaces.
**Workaround:** Agent makes parallel recall calls to each workspace and merges results. Works but adds latency and complexity.
**Why parked:** Global brain (above) covers the main use case (shared knowledge). True cross-workspace search is an edge case for most enterprise customers in month 1.
**When to build:** When a customer has 5+ workspaces and builds meta-agents that span projects.

### Workspace export/import
**What:** Export a workspace's data (memories, files, config) for backup or migration. Import into another HEBBS instance.
**Engine support:** Partially — RocksDB backup exists but no clean export/import API.
