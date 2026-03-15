# Blog: Why Your AI Agent's Memory Retrieval Wastes Half Its Context Window

**Status:** Backlog (feature deprioritized -- agents handle truncation themselves for now)
**Related:** TASK-12 Feature 3

## What to communicate

HEBBS returns ranked memories via `prime()`. Today, agents receive top-K results and truncate to fit their context window. This works, but the agent truncates blindly. HEBBS already knows which memories matter most -- it should do the packing.

The planned feature: a `max_tokens` parameter on `prime()` that lets HEBBS greedily fill the token budget, highest-score first. The agent gets exactly what fits, already optimally ranked. No wasted tokens, no blind truncation.

### Why it matters

Every AI agent that retrieves memory context faces the same problem: the retrieval system returns more than fits. The agent (or framework) truncates from the bottom. But "bottom" in retrieval order is not always "least important" -- it depends on how the retrieval system ranks. When HEBBS does the packing, it uses its full composite score (relevance 0.5, recency 0.2, importance 0.2, reinforcement 0.1) to decide what fits.

For a 2,000-token budget pulling from a 10,000-memory vault, the difference between "first 2,000 tokens of top-50 results" and "optimally packed 2,000 tokens across all candidates" is meaningful. The agent gets denser, more relevant context.

### How it would work (technical)

1. `prime(context, max_tokens=2000)` queries the index as usual
2. Candidates ranked by composite score (same as today)
3. Instead of slicing at `max_memories`, greedily pack: add highest-score memory, subtract its token count, repeat until budget exhausted
4. Token counting: approximate (`content.len() / 4`) by default, exact tokenizer optional
5. Optional `prefer_insights: true` to favor consolidated insights (denser information per token) over raw episode memories when budget is tight

### Current state

Deprioritized. Agents calling HEBBS today receive ranked results and handle their own truncation. The optimization is real but not urgent -- it becomes important when:
- Agents are in production with tight context windows where every token matters
- The Obsidian sidebar ships and needs to fit content into a fixed display budget
- Users have large vaults (10,000+ memories) where the gap between naive truncation and optimal packing is significant

### Audience

- AI agent builders using HEBBS as a memory backend
- Framework authors (LangChain, CrewAI, etc.) integrating HEBBS retrieval
- Anyone building context-window-aware applications

### Messaging angles

1. **"Your agent is throwing away its best memories."** Blind truncation after retrieval discards information that the retrieval system already ranked. Let the ranker do the packing.
2. **"One parameter, zero wasted tokens."** Add `max_tokens` to your `prime()` call. That's it. HEBBS handles the rest.
3. **"Insights over episodes."** When budget is tight, HEBBS can prefer consolidated insights (more information per token) over raw memories. Information density as a retrieval strategy.
