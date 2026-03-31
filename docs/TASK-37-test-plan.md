# TASK-37: Test Plan — Auto Entity ID from Folder Convention

**Run manually. Not for CI/GitHub Actions.**

## Setup

The test script creates a temp workspace, indexes it, and verifies entity_id assignment via `hebbs recall` and `hebbs get`.

## Test Cases

### Group 1: Folder Convention

| # | File Path | Expected entity_id | Why |
|---|-----------|-------------------|-----|
| 1 | `entities/acme-corp/call-notes.md` | `acme-corp` | Basic folder convention |
| 2 | `entities/acme-corp/emails/sarah.md` | `acme-corp` | Nested subfolder, still second segment |
| 3 | `entities/acme-corp/deep/nested/file.md` | `acme-corp` | Deeply nested, still second segment |
| 4 | `entities/initech/discovery.md` | `initech` | Second entity, independent scoping |
| 5 | `entities/Globex-Corp/notes.md` | `globex-corp` | Casing normalized to lowercase |

### Group 2: Frontmatter Override

| # | File Path | Frontmatter | Expected entity_id | Why |
|---|-----------|-------------|-------------------|-----|
| 6 | `entities/acme-corp/special.md` | `entity_id: big-deal-q2` | `big-deal-q2` | Frontmatter overrides folder |
| 7 | `blogs/initech-migration.md` | `entity_id: initech` | `initech` | Frontmatter on file outside entities/ |
| 8 | `docs/random.md` | `entity_id: secret-project` | `secret-project` | Frontmatter works anywhere |

### Group 3: No Entity (Shared Knowledge)

| # | File Path | Frontmatter | Expected entity_id | Why |
|---|-----------|-------------|-------------------|-----|
| 9 | `products/features.md` | none | none (or LLM-extracted) | Outside entities/, no frontmatter = shared |
| 10 | `blogs/soc2-compliance.md` | none | none (or LLM-extracted) | Shared knowledge |
| 11 | `training/objection-handling.md` | none | none (or LLM-extracted) | Shared knowledge |

### Group 4: Edge Cases

| # | File Path | Expected entity_id | Why |
|---|-----------|-------------------|-----|
| 12 | `entities/file-at-root.md` | none | No second segment — not inside an entity subfolder |
| 13 | `not-entities/acme-corp/notes.md` | none | Folder not named `entities` |
| 14 | `docs/entities/acme-corp/notes.md` | none | `entities` not at workspace root |
| 15 | `entities//empty.md` | none | Empty entity name |
| 16 | `entities/has spaces/notes.md` | `has-spaces` (slugified) | Spaces in folder name |
| 17 | `entities/UPPER-CASE/notes.md` | `upper-case` | Uppercase normalized |

### Group 5: Recall & Prime Verification

| # | Test | Expected |
|---|------|----------|
| 18 | `hebbs recall "budget" --entity-id acme-corp` | Returns only acme-corp memories mentioning budget |
| 19 | `hebbs recall "budget"` (no entity filter) | Returns matches from ALL entities + shared |
| 20 | `hebbs prime --entity-id acme-corp` | Returns acme-corp temporal history + similar shared knowledge |
| 21 | `hebbs prime --entity-id initech` | Returns initech memories only, not acme-corp |
| 22 | `hebbs recall "call notes" --strategy temporal --entity-id acme-corp` | Returns acme-corp calls in chronological order |
| 23 | `hebbs recall "what happened with initech" --strategy temporal --entity-id initech` | Returns initech memories only |

### Group 6: Contradiction & Reflect Scoping

| # | Test | Expected |
|---|------|----------|
| 24 | Acme call-1 says "budget $100K", call-2 says "budget $50K" | Contradiction detected within acme-corp entity |
| 25 | `hebbs reflect --entity-id acme-corp` | Consolidates only acme-corp memories into insights |
| 26 | `hebbs insights --entity-id acme-corp` | Returns insights scoped to acme-corp |

## Test Script

```bash
#!/usr/bin/env bash
# TASK-37 manual test — do NOT add to CI
# Usage: bash test-task-37.sh
# Requires: hebbs binary in PATH, OPENAI_API_KEY set

set -euo pipefail

WORK=$(mktemp -d)
echo "=== Workspace: $WORK ==="

# ── Build test fixtures ──

# Group 1: Folder convention
mkdir -p "$WORK/entities/acme-corp/emails"
mkdir -p "$WORK/entities/acme-corp/deep/nested"
mkdir -p "$WORK/entities/initech"
mkdir -p "$WORK/entities/Globex-Corp"

cat > "$WORK/entities/acme-corp/call-2026-03-15.md" << 'EOF'
# Call Notes — March 15, 2026

Spoke with Sarah Chen (VP Engineering). Key points:
- Budget approved at $100K for annual contract
- Need SOC2 certification before procurement can sign off
- Timeline: aiming for Q2 close
- Competitor: also evaluating Vendor X
- Next step: send security questionnaire by Friday
EOF

cat > "$WORK/entities/acme-corp/call-2026-03-22.md" << 'EOF'
# Call Notes — March 22, 2026

Follow-up with Sarah and Mike (CFO). Key points:
- Budget revised down to $50K — Mike pushed back on original scope
- Sarah still champion but needs internal business case
- SOC2 review in progress, expect 2 weeks
- Timeline slipped to Q3
- Sent them our Initech case study as reference
EOF

cat > "$WORK/entities/acme-corp/emails/sarah-thread.md" << 'EOF'
# Email Thread: Sarah Chen — SOC2 Questions

Sarah asked for our SOC2 Type II report.
Sent report on March 18. She confirmed receipt.
Forwarded to their security team for review.
EOF

cat > "$WORK/entities/acme-corp/deep/nested/file.md" << 'EOF'
Internal note: Acme Corp deal is strategic — first enterprise customer in fintech vertical.
EOF

cat > "$WORK/entities/initech/discovery.md" << 'EOF'
# Discovery Call — Initech

Spoke with Bob Slydell. They're migrating off legacy CRM.
Team size: 30 reps. Current tool: Salesforce.
Pain: reps spend 2 hours/day on data entry.
Budget: $200K approved for tooling refresh.
Decision timeline: Q3 2026.
EOF

cat > "$WORK/entities/Globex-Corp/notes.md" << 'EOF'
# Globex Corp — Inbound Lead

Came through website demo request.
Company: 500 employees, Series C.
Vertical: healthcare SaaS.
Contact: Hank Scorpio, VP Sales.
EOF

# Group 2: Frontmatter override
cat > "$WORK/entities/acme-corp/special.md" << 'EOF'
---
entity_id: big-deal-q2
---
# Special override file
This file is in acme-corp folder but should be scoped to big-deal-q2 entity.
EOF

mkdir -p "$WORK/blogs"
cat > "$WORK/blogs/initech-migration.md" << 'EOF'
---
entity_id: initech
---
# How Initech Cut CRM Data Entry by 80%

Case study about Initech's migration from Salesforce to our platform.
30 reps, 2 hours/day saved per rep. ROI achieved in 6 weeks.
EOF

mkdir -p "$WORK/docs"
cat > "$WORK/docs/random.md" << 'EOF'
---
entity_id: secret-project
---
# Secret Project Notes
Internal planning document for secret project.
EOF

# Group 3: Shared knowledge (no entity)
mkdir -p "$WORK/products"
mkdir -p "$WORK/training"

cat > "$WORK/products/features.md" << 'EOF'
# Product Features

- Temporal recall: reconstruct what happened in order
- Causal chains: understand why outcomes occurred
- Analogical transfer: apply patterns across domains
- Automatic consolidation: episodes become insights
- Sub-10ms retrieval at 100M memories
EOF

cat > "$WORK/blogs/soc2-compliance.md" << 'EOF'
# Why SOC2 Compliance Matters for AI Infrastructure

Enterprise buyers require SOC2 Type II before procurement.
On-prem deployment eliminates data residency concerns.
Single binary means smaller attack surface for security review.
EOF

cat > "$WORK/training/objection-handling.md" << 'EOF'
# Objection Handling Playbook

## "Too expensive"
- Reframe as cost per rep per day
- Compare to hours wasted on manual CRM entry
- Reference Initech case study: ROI in 6 weeks

## "We already have Salesforce"
- Position as augmentation, not replacement
- Memory layer sits alongside existing CRM
- Zero migration risk
EOF

# Group 4: Edge cases
cat > "$WORK/entities/file-at-root.md" << 'EOF'
This file is directly in entities/ with no subfolder. Should NOT get entity_id.
EOF

mkdir -p "$WORK/not-entities/acme-corp"
cat > "$WORK/not-entities/acme-corp/notes.md" << 'EOF'
This is NOT in the entities/ folder. Should not auto-tag.
EOF

mkdir -p "$WORK/docs/entities/acme-corp"
cat > "$WORK/docs/entities/acme-corp/notes.md" << 'EOF'
Entities folder is not at workspace root. Should not auto-tag.
EOF

echo ""
echo "=== Fixture created. File count: ==="
find "$WORK" -name "*.md" | wc -l
echo ""
echo "=== File tree ==="
find "$WORK" -name "*.md" | sort | sed "s|$WORK/||"

echo ""
echo "=== Initializing hebbs ==="
cd "$WORK"
hebbs init --provider openai --key "$OPENAI_API_KEY"

echo ""
echo "=== Indexing ==="
hebbs index

echo ""
echo "=== TESTS ==="
PASS=0
FAIL=0

run_test() {
    local num="$1"
    local desc="$2"
    local cmd="$3"
    local expect="$4"

    echo ""
    echo "--- Test $num: $desc ---"
    echo "  Command: $cmd"
    result=$(eval "$cmd" 2>&1) || true

    if echo "$result" | grep -qi "$expect"; then
        echo "  PASS (found: $expect)"
        PASS=$((PASS + 1))
    else
        echo "  FAIL (expected: $expect)"
        echo "  Got: $(echo "$result" | head -5)"
        FAIL=$((FAIL + 1))
    fi
}

# Group 1: Folder convention — verify entity_id on stored memories
# Use recall with entity-id filter to verify scoping works
run_test 1 "acme-corp folder -> entity_id" \
    "hebbs recall 'budget approved' --entity-id acme-corp -k 3" \
    "budget"

run_test 2 "acme-corp nested email -> entity_id" \
    "hebbs recall 'SOC2 report' --entity-id acme-corp -k 3" \
    "SOC2"

run_test 3 "initech folder -> entity_id" \
    "hebbs recall 'migrating off legacy' --entity-id initech -k 3" \
    "legacy"

run_test 4 "globex-corp folder -> entity_id (case normalized)" \
    "hebbs recall 'healthcare' --entity-id globex-corp -k 3" \
    "healthcare"

# Group 2: Frontmatter override
run_test 6 "frontmatter overrides folder" \
    "hebbs recall 'override file' --entity-id big-deal-q2 -k 3" \
    "override"

run_test 7 "frontmatter on file outside entities/" \
    "hebbs recall 'initech migration' --entity-id initech -k 3" \
    "initech"

# Group 5: Prime scoping
run_test 18 "prime acme-corp returns acme content" \
    "hebbs prime --entity-id acme-corp" \
    "acme"

run_test 19 "prime initech returns initech content" \
    "hebbs prime --entity-id initech" \
    "initech"

# Group 5: Temporal recall
run_test 20 "temporal recall acme-corp" \
    "hebbs recall 'call notes' --strategy temporal --entity-id acme-corp -k 5" \
    "call"

# Group 5: Unscoped recall returns everything
run_test 21 "unscoped recall returns shared + entity content" \
    "hebbs recall 'SOC2 compliance' -k 10" \
    "SOC2"

# Group 6: Contradiction (budget $100K vs $50K in acme-corp)
run_test 24 "contradiction detection within entity" \
    "hebbs contradiction-prepare" \
    "budget"

echo ""
echo "=== RESULTS ==="
echo "  PASS: $PASS"
echo "  FAIL: $FAIL"
echo "  TOTAL: $((PASS + FAIL))"
echo ""
echo "Workspace preserved at: $WORK"
echo "Clean up: rm -rf $WORK"
```

## Running

```bash
export OPENAI_API_KEY=sk-...
bash docs/TASK-37-test-plan.md   # won't work, copy the script block

# Or extract and run:
chmod +x test-task-37.sh
./test-task-37.sh
```

## What to Manually Inspect

After the script runs, also verify by hand:

```bash
cd $WORK

# Inspect raw memories to verify entity_id field
hebbs get <memory-id>   # check entity_id field in output

# Verify shared knowledge has NO entity_id
hebbs recall "product features" -k 3 --json | grep entity_id
# Should show null/empty

# Verify cross-layer prime (deal memory + shared knowledge)
hebbs prime --entity-id acme-corp
# Should return BOTH acme call notes AND relevant shared content
# (e.g., SOC2 blog post should surface because acme needs SOC2)

# Verify entity isolation
hebbs recall "budget" --entity-id initech -k 5
# Should return Initech's $200K budget, NOT Acme's $100K/$50K

hebbs recall "budget" --entity-id acme-corp -k 5
# Should return Acme's budget discussion, NOT Initech's
```

## Not for CI

This test requires:
- LLM API key (real extraction, not mocked)
- ~60 seconds to run (indexing + extraction)
- Manual inspection of some results

Unit tests for `parse_entity_from_frontmatter` and `parse_entity_from_path` go in the Rust test suite and DO run in CI. This integration test is for manual validation only.
