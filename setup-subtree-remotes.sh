#!/usr/bin/env bash
set -euo pipefail

# Configure subtree remotes for this workspace.
# Safe to run repeatedly: existing remotes are updated to the expected URL.
declare -A REMOTES=(
  [hebbs-engine]="git@github.com:hebbs-ai/hebbs.git"
  [hebbs-ts]="git@github.com:hebbs-ai/hebbs-typescript.git"
  [hebbs-py]="git@github.com:hebbs-ai/hebbs-python.git"
  [hebbs-web]="git@github.com:hebbs-ai/hebbs-website.git"
  [hebbs-docs]="git@github.com:hebbs-ai/hebbs-docs.git"
  [hebbs-deploy]="git@github.com:hebbs-ai/hebbs-deploy.git"
  [hebbs-skill]="git@github.com:hebbs-ai/hebbs-skill.git"
  [hebbs-blog]="git@github.com:hebbs-ai/hebbs-blog.git"
  [homebrew-tap]="git@github.com:hebbs-ai/homebrew-tap.git"
  [hebbs-enterprise]="git@github.com:hebbs-ai/hebbs-enterprise.git"
  [hebbs-platform]="git@github.com:hebbs-ai/hebbs-platform.git"
  [hebbs-dashboard]="git@github.com:hebbs-ai/hebbs-dashboard.git"
)

for remote in "${!REMOTES[@]}"; do
  url="${REMOTES[$remote]}"
  if git remote get-url "$remote" >/dev/null 2>&1; then
    git remote set-url "$remote" "$url"
    echo "updated $remote -> $url"
  else
    git remote add "$remote" "$url"
    echo "added $remote -> $url"
  fi
done

echo "Subtree remotes are configured."
