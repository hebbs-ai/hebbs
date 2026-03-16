---
title: "One Binary. One Brain."
description: "HEBBS used to ship two binaries that stored memories in two separate places. They could not see each other. v0.3.0 fixes that: one binary, one RocksDB, one recall that searches everything."
pubDate: 2026-03-16
tags: ["engineering", "release", "architecture"]
heroImage: "/images/one-binary-one-brain.png"
ogImage: "/images/one-binary-one-brain.png"
---

HEBBS used to ship as two separate binaries: `hebbs-cli` and `hebbs-vault`. They did different things, stored data in different places, and could not see each other's memories.

That ends with v0.3.0. There is now one binary: `hebbs`.

## The Problem With Two Brains

The old split made sense at the time. `hebbs-cli` was the agent interface: store a memory, recall context, prime before a conversation. It talked to a background server and wrote memories into a RocksDB instance at `~/.hebbs/data/`.

`hebbs-vault` was the file interface: index a directory of markdown files, watch for changes, rebuild the index. It maintained a separate RocksDB at `<project>/.hebbs/index/db/`.

Same engine code. Two storage locations. Two brains.

The consequence: a memory your agent stored via `hebbs-cli remember` was invisible when you ran `hebbs-vault recall`. A design doc indexed by the vault was invisible to the CLI. If you asked your agent "what did we decide about the API pagination strategy?", it could only search one brain or the other, never both.

```
Before                          After

~/.hebbs/data/      (CLI)       <project>/.hebbs/index/db/
                                  (one brain, searches everything)
<project>/.hebbs/   (vault)
```

The brains did not talk. This was wrong.

## One Binary, One Brain

v0.3.0 merges everything. The `hebbs` binary handles all operations: remember, recall, prime, index, watch, reflect. One RocksDB. One `recall` that returns memories from your project docs, your agent-stored preferences, and your reflection insights in a single ranked list.

The command changes are mechanical:

| Old | New |
|-----|-----|
| `hebbs-cli remember "..."` | `hebbs remember "..."` |
| `hebbs-cli recall "..."` | `hebbs recall "..."` |
| `hebbs-cli prime ...` | `hebbs prime ...` |
| `hebbs-vault init .` | `hebbs init .` |
| `hebbs-vault index .` | `hebbs index .` |
| `hebbs-vault watch .` | `hebbs watch .` |
| `hebbs-vault status` | `hebbs status` |

All flags and output formats are identical. Only the binary name changes.

If you installed via Homebrew, nothing breaks today. The formula installs symlinks so both `hebbs-cli` and `hebbs-vault` still point to the new `hebbs` binary. Update your scripts when convenient. The symlinks will be removed in a future release.

If you installed via the install script or built from source, update your scripts now and optionally create temporary symlinks:

```sh
ln -s $(which hebbs) /usr/local/bin/hebbs-cli
ln -s $(which hebbs) /usr/local/bin/hebbs-vault
```

## How the Brain Discovers Itself

With one binary, there is one consistent discovery chain. Run any `hebbs` command from inside a project and it finds the right brain automatically:

```
1. --vault flag or HEBBS_VAULT env var   (explicit override)
2. Walk up from current directory looking for .hebbs/
3. Fall back to ~/.hebbs/                (global brain)
4. Nothing found: "Run: hebbs init <path>"
```

No configuration needed for the common case. `cd` into a project directory, run `hebbs recall "..."`, and you search that project's brain. Step outside any project and you search the global brain at `~/.hebbs/`.

## Agent Memories Are Files Now

This is the change that matters most.

In the old design, memories stored by an agent via `hebbs-cli remember` lived only in RocksDB. They were not files. You could not read them, version control them, or move them to a new machine. If you deleted `~/.hebbs/data/`, they were gone.

Now, every `hebbs remember` call writes a markdown file into your vault:

```
my-app/memories/2026-03-16-explicit-error-handling.md
---
hebbs-kind: memory
hebbs-importance: 0.9
hebbs-entity-id: coding_prefs
---

Developer prefers explicit error handling (match/if-let) over unwrap().
```

The watcher picks up the file and indexes it. The memory lives in RocksDB for fast recall, but the file is the source of truth. Same for reflection insights: they are written as `.md` files in `memories/insights/`.

The consequence: delete `.hebbs/` at any time. Run `hebbs init . && hebbs index .` and every memory, preference, and insight comes back. The brain rebuilds from files because the files are the brain.

This also means a new machine is trivial:

```sh
git clone git@github.com:user/my-app.git
cd my-app
hebbs init .
hebbs index .
```

If your `memories/` directory syncs with the repo (or via Dropbox, iCloud, whatever you use), the agent picks up exactly where it left off.

## What One Recall Means

Before the merge, an agent querying HEBBS had to decide: search the server brain (agent memories) or the vault brain (file-backed memories)? It could never get a single ranked result across both.

Now it runs one command:

```sh
hebbs recall "API pagination strategy"
```

And gets back a ranked list that might include:

1. `docs/api-design.md` section "Pagination" (source: file, score: 0.92)
2. `DECISIONS.md` section "API Pagination" (source: file, score: 0.87)
3. `memories/cursor-pagination-preference.md` (source: agent, score: 0.81)

The agent sees the full picture: design docs, decision records, and its own stored preferences, ranked together by relevance. It does not need to know which brain to search. There is only one.

## Upgrade

```sh
brew upgrade hebbs-ai/tap/hebbs
```

Or via the install script:

```sh
curl -fsSL https://hebbs.ai/install | sh
```

The binary is `hebbs`. Update your agent skills, CI scripts, and shell aliases when you have a moment. Homebrew users have symlinks buying them time.

One binary. One brain. One recall.
