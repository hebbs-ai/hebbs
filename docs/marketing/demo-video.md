# Demo Video Script -- YC Application (3 minutes)

Screen recording with voiceover. Dark terminal, large font. Let every panel render fully before moving on.

---

## Scene 1: Launch (0:00 -- 0:12)

**TYPE:**

```
hebbs-demo interactive --config gemini-vertex --entity-id acme_corp --verbosity verbose
```

**SPEAK (while it loads):**

> "This is HEBBS -- a memory engine for AI agents. What you're seeing is our Python SDK -- we're making it public this week -- and a CLI tool we built on top of it. I'm running this locally right now, so you'll see some LLM latency -- that's Google Gemini plus my internet and region, nothing to do with HEBBS. Everything in the panels below -- the recall scores, the latency numbers, the insight lineage -- that's HEBBS engine time."

> "I'm starting as acme_corp -- that's one entity. Later I'll switch to a different entity to show you how multitenancy works."

---

## Scene 2: First Memory (0:12 -- 0:50)

**TYPE (as the prospect):**

```
We're evaluating vendors for an AI customer support platform. Our current system handles about 50,000 tickets a month and we're on Zendesk.
```

**SPEAK (while the agent responds and panels render):**

> "Watch the panels. HEBBS just stored that as a memory with importance scoring -- it detected buying intent and a tech stack signal. It scored it 0.85 importance, tagged the context automatically. That REMEMBER operation took under a millisecond."

*Pause. Let the viewer read the REMEMBER panel.*

> "Now look at the RECALL panel. Before the agent replied, HEBBS searched across similarity and temporal strategies. Two milliseconds. That's engine time, not LLM time."

---

## Scene 3: Second Turn -- Recall in Action (0:50 -- 1:20)

**TYPE:**

```
We also looked at Intercom but their AI felt like a wrapper around GPT. We need something that actually learns from past tickets.
```

**SPEAK (as panels render):**

> "Two memories stored now. But look at the recall -- HEBBS pulled the first memory about Zendesk and 50,000 tickets alongside this new one. The agent's response references both. It didn't re-read a transcript. It recalled structured memories ranked by relevance."

---

## Scene 4: Switch Entity -- Multitenancy (1:20 -- 1:55)

**TYPE:**

```
/session techflow_inc
```

**SPEAK (while prime panel renders):**

> "This is the multitenancy I mentioned. I just switched to techflow_inc -- a completely different entity. Watch the prime panel -- zero memories loaded. HEBBS knows nothing about this prospect. The data is fully isolated."

**TYPE:**

```
Hey, following up from our call last week. You mentioned something about compliance?
```

**SPEAK (after the agent responds):**

> "Notice the agent didn't call me by name, didn't mention Zendesk, didn't reference anything from acme_corp. Complete entity isolation. Now let me switch back."

**TYPE:**

```
/session acme_corp
```

**SPEAK (as prime panel shows loaded memories):**

> "And there it is -- HEBBS primed the session and loaded every memory for acme_corp. The agent already knows about Zendesk, the 50,000 tickets, the Intercom comparison. No context window stuffing. Structured recall, scoped to the entity."

---

## Scene 5: Reflect -- The Wow Moment (1:55 -- 2:30)

**TYPE:**

```
/reflect
```

**SPEAK (while reflect panel renders -- clustering, proposing, validating, storing):**

> "This is what separates HEBBS from a vector database. Reflect takes raw conversation memories and consolidates them into institutional knowledge. Watch the pipeline -- it clusters related memories, proposes an insight, validates it against the evidence, and stores it with full lineage back to every source memory."

*Pause. Let the viewer read the insight and its source lineage.*

> "That insight didn't exist five seconds ago. HEBBS synthesized it from the raw episodes. And it traces back to every memory that contributed. This is how agents go from 'I remember what you said' to 'I know what I've learned.'"

**TYPE:**

```
/insights
```

**SPEAK:**

> "There it is -- distilled knowledge with lineage. Every insight links back to the memories that produced it."

---

## Scene 6: Close on Latency (2:30 -- 2:50)

**TYPE:**

```
/stats
```

**SPEAK:**

> "Look at the numbers. Every HEBBS operation -- remember, recall, prime, reflect storage -- sub-10 milliseconds. The LLM took one to two seconds per turn. HEBBS is not the bottleneck. The LLM is. And this is a single Rust binary with embedded storage. No Redis. No Postgres. No Pinecone. No infrastructure to manage."

---

## Scene 7: Exit (2:50 -- 3:00)

**TYPE:**

```
quit
```

**SPEAK (over the session summary):**

> "Single binary. Sub-10ms. Agents that actually learn. That's HEBBS."

---

## Recording Checklist

- QuickTime screen recording with voiceover (or OBS if you want audio separately)
- Terminal font: 16pt minimum so YC reviewers can read the panels
- Dark terminal theme -- Rich panels pop on dark backgrounds
- Don't rush. Let every panel fully render. Pause 2-3 seconds on each panel so they can read
- If a panel is too dense to read, verbally call out the key number ("0.92 similarity score", "1.8 milliseconds")
- Total target: 2:50 -- leave 10 seconds of breathing room under the 3-minute cap

## Why This Demo Lands

- **The panels are the pitch.** Without them it's a chatbot. With them, the reviewer sees recall scores, strategy names, latency breakdowns, and insight lineage -- and understands this is infrastructure, not a wrapper.
- **Reflect is the killer moment.** No competitor shows "raw episodes turning into distilled knowledge with lineage tracking" live.
- **Sub-10ms next to 1-2s LLM latency** makes the point without arguing it: HEBBS is not the bottleneck.
- **Single binary, no infrastructure** lands immediately with technical YC partners who've seen the pain of running Postgres + Redis + Pinecone + a graph DB.
