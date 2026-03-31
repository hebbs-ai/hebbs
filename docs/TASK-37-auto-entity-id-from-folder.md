# TASK-37: Auto Entity ID from `entities/` Folder Convention

**Status:** Planned  
**Priority:** High  
**Scope:** `hebbs-vault` crate (extract.rs), CLI docs  

---

## Problem

Today, entity_id assignment during `hebbs index` relies entirely on LLM extraction (`find_primary_entity`). This works for extracting entities mentioned in prose, but:

- **Document-level memories get `entity_id: None`** — never scoped.
- **Proposition entity_id is fragile** — depends on what the LLM chooses to extract.
- **No way to bulk-import scoped content** — reps can't just dump call notes into a folder and have them auto-tagged.
- **`hebbs remember` requires explicit `--entity-id` flag** for scoping.

For CRM, support, legal, and every vertical use case, users need a zero-friction way to scope content to entities: save the file in the right folder, done.

## Solution

### Convention: `entities/` as a reserved top-level folder

```
workspace/
├── entities/              <- auto-tagged by subfolder name
│   ├── acme-corp/
│   │   ├── call-2026-03-15.md      -> entity_id: "acme-corp"
│   │   ├── call-2026-03-22.md      -> entity_id: "acme-corp"
│   │   └── emails/sarah.md         -> entity_id: "acme-corp"
│   ├── initech/
│   │   └── discovery-notes.md      -> entity_id: "initech"
│   └── project-alpha/
│       └── status.md               -> entity_id: "project-alpha"
│
├── products/              <- shared knowledge, no entity_id
├── case-studies/          <- shared (or LLM-extracted entity)
├── blogs/
├── training/
└── anything-else/
```

### Resolution order (first match wins)

1. **Frontmatter override:** File contains `entity: acme-corp` in YAML frontmatter -> use it.
2. **Folder convention:** File path starts with `entities/{name}/` -> `entity_id = name` (second path segment, always).
3. **LLM extraction:** Current behavior — `find_primary_entity()` from extracted entities.
4. **None:** Shared knowledge, no entity scoping.

### Rules

- `entities/` must be at the workspace root. `docs/entities/` does not trigger the convention.
- Entity ID is always the **second path segment**: `entities/acme-corp/deep/nested/file.md` -> `entity_id: "acme-corp"`.
- Entity ID is lowercased and slugified (alphanumeric + hyphens).
- Frontmatter `entity:` can be used on ANY file, not just those in `entities/`. This allows scoping a case study to a specific account: `case-studies/initech-migration.md` with `entity: initech`.
- Both Document memories (Layer 1) and Proposition memories (Layer 2) inherit the resolved entity_id.

## Implementation

### Changes to `hebbs-vault/src/extract.rs`

**1. Add frontmatter parser** (~15 lines)

```rust
/// Extract entity_id from YAML frontmatter if present.
/// Looks for `entity: <value>` in --- delimited frontmatter block.
fn parse_entity_from_frontmatter(content: &str) -> Option<String> {
    let content = content.trim();
    if !content.starts_with("---") {
        return None;
    }
    let end = content[3..].find("---")?;
    let frontmatter = &content[3..3 + end];
    for line in frontmatter.lines() {
        let line = line.trim();
        if let Some(value) = line.strip_prefix("entity:") {
            let entity = value.trim().to_lowercase();
            if !entity.is_empty() {
                return Some(entity);
            }
        }
    }
    None
}
```

**2. Add folder convention parser** (~10 lines)

```rust
/// Extract entity_id from file path if it follows the entities/ convention.
/// `entities/acme-corp/anything.md` -> Some("acme-corp")
fn parse_entity_from_path(rel_path: &str) -> Option<String> {
    let parts: Vec<&str> = rel_path.split('/').collect();
    if parts.len() >= 2 && parts[0] == "entities" && !parts[1].is_empty() {
        Some(parts[1].to_lowercase())
    } else {
        None
    }
}
```

**3. Add resolution function** (~10 lines)

```rust
/// Resolve entity_id for a file using the priority chain:
/// frontmatter > folder convention > LLM extraction > None
fn resolve_entity_id(
    content: &str,
    rel_path: &str,
    llm_entities: &[extraction::ExtractedEntity],
    proposition_content: Option<&str>,
) -> Option<String> {
    // 1. Frontmatter
    if let Some(entity) = parse_entity_from_frontmatter(content) {
        return Some(entity);
    }
    // 2. Folder convention
    if let Some(entity) = parse_entity_from_path(rel_path) {
        return Some(entity);
    }
    // 3. LLM extraction (proposition-level only)
    if let Some(prop_content) = proposition_content {
        return find_primary_entity(prop_content, llm_entities);
    }
    None
}
```

**4. Update `extract_and_store_file`**

In the Document memory creation (~line 198-205):
```rust
// Before (current):
entity_id: None,

// After:
entity_id: resolve_entity_id(file_content, rel_path, &[], None),
```

In the Proposition memory creation (~line 327-334):
```rust
// Before (current):
let primary_entity = find_primary_entity(&prop.content, &extraction_output.entities);
entity_id: primary_entity,

// After:
let primary_entity = resolve_entity_id(
    file_content, rel_path, &extraction_output.entities, Some(&prop.content)
);
entity_id: primary_entity,
```

### Changes to CLI / docs

- Document the `entities/` convention in `hebbs init` output hint.
- Add `entities/` to default scaffold when `hebbs init` creates a workspace.
- Document frontmatter `entity:` field in API docs.

## Use Cases Enabled

| Use Case | Workflow |
|----------|---------|
| **CRM** | Save call notes to `entities/acme-corp/`, auto-scoped. `prime("acme-corp")` returns everything. |
| **Support** | Save ticket threads to `entities/ticket-4821/`. Temporal recall reconstructs the thread. |
| **Legal** | Save case files to `entities/smith-v-jones/`. Causal recall traces precedent chains. |
| **Coding** | Save decision logs to `entities/auth-refactor/`. Recall what was tried and why. |
| **Personal** | Save per-person context to `entities/john/`. Prime before each conversation. |
| **Robotics** | Save mission logs to `entities/robot-07/`. Reflect across fleet. |

## Testing

1. **Unit tests** for `parse_entity_from_frontmatter` (with/without frontmatter, edge cases).
2. **Unit tests** for `parse_entity_from_path` (valid paths, nested paths, non-entities paths).
3. **Integration test:** Index a workspace with `entities/test-entity/doc.md` and verify memories have `entity_id: "test-entity"`.
4. **Integration test:** File with frontmatter `entity: override` inside `entities/original/` uses "override" not "original".
5. **Integration test:** File outside `entities/` with no frontmatter falls back to LLM extraction.

## Estimated Effort

~30-40 lines of Rust in `extract.rs`. No new crates, no schema changes, no migration. Half-day task.
