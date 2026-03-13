# Managing Git in `hebbs-repos`

This repo is your private unified workspace. Daily development happens here, and selected folders are pushed back to their original upstream repositories using `git subtree`.

## Repository model

- `origin` points to your private repo: `git@github.com:parag/hebbs-repos.git`
- Each subtree folder maps to an upstream remote:

| Prefix folder | Remote name | Upstream URL (SSH) |
|---|---|---|
| `hebbs/` | `hebbs-engine` | `git@github.com:hebbs-ai/hebbs.git` |
| `hebbs-typescript/` | `hebbs-ts` | `git@github.com:hebbs-ai/hebbs-typescript.git` |
| `hebbs-python/` | `hebbs-py` | `git@github.com:hebbs-ai/hebbs-python.git` |
| `hebbs-website/` | `hebbs-web` | `git@github.com:hebbs-ai/hebbs-website.git` |
| `hebbs-docs/` | `hebbs-docs` | `git@github.com:hebbs-ai/hebbs-docs.git` |
| `hebbs-deploy/` | `hebbs-deploy` | `git@github.com:hebbs-ai/hebbs-deploy.git` |
| `hebbs-skill/` | `hebbs-skill` | `git@github.com:hebbs-ai/hebbs-skill.git` |
| `homebrew-tap/` | `homebrew-tap` | `git@github.com:hebbs-ai/homebrew-tap.git` |

`experience-demo/` has no upstream remote and is maintained only in this unified repo unless you add one later.

## Daily workflow (most common)

1. Edit code anywhere in this repo.
2. Commit to unified repo:
   - `git add .`
   - `git commit -m "your message"`
3. Push private repo:
   - `git push origin main`

You do **not** need subtree commands for normal day-to-day commits.

## Release/back-port workflow (push to original repos)

When you want to publish changes from one folder back to its original repo:

1. Check what changed since last push tag:
   - `git diff --stat last-push/hebbs -- hebbs/`
   - `git diff --stat last-push/hebbs-typescript -- hebbs-typescript/`
   - (repeat for other prefixes)
2. Push only the changed prefix:
   - `git subtree push --prefix=hebbs hebbs-engine main`
   - `git subtree push --prefix=hebbs-typescript hebbs-ts main`
   - etc.
3. Move that prefix tag to new baseline:
   - `git tag -f last-push/hebbs`
   - `git push origin --tags`

## Pulling upstream changes into unified repo

If the original repo advanced independently:

- `git subtree pull --prefix=hebbs hebbs-engine main --squash`
- resolve conflicts if any
- commit and push `origin main`

## One-command check: which subtrees changed?

```bash
for p in hebbs hebbs-typescript hebbs-python hebbs-website hebbs-docs hebbs-deploy hebbs-skill homebrew-tap; do
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

## Clone on another machine (no repeated setup pain)

1. Clone:
   - `git clone git@github.com:parag/hebbs-repos.git`
   - `cd hebbs-repos`
2. Configure subtree remotes (idempotent):
   - `./setup-subtree-remotes.sh`
3. Verify remotes:
   - `git remote -v`

All code is already in the unified repo; the script just restores subtree remote aliases for future `subtree push/pull`.

## Troubleshooting

- Push rejected (non-fast-forward):
  1. `git subtree pull --prefix=<prefix> <remote> main --squash`
  2. resolve and commit
  3. rerun `git subtree push --prefix=<prefix> <remote> main`

- Unsure which repo needs release:
  - use the diff/tag check above and push only changed prefixes.
