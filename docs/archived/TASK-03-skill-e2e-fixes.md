# TASK-03: HEBBS Skill E2E Fixes

Source: Full skill E2E test session run on 2026-03-12, exercising all HEBBS CLI commands as an OpenClaw agent would.

---

## What was fixed (this session)

### Issue 3: CLI output parseability for agents

**Status:** Done
**Component:** docs (SKILL.md x2, hebbs-docs commands.mdx, output-formats.mdx)

`--format json` already worked with `remember` but wasn't documented. Agents regex-parsed human output and grabbed wrong tokens for `--edge`. Fixed by documenting `--format json` for `remember` with `jq -r '.memory_id'` extraction in all skill and docs files.

### Issue 4: Analogical recall structural similarity always 0.5

**Status:** Done
**Component:** hebbs-core (engine.rs), hebbs-cli (cli.rs, commands.rs)

Root cause: CLI never passed `cue_context` (hardcoded `None`), and `compute_structural_similarity` had two hardcoded components (`kind_match = 1.0`, `entity_pattern = 0.5`). Fixed:
- Added `--context` flag to `recall` CLI subcommand
- Implemented real `kind_match` comparison (cue_context `"kind"` key vs memory kind)
- Implemented real `entity_pattern` comparison (cue entity_id vs memory entity_id)
- Removed early-return `0.5` for empty contexts; now always computes kind + entity signals
- Added 2 new tests, updated 2 existing tests

Verified: structural_similarity=0.95 for matching context, 0.0 for non-matching. Real differentiation.

### Issue 5: Reflection LLM silent fallback

**Status:** Done (doc clarification)
**Component:** SKILL.md x2, hebbs-docs commands.mdx

Clarified that `reflect-prepare` + `reflect-commit` exists so the agent IS the LLM. No server-side LLM needed. `reflect` is for when LLM is configured on the server.

### Issue 6: Skill loses priority to native tools

**Status:** Done (doc fix)
**Component:** SKILL.md x2

Added "Your two essential commands" section at the top of SKILL.md making `remember` and `recall` prominent before any setup or trigger docs.

### Server start command syntax

**Status:** Done
**Component:** SKILL.md x2

SKILL.md said `hebbs-server --data-dir` but the binary requires `hebbs-server start --data-dir`. Fixed all occurrences.

### `--edge` zsh shell quoting

**Status:** Done
**Component:** SKILL.md x2

`$MEM_ID:edge_type` triggers zsh variable modifier expansion. Added warning to use `"${MEM_ID}:edge_type"`.

### `reflect-commit` ID format mismatch

**Status:** Done
**Component:** SKILL.md x2

Prepare output has `memories[].memory_id` (ULID) and `memory_ids[]` (hex). Commit requires hex. Clarified: use `memory_ids[]`, not `memories[].memory_id`.

### Reflect minimum memory threshold

**Status:** Done
**Component:** SKILL.md x2

`reflect-prepare` returns empty `clusters: []` silently with too few memories. Added note: reflection requires at least 5 memories to produce clusters.

---

## What remains open

### Issue 1: Storage reliability (data directory loss)

**Severity:** Critical
**Component:** hebbs-server (storage layer)
**Status:** Not started

Server keeps running after `~/.hebbs/data` is deleted. Reads return empty silently. Writes fail with I/O error only when attempted. Needs:
- Periodic data directory health check in the storage layer
- Health endpoint should detect missing/unwritable data dir
- `status` command should verify data dir exists, not just that the server responds

### Issue 2: Causal recall pulls unrelated memories

**Severity:** Medium
**Component:** hebbs-core (causal strategy)
**Status:** Not started, confirmed in E2E test

Causal recall seeded from a storage decision returned an unrelated preference memory ("Parag prefers concise responses" appeared in a storage technology causal query). The graph traversal falls back to semantic similarity when no explicit edges exist, pulling topically unrelated memories. Needs:
- Causal strategy should only traverse explicit edges, not blend in semantic similarity
- Or apply a minimum causal relevance threshold to filter noise

### `reflect-prepare` JSON control characters

**Severity:** Low
**Component:** hebbs-server (JSON serialization)
**Status:** Not started

The `proposal_system_prompt` and `proposal_user_prompt` fields in the reflect-prepare JSON output contain unescaped control characters (raw newlines etc.) that break `jq` parsing. Agents must use Python or strip control chars before parsing. The server's JSON serializer should properly escape these.

---

## Files modified

### hebbs-skill repo
- `hebbs/SKILL.md` — all doc fixes above

### hebbs repo
- `skills/hebbs/SKILL.md` — all doc fixes above (kept consistent)
- `crates/hebbs-cli/src/cli.rs` — added `--context` flag to `Recall`
- `crates/hebbs-cli/src/commands.rs` — parse context, pass to `cue_context`
- `crates/hebbs-core/src/engine.rs` — fixed `compute_structural_similarity` + 4 tests

### hebbs-docs repo
- `src/content/docs/cli/commands.mdx` — `--format json` for remember, reflect docs, `--context` on recall
- `src/content/docs/cli/output-formats.mdx` — `remember` JSON output example
