# .hebbsignore

Control which files HEBBS indexes in your vault.

---

## Overview

When HEBBS indexes a vault, it walks all `.md` files and creates memories from heading sections. Some files -- templates, drafts, archived notes, generated content -- produce noise that clutters recall results. `.hebbsignore` lets you exclude them.

Create a `.hebbsignore` file at the root of your vault (next to `.hebbs/`):

```
my-vault/
  .hebbs/
  .hebbsignore       <-- this file
  notes/
  templates/          <-- excluded by .hebbsignore
  archive/            <-- excluded by .hebbsignore
  meeting-notes.md
```

## Syntax

`.hebbsignore` uses the same pattern syntax as `.gitignore`:

```
# Lines starting with # are comments
# Blank lines are ignored

# Exclude a directory and everything in it
templates/
archive/
.obsidian/

# Exclude files matching a glob
*.template.md
drafts/*.md

# Exclude a specific file
scratch-pad.md
```

Each non-blank, non-comment line is a glob pattern matched against file paths relative to the vault root.

## How Patterns Are Resolved

HEBBS merges ignore patterns from three sources, in order:

| Source | Purpose | Example |
|--------|---------|---------|
| **Built-in defaults** | Always excluded | `.hebbs/`, `.git/`, `.obsidian/`, `node_modules/`, `contradictions/` |
| **`config.toml`** | Per-vault config | `watch.ignore_patterns = ["vendor/", "generated/"]` |
| **`.hebbsignore`** | User-managed file | `templates/`, `archive/`, `*.draft.md` |

All three are merged at runtime. Duplicates are removed. The combined set is used by:

- **`hebbs index`** -- skips matched files during initial indexing
- **`hebbs serve`** (daemon) -- skips matched files during file watching
- **Config reload** -- daemon re-reads `.hebbsignore` when config changes are detected, no restart needed

## When to Use `.hebbsignore` vs `config.toml`

**Use `.hebbsignore` when:**
- You want gitignore-style convenience (one pattern per line, comments)
- The patterns are vault-specific and might change often
- You want the file visible at the vault root for easy discovery
- You want to commit it to version control alongside your notes

**Use `config.toml` when:**
- You're managing patterns programmatically (via `hebbs config set` or the panel UI)
- The patterns are part of a broader config change

Both work. Both are merged. Use whichever fits your workflow.

## Common Patterns

### Obsidian vault with templates

```
# .hebbsignore
templates/
.obsidian/
.trash/
```

### Project docs with generated content

```
# .hebbsignore
api-reference/          # auto-generated from code
CHANGELOG.md            # noise for recall
node_modules/
```

### Personal vault with drafts

```
# .hebbsignore
drafts/
_scratch/
*.wip.md
```

### Coding project with HEBBS alongside code

```
# .hebbsignore
target/
build/
dist/
*.lock
```

## Interaction with the Panel

The Memory Palace control panel (Settings tab) shows and edits `watch.ignore_patterns` from `config.toml`. Patterns from `.hebbsignore` are not shown in the panel UI -- they are merged at runtime. This is intentional: `.hebbsignore` is a file you edit directly, like `.gitignore`.

## Verifying Your Patterns

To see which files HEBBS will index after applying all ignore patterns:

```bash
hebbs status
```

The status output shows total file count and indexed percentage. If files you expect to be excluded are still showing up, check:

1. Pattern syntax -- directory patterns need a trailing `/` (e.g., `templates/` not `templates`)
2. The file is saved -- daemon picks up `.hebbsignore` on next config reload cycle
3. Already-indexed files -- excluding a file removes it from future indexing but does not delete memories already created from it. Run `hebbs index --force` to rebuild from scratch.

## Technical Details

- Patterns are processed by Rust's `globset` crate, which supports standard glob syntax (`*`, `**`, `?`, `[...]`)
- Each pattern is matched both as-is and with a `**/` prefix, so `templates/` matches `templates/` at the root and `subdir/templates/` anywhere in the tree
- `.hebbsignore` is read from `vault_root/.hebbsignore` where `vault_root` is the directory containing `.hebbs/`
- The file is optional. If it doesn't exist, only built-in defaults and `config.toml` patterns apply
