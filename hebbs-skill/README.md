# HEBBS Skill

Teaches AI agents how to use [HEBBS](https://hebbs.ai), a local-first cognitive memory engine that stores, indexes, and retrieves knowledge across four dimensions.

## Install the skill

### OpenClaw

```bash
git clone https://github.com/hebbs-ai/hebbs-skill.git ~/.openclaw/skills/hebbs
```

Restart your OpenClaw session to pick up the skill.

### Claude Code

```bash
git clone https://github.com/hebbs-ai/hebbs-skill.git ~/.claude/skills/hebbs
```

### Claude.ai

1. Download this repo as a ZIP
2. Go to Settings > Capabilities > Skills > Upload the ZIP

## How It Works

HEBBS creates a `.hebbs/` directory next to your files: a self-contained cognition layer. Build the index once, then share it across agents, machines, or your team. Everyone gets the same memory instantly.

`.hebbsignore` works like `.gitignore`: your private files stay private, your agents only see what you allow.

Your files are the source of truth. `.hebbs/` is derived and rebuildable. Delete it anytime and run `hebbs init . && hebbs index .` to get it back.

## Install HEBBS

**macOS (Homebrew):**

```bash
brew install hebbs-ai/tap/hebbs
```

**Any platform (Linux, macOS):**

```bash
curl -sSf https://hebbs.ai/install | sh
```

## Quick start

```bash
hebbs init . --provider openai --key $OPENAI_API_KEY
hebbs index .
hebbs recall "your query here"
```

`--model` is optional (defaults per provider). Embedding auto-configures when using OpenAI. The daemon starts automatically and watches for file changes.

## Verify

```bash
hebbs status
```

## Tuning

The `tune/` directory contains a skill for agent-driven retrieval tuning: generate evals, run baselines, optimize parameters, and export compiled rules.

## Links

- [HEBBS](https://hebbs.ai)
- [HEBBS GitHub](https://github.com/hebbs-ai/hebbs)
- [Homebrew Tap](https://github.com/hebbs-ai/homebrew-tap)
