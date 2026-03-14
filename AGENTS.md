# AGENTS.md -- Instructions for AI Coding Agents

This file governs how AI agents (Cursor, Codex, Copilot, or any other coding assistant) must operate across the HEBBS workspace.

---

## Workspace Structure

```
hebbs-repos/
  AGENTS.md                    ← you are here (workspace-level agent instructions)
  hebbs/                       ← core engine (public repo, Rust workspace)
    Cargo.toml                    workspace root
    crates/                       12 Rust crates (storage, index, embed, core, reflect, server, etc.)
    proto/                        protobuf service definitions
    hebbs-demo/                   reference app: AI Sales Intelligence Agent (Python)
    benches/                      benchmark baselines
    tests/                        integration tests
    LICENSE                       BSL 1.1
  docs/                        ← project documentation (private repo)
    DocsSummary.md                index of all documentation
    GuidingPrinciples.md          12 engineering principles
    ...
  hebbs-typescript/            ← TypeScript SDK (public repo, Node.js gRPC client)
  hebbs-website/               ← landing page (public repo, Astro + Tailwind)
  hebbs-blog/                  ← blog (public repo, Astro + Tailwind, GitHub Pages)
  legal/                       ← IP ownership templates (never committed publicly)
```

---

## Engineering Standard

**HEBBS is built to the standard of government-grade, security-audited infrastructure.** Every line of code you produce will be scrutinized by security auditors, performance engineers, and formal reviewers who examine every allocation, every branch, every data flow, and every failure mode.

**You must code as the world's best systems engineer -- obsessive about latency, paranoid about correctness, and relentless about algorithmic efficiency.**

This means:
- Know the time complexity of every operation you write. Document it. `O(log n)` lookup, `O(k)` traversal with bounded `k` -- be explicit.
- Know the cost of every allocation. Pre-allocate buffers. Reuse vectors. Use arena allocators for batch operations. Profile allocation counts, not just wall-clock time.
- Choose the provably optimal data structure for each access pattern. Do not default to `Vec` or `HashMap` without analyzing the workload.
- No `unsafe` without a written safety invariant. No `unwrap()` on external input paths. No dead code. No TODO comments in merged code.
- Every input is hostile. Validate at the boundary before it touches engine internals.
- Crash safety at every point. If `kill -9` hits between any two instructions, the database recovers to a consistent state.
- Latency budgets in the guiding principles are hard contracts. `recall` similarity at 10ms p99 means 10ms under load, under contention, under compaction, at 10M memories.

The full engineering standard is documented in `docs/GuidingPrinciples.md`.

---

## Before Any Work

1. **Read `docs/GuidingPrinciples.md`.** Every code change, architecture decision, and design trade-off must comply with the 12 engineering principles. The priority ordering determines which principle wins when two conflict.

2. **Check dependency health.** Run `cargo audit` and review `Cargo.lock` for any crates that are unmaintained, yanked, pre-release (e.g., `-rc`, `-alpha`, `-beta`), or have known advisories. If you find any, **stop and report them to the user before starting other work.** Include the crate name, current version, the issue (unmaintained, pre-release, advisory), and a recommended replacement if one exists.

---

## Writing Style

- **Never use em-dashes (—) or double dashes (--)** in any written content: blog posts, documentation, commit messages, comments, or user-facing text. Use alternatives instead: commas, colons, semicolons, parentheses, or separate sentences.
- **Every blog post must have a generated hero image.** Create the image using HTML/CSS (not AI image generators), render it to a static image using Puppeteer (`node render script` in `/tmp`), and save it to the blog's `public/images/` directory. The image must include the HEBBS logo (amber circles on dark, from `public/logo-icon.svg`). Use clean, minimal designs (1200x630, 2x DPI) with the post title and relevant visual elements. Reference the image in the post's frontmatter as both `heroImage` and `ogImage` for social meta tags.

---

## Documentation Rules

Documentation lives in the separate `docs/` repo. When making code changes in `hebbs/` that affect architecture, API surface, data structures, configuration, or benchmarks:

- **Flag which docs need updating.** Tell the user which documents in `docs/` are affected so they can update them.
- **Never let docs drift from code.** If the code says one thing and the docs say another, both are wrong until reconciled.
- **Inline code documentation is mandatory.** Rustdoc on all public types and functions. README files in each crate explaining purpose and usage.

---

## Code Standards

### Guiding Principles Compliance

Every code change must comply with the guiding principles. The most commonly relevant during development:

- **Principle 1 (Hot Path Sanctity):** No network calls, no unbounded computation, no locks on the read path. Check latency budgets.
- **Principle 4 (Bounded Everything):** Every buffer, traversal, and collection must have a configurable upper bound.
- **Principle 6 (Lineage):** Insights must track source memories. Revisions must track predecessors.
- **Principle 9 (Measure Everything):** New operations must emit latency histograms and resource gauges.
- **Principle 11 (Correctness):** Multi-index updates must be atomic (RocksDB WriteBatch). Design interfaces for async and zero-copy.
- **Principle 12 (Security):** Validate inputs at boundaries. Tenant isolation is structural.

### Rust Conventions

- All I/O-bound operations are `async`.
- Error handling uses `thiserror` for library crates, `anyhow` for binary crates. No `unwrap()` or `expect()` on paths reachable by external input. Panics in production are audit failures.
- No `unsafe` blocks without a written safety invariant comment. Every `unsafe` block must document: why it is needed, what invariants must hold, and what happens if they are violated.
- Public APIs accept `impl AsRef<str>` over `String`, return references over owned values where lifetime permits.
- Every search, sort, and traversal has documented time complexity. `O(log n)`, `O(k * ef_search)`, `O(d * branching_factor)` -- be explicit in code comments.
- Pre-allocate buffers and reuse vectors on hot paths. No heap allocations in tight loops. Profile with `dhat` or allocation counters.
- Tests live next to the code they test (`#[cfg(test)] mod tests`). Integration tests live in `tests/`.
- Property-based tests (via `proptest`) for serialization round-trips, index consistency, and invariant verification.
- Benchmarks use Criterion and live in `benches/`. Every hot-path function has a corresponding benchmark. Regressions > 10% block merge.
- Use `zeroize` crate for sensitive data paths (credentials, API keys, compliance-sensitive memory content).

### Cross-Component Consistency

Every change must propagate across all affected layers: proto → server (gRPC + REST) → CLI → Rust client SDK → Python SDK → TypeScript SDK → FFI/PyO3 → hebbs-docs. Never change one layer in isolation. See `.cursor/rules/cross-component-consistency.mdc` for the full component map and propagation checklist.

### Commits

- **NEVER add Claude, Cursor, Copilot, or any AI agent as author or co-author of git commits. No `Co-Authored-By` lines. No AI attribution in commits. EVER.**
- **Commit messages MUST be a single line. No multi-line bodies, no blank lines, no paragraphs.**
- **Do not run `git commit` or `git push`.** Only generate a single-line commit message for the user to run themselves.
- Use conventional commit tags: `feat:`, `fix:`, `chore:`, `refactor:`, `docs:`, `test:`, `perf:`, `ci:`.
- Format: `<tag>(<scope>): <concise description>` — e.g. `fix(server): return 404 instead of 500 for missing memory`.
- If a code change requires a doc update, mention it in the commit message scope or body.

### Subtree Push — NON-NEGOTIABLE

**Every time you push to `origin main`, you MUST check ALL subtrees for unpushed changes and report them.** This is mandatory, no exceptions.

Run the subtree diff check script from `MANAGING_GIT.md` after every push to origin:

```bash
for p in hebbs hebbs-typescript hebbs-python hebbs-website hebbs-blog hebbs-docs hebbs-deploy hebbs-skill homebrew-tap; do
  t="last-push/$p"
  if git rev-parse "$t" >/dev/null 2>&1; then
    s=$(git diff --shortstat "$t" -- "$p/")
    if [ -n "$s" ]; then
      echo "CHANGED  $p  $s"
    else
      echo "clean    $p"
    fi
  else
    echo "NO TAG   $p"
  fi
done
```

- Report all changed subtrees to the user.
- Ask the user which subtrees should be pushed upstream.
- Push only the ones the user confirms.
- Update `last-push/<prefix>` tags after each successful subtree push.

---

## Debug Mode (Evidence-Driven Debugging)

### When to Use

Use debug mode **only when the user explicitly asks** (e.g., "debug this", "use debug mode", "can you debug why X fails"). This is the right approach when:

- The root cause is **not obvious from reading code alone** (the bug spans multiple components, involves runtime state, timing, or data flow across boundaries).
- A straightforward fix attempt already failed, or the symptom is far from the cause.
- The system behaves differently from what the code appears to do (stale state, wrong config propagated, silent filtering, race conditions).

**Do not use debug mode for trivial fixes** where reading the code makes the problem and solution immediately clear. In those cases, just fix it directly.

### Protocol

1. **Understand the bug.** Read logs, error output, and relevant source code to build context. Trace the data flow end-to-end through the component map.

2. **Generate 3–5 hypotheses.** Each must be specific, testable, and describe a concrete mechanism (e.g., "empty `kind_filter` from proto maps to `memory_kinds: []`, which causes `.contains()` to reject every memory in the scope scan"). Cast a wide net — the real cause is often not the most obvious one. Rank by likelihood.

3. **Instrument code with debug logs.** Add temporary log statements at decision points so a single reproduction run can confirm or reject **all** hypotheses in parallel.
   - **Where to log:** function entry with params, function exit with return value, values before/after critical operations, which branch was taken, suspected edge-case values.
   - **Log format:** Append one NDJSON line per log to a temporary file (e.g., `/tmp/hebbs-debug.log`).
     - **Rust / Python / Go:** Use standard file I/O (`OpenOptions::append`, `open(..., 'a')`).
     - **JavaScript / TypeScript:** Use `fs.appendFileSync` (Node.js) or `fetch()` POST to a local endpoint.
   - **Log payload:** Each entry must include `location` (file:line), `message`, `data` (structured key-value), and `hypothesisId` (which hypothesis this log tests).
   - **Hygiene:** Wrap every debug log in a collapsible region (`// #region debug` / `// #endregion`). Minimum logs to test all hypotheses (typically 3–8). **Never log secrets.**

4. **Reproduce the bug.** For compiled languages: rebuild the binary and restart the service (a stale binary is the #1 reason for "missing" logs). Run the failing test or repro steps. Collect the log file.

5. **Analyze logs — evaluate every hypothesis.**
   - For each hypothesis, cite specific log lines as evidence.
   - Verdict per hypothesis: **CONFIRMED** (log proves it), **REJECTED** (log disproves it), or **INCONCLUSIVE** (need more instrumentation).
   - If all are rejected: generate new hypotheses targeting different subsystems, add more instrumentation, and re-run.

6. **Fix with 100% confidence.** Only apply a fix when log evidence points to a confirmed root cause. Keep all instrumentation in place — do not remove it yet.

7. **Verify the fix.** Re-run with instrumentation still active. Compare before/after logs with cited entries to prove the fix works. A fix is not proven until the post-fix log output demonstrates the corrected behavior.

8. **Clean up.** Only after the post-fix verification run succeeds: remove all debug instrumentation and delete the temporary log file.

### Rules

- **No fix without runtime evidence.** Code-only reasoning produces speculative fixes that often fail or mask the real issue.
- **No removing instrumentation before verification.** Logs stay active through the post-fix run.
- **Revert rejected hypotheses.** If logs disprove a hypothesis, immediately remove any code changes introduced for it. Never accumulate speculative guards or defensive checks from discarded theories.
- **Iteration is expected.** First-attempt fixes frequently fail. More data and more iterations yield more precise fixes.
- **Rebuild and restart.** Always rebuild and restart for compiled languages before every reproduction run.

---

## What Not to Do

- **Do not add operations beyond the 9 defined in the problem statement** without explicit discussion.
- **Do not introduce external database dependencies.** HEBBS is a single binary with embedded RocksDB.
- **Do not put LLM calls on the hot path.** LLM calls belong exclusively in the reflect pipeline (background).
- **Do not skip documentation.** A code change without corresponding doc flags is incomplete.
- **Do not optimize without benchmarks.** Write the correct implementation first, add Criterion benchmarks, then optimize with profiling data.
- **Do not add Rust crate dependencies without justification.** Dependency count is minimized. `cargo audit` must pass.
- **Do not use `unwrap()` or `expect()` on any path reachable by external input.** Use `Result` + `?` with descriptive error context.
- **Do not use `unsafe` without a written safety invariant** documenting why it is needed and what must hold.
- **Do not introduce O(n^2) or worse complexity** anywhere in the codebase. If you find yourself writing nested loops over unbounded collections, redesign.
- **Do not leave dead code, commented-out code, or TODO comments** in merged code.
- **Do not merge without Criterion benchmark results** for any change touching hot-path code. A > 10% p99 regression blocks the merge.
- **Do not commit secrets, API keys, or credentials.** Use environment variables or config files excluded by `.gitignore`.
