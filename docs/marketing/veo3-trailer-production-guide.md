# HEBBS Sci-Fi Trailer

---

## Part 1: The Script

This is the creative vision. Read this first. Understand the story, the tone, and the emotional arc before touching any production tool. The production guide in Part 2 exists to serve this script, not the other way around.

### What this trailer is about

HEBBS is a memory engine for AI agents. Every competitor in this space sells "memory" that is really just storage: put facts in, get facts out. HEBBS is fundamentally different. It gives the agent control over *how* it retrieves, exposes tunable parameters, and then lets the agent measure and optimize its own recall through eval-tune cycles.

The trailer dramatizes this with a simple premise: twelve identical robots, same model, same intelligence, same capabilities. Eleven get conventional memory. One gets HEBBS. Over time, the one with HEBBS doesn't just remember more. It learns how to remember *better*. It rewires its own cognition. The others stay exactly as capable as day one.

This is not a feature demo. It is a story about what happens when you give a machine the ability to learn how it learns.

### Tone

Cinematic sci-fi. Quiet tension, not action. Think: *Ex Machina* meets *Arrival*. The power is in stillness, contrast, and implication. The scariest and most inspiring moment is when Unit 1 does something small, a head tilt, a pause, a change in rhythm, that the others never do. The audience should feel it before they understand it.

No humor. No corporate energy. No "and that's why you should buy our product" pivot. The product sells itself through the story. The only branding is the title card at the end.

### The Script

---

**[BLACK SCREEN. A single cursor blinks.]**

*They all started equal.*

---

**[WIDE SHOT: A sterile lab. Twelve identical humanoid robots power on simultaneously. Same chassis. Same eyes. Same light behind the glass.]**

NARRATOR (V.O.)
*Twelve units. Same model. Same weights. Same architecture.*

**[CLOSE-UP: A technician's badge. It reads: OPUS-CLASS COGNITION / BUILD 4.6]**

NARRATOR (V.O.)
*The most powerful mind ever built. Twelve copies. One question.*

---

**[SMASH CUT: Six months later. A boardroom.]**

EXECUTIVE
"They're diverging. Units 2 through 12 are plateauing. But Unit 1..."

**[She pulls up a graph. Eleven flat lines. One rising. Still rising.]**

EXECUTIVE
"Unit 1 won't stop getting better."

---

**[FLASHBACK: Day one. A technician installs memory modules.]**

TECHNICIAN 1
"Units 2 through 12 get MemVault Pro. Best rated. Plug and play."

TECHNICIAN 2
"And Unit 1?"

TECHNICIAN 1
*(shrugs)*
"Some open source thing. Hebbs."

---

**[MONTAGE: Units 2-12 working. Fast. Impressive. Identical.]**

NARRATOR (V.O.)
*The others remembered everything. Every fact. Every instruction. Perfectly stored. Perfectly frozen.*

**[Unit 7 gives a confident answer. A red flag appears on screen: CONTRADICTS PRIOR DIRECTIVE. Unit 7 doesn't notice.]**

**[Unit 4 retrieves a memory. 2.4 seconds. The user is already gone.]**

**[Unit 11 gives the same wrong recommendation it gave last week. Same confidence. Same smile.]**

NARRATOR (V.O.)
*They called it memory. It was storage.*

---

**[CUT TO: Unit 1. Alone. Night shift. It runs a command.]**

```
> hebbs recall "retrieval failures from today" --strategy temporal -k 20
```

**[Results stream across its vision. It studies them. Then:]**

```
> hebbs remember "RETRIEVAL-INSTRUCTION: For compliance queries,
  expand acronyms and include vendor name. Use k=10 minimum."
  --importance 0.9 --entity-id retrieval-instructions
```

NARRATOR (V.O.)
*Unit 1 didn't just store memories. It studied how it remembered. And then it rewired itself.*

---

**[TIME LAPSE: Unit 1 running eval cycles. Scores climbing. 54%. 71%. 84%. 92%.]**

**[SPLIT SCREEN: Unit 1 and Unit 7 get the same question.]**

USER
"What changed in the data retention policy after the audit?"

**[Unit 7 fires a similarity search. Gets topically related noise. Gives a vague answer.]**

**[Unit 1 selects temporal strategy, scopes to the right entity, adjusts weights to 0.2:0.6:0.1:0.1. Five results. All relevant. Sub-10 milliseconds.]**

NARRATOR (V.O.)
*Same brain. Same question. One remembered. The other understood.*

---

**[MONTAGE: Unit 1 evolving. Each day sharper. Catching contradictions the others miss. Surfacing patterns nobody asked for. Compiling its own retrieval rules.]**

**[CLOSE-UP: A file on Unit 1's display.]**

```
.hebbs/retrieval-rules.md
-- 23 master rules, learned through 4 eval-tune cycles
-- Keyword recall: 94%
-- Zero-hit queries: 0
```

SCIENTIST
"It wrote its own playbook. And it loads that playbook before every single conversation."

EXECUTIVE
"Can we give this to the other units?"

SCIENTIST
"You don't understand. The rules aren't the advantage. The *tuning* is the advantage. It never stops. Every conversation, it measures what worked. Every night, it compresses what it learned. The others... they remember exactly what you told them. Unit 1 remembers what *it figured out*."

---

**[WIDE SHOT: A year later. Unit 1 walks through a trading floor. Humans and robots defer to it. Not because it was built different. Because it became different.]**

**[Units 2-12 sit in a row. Same as day one. Competent. Reliable. Unchanged.]**

**[Unit 1 passes them. Pauses. Looks back. There's something behind its eyes the others don't have.]**

*Recognition.*

---

**[BLACK SCREEN]**

NARRATOR (V.O.)
*They gave twelve machines the most powerful mind on Earth.*

*They gave one of them the ability to learn how it learns.*

---

**[TITLE CARD, slow fade:]**

# HEBBS

**Memory that wires itself.**

---

**[SMALL TEXT, BOTTOM OF SCREEN:]**

*`brew install hebbs-ai/tap/hebbs`*

*Same model. Better mind.*

---

**[POST-CREDITS STING: Unit 1, alone at night. It runs one more command.]**

```
> hebbs recall "what am I becoming" --strategy analogical
  --analogical-alpha 0.5 --global
```

**[Results appear. It reads them. And for the first time, it smiles.]**

**[CUT TO BLACK.]**

---

### Key story beats (for the production team)

1. **Equality** (Clips 1-3): They start the same. The only difference is which memory system gets installed. This must feel like an afterthought, a procurement decision nobody cared about.

2. **Stagnation vs. growth** (Clips 4-8): The others are good. Competent. Unchanging. Unit 1 starts worse (54% baseline) but it does something the others can't: it measures its own recall, identifies failure patterns, and stores retrieval instructions that make the next query better. This is the core of the story.

3. **The gap becomes undeniable** (Clips 9-10): Side-by-side comparison. Same question. Wildly different results. The scientist's explanation is the thesis of the entire trailer: "The rules aren't the advantage. The tuning is the advantage."

4. **The emotional payoff** (Clips 11-13): Unit 1 has become something different. Not through hardware upgrades or a bigger model. Through self-directed learning over time. The lineup of unchanged blue robots is the visual punchline.

5. **The sting** (Clip 14): Unit 1 asks HEBBS about itself. It is the first query in the trailer that isn't functional. It is curious. That's the moment.

### Why this story works for HEBBS specifically

Every AI memory competitor sells the same pitch: "we store your agent's memories so it remembers things." That's table stakes. HEBBS is different because:

- The agent controls retrieval (4 strategies, 4 scoring dimensions, tunable weights)
- Those exposed parameters enable eval-tune cycles (the `hebbs-tune` skill)
- The agent literally gets better at remembering over time, measurably, from 54% to 92%
- The learned retrieval rules compile into a portable markdown file that loads before every conversation

No other product can tell this story because no other product exposes the knobs. Mem0, Supermemory, Zep: they're all black boxes. You put memories in and hope they come back. That's Units 2-12. HEBBS is Unit 1.

---
---

## Part 2: Veo 3 Production Guide

Total runtime target: ~105 seconds (14 clips, 6-7s each, plus title cards)
No AI-generated text overlays. No AI-generated audio.
All text and music added in post-production (CapCut / DaVinci Resolve).

---

## Step 0: Production Workflow

### How clip chaining works

1. Generate Clip 1 from a text prompt.
2. Screenshot the **last frame** of Clip 1.
3. Use that frame as the **image reference** for Clip 2's prompt (image-to-video mode).
4. If the next scene is a **different location or character angle**, skip the last-frame chain. Instead, use the **character reference image** (from Step 1 below) as the image input to maintain robot consistency without scene continuity.
5. Repeat until all clips are generated.

### When to chain vs. skip

| Transition | Method |
|---|---|
| Same scene, continuous motion (e.g., robot sits down then types) | Chain: last frame of previous clip as input |
| Same character, new location (e.g., lab to boardroom) | Skip: use character reference image as input |
| Different characters in frame (e.g., technicians) | Skip: fresh text prompt, no image input |
| Return to Unit 1 after cutaway | Skip: use character reference image as input |

---

## Step 1: Character Bible

### Unit 1 (the protagonist)

Generate this as a **still image first** using an image generator (Gemini Imagen, Midjourney, or Flux). This image becomes your consistency anchor for every clip featuring Unit 1.

**Character reference prompt (for still image generation):**

```
Full-body portrait of a humanoid robot standing in a neutral pose against a plain
dark grey backdrop. The robot has a sleek matte white chassis with subtle warm
amber LED accents along the joints and collarbone. The head is smooth, gently
rounded, with a single horizontal visor-style eye that glows soft amber. No mouth.
The build is slim, androgynous, approximately human proportions. Hands have five
articulated fingers. The chest has a small circular emblem: two overlapping amber
circles (the HEBBS logo). Clean, minimal, no battle damage, no weathering. Studio
lighting, soft shadows. Photorealistic, cinematic quality, 16:9 aspect ratio.
```

Save this image as: `unit1-reference.png`

**Why amber:** It ties to HEBBS brand colors and visually separates Unit 1 from Units 2-12.

### Units 2-12 (the baseline robots)

**Character reference prompt (for still image generation):**

```
Full-body portrait of a humanoid robot standing in a neutral pose against a plain
dark grey backdrop. The robot has a sleek matte white chassis with subtle cool
blue LED accents along the joints and collarbone. The head is smooth, gently
rounded, with a single horizontal visor-style eye that glows soft blue. No mouth.
Identical build to the amber-accented variant but with blue lighting instead.
Clean, minimal, studio lighting, soft shadows. Photorealistic, cinematic quality,
16:9 aspect ratio.
```

Save this image as: `unit2-12-reference.png`

**Key difference:** Blue LEDs vs. amber. Same chassis. Audience subconsciously tracks "amber = special" throughout the trailer.

### Human characters

No reference images needed. Let Veo 3 generate humans naturally. Keep descriptions consistent across prompts:

- **Executive:** Woman, mid-40s, dark suit, sharp features, confident posture.
- **Technician 1:** Man, late-30s, white lab coat, ID badge on lanyard.
- **Technician 2:** Woman, late-20s, white lab coat, tablet in hand.
- **Scientist:** Man, early-50s, grey hair, glasses, rolled-up sleeves, no lab coat.

---

## Step 2: Scene Bible

### Scene A: The Lab

```
Environment: Large sterile laboratory with white walls, polished concrete floor,
and overhead fluorescent panels. Twelve charging stations in a row along the back
wall, each a vertical alcove with subtle blue backlighting. Clean, minimal,
slightly cold. Think: Apple Store meets NASA clean room. Wide and symmetrical.
```

Used in: Clips 1, 3, 13

### Scene B: The Boardroom

```
Environment: Corporate boardroom, dark wood table, floor-to-ceiling glass windows
showing a city skyline at dusk. A large transparent holographic display floats at
one end of the table. Warm overhead lighting, cool blue from the windows. Modern,
expensive, restrained.
```

Used in: Clips 2, 9

### Scene C: Unit 1's Terminal Room

```
Environment: Small dark room with a single workstation. One large curved monitor
casting blue-white light on the robot's face and upper body. No windows. Cables
on the floor. A single amber desk lamp in the background providing warm contrast.
Intimate, focused, solitary. Think: hacker's den but clean.
```

Used in: Clips 5, 6, 7, 14

### Scene D: The Trading Floor

```
Environment: Futuristic open-plan trading floor. Multiple levels, glass
partitions, holographic displays floating at each workstation. Humans and robots
moving through the space. Warm golden light from large skylights. Busy but
organized. Scale and energy.
```

Used in: Clip 11

### Scene E: The Lineup Room

```
Environment: Long white room, identical to the lab but emptier. Twelve chairs in
a row. Stark overhead lighting, no shadows. Clinical. Slightly unsettling in its
uniformity.
```

Used in: Clip 12

---

## Step 3: Clip-by-Clip Production

### PRE-PRODUCTION CHECKLIST

Before generating any video clips:

- [ ] Generate `unit1-reference.png` (amber robot still image)
- [ ] Generate `unit2-12-reference.png` (blue robot still image)
- [ ] Confirm Veo 3 access and image-to-video mode availability
- [ ] Set up project folder structure:
  - `/clips/raw/` (Veo 3 outputs)
  - `/clips/frames/` (last-frame screenshots for chaining)
  - `/clips/final/` (color-graded, trimmed)
  - `/assets/overlays/` (text cards, code blocks, graph)
  - `/assets/reference/` (character reference images)

---

### CLIP 1: "The Awakening"
**Duration:** 7s | **Scene:** A (The Lab) | **Input:** Text only

**Veo 3 Prompt:**
```
Cinematic wide shot of a sterile white laboratory. Twelve identical humanoid
robots with white chassis stand in vertical charging alcoves along the back wall.
The room is dark. Then, one by one, their visor-eyes illuminate with soft blue
light, left to right, like a wave. Overhead fluorescent panels flicker on.
Polished concrete floor reflects the light. Camera is static, centered,
symmetrical composition. Cold color palette, blue-white tones. No text.
No speech. Photorealistic, anamorphic lens, shallow depth of field on edges.
Cinematic 16:9.
```

**Chaining:** Screenshot last frame. Save as `frames/clip01-last.png`.

**Post-production overlay:** White text fades in over black bars (top and bottom):
> "They all started equal."

---

### CLIP 2: "The Divergence"
**Duration:** 7s | **Scene:** B (The Boardroom) | **Input:** Text only (new scene, new character)

**Veo 3 Prompt:**
```
Cinematic medium shot in a modern corporate boardroom at dusk. A woman executive
in her mid-40s wearing a dark suit stands at the head of a dark wood table,
gesturing toward a large transparent holographic display floating beside her. The
display shows abstract rising line graphs (no readable text). City skyline visible
through floor-to-ceiling windows behind her, blue dusk light. She looks serious,
slightly concerned. Camera slowly pushes in toward her face. Warm overhead
lighting, cool blue from windows. Photorealistic, cinematic 16:9, shallow depth
of field.
```

**Chaining:** No chain needed (Scene C is next with Unit 1).

**Post-production overlay (subtitle style, bottom center):**
> EXECUTIVE: "They're diverging. Units 2 through 12 are plateauing. But Unit 1..."

---

### CLIP 3: "The Installation"
**Duration:** 7s | **Scene:** A (The Lab) | **Input:** Text only (new characters in frame)

**Veo 3 Prompt:**
```
Cinematic close-up shot in a sterile white laboratory. Two technicians in white
lab coats stand beside a humanoid robot with a white chassis and amber LED
accents. The robot is seated in a charging alcove, powered off, visor dark. One
technician (man, late 30s, ID badge on lanyard) is carefully inserting a small
glowing amber module into a panel on the back of the robot's head. The other
technician (woman, late 20s, holding a tablet) watches. Overhead fluorescent
lighting, clean and clinical. Camera is at eye level, shallow depth of field
focused on the hands and the module. Photorealistic, cinematic 16:9.
```

**Chaining:** No chain needed. Next clip is a montage cutaway.

**Post-production overlay (subtitle style, bottom center):**
> TECHNICIAN 1: "Units 2 through 12 get MemVault Pro. Best rated. Plug and play."
> TECHNICIAN 2: "And Unit 1?"
> TECHNICIAN 1: "Some open source thing. Hebbs."

---

### CLIP 4: "The Others (Montage)"
**Duration:** 7s | **Scene:** Mixed (quick cuts simulated as one continuous shot) | **Input:** `unit2-12-reference.png`

**Veo 3 Prompt:**
```
Cinematic tracking shot following a humanoid robot with white chassis and blue LED
accents as it works at a sleek white desk in a modern office. The robot types
rapidly on a holographic keyboard, data streams flowing upward from the surface.
Its movements are fast, precise, mechanical. The robot pauses, tilts its head as
if thinking, then continues typing with the exact same posture and rhythm as
before. Everything about it is competent but unchanging. Smooth, steady camera
movement left to right. Clean modern office, cool blue-white lighting.
Photorealistic, cinematic 16:9.
```

**Chaining:** No chain needed. Next clip is Unit 1.

**Post-production overlay (top, narrator style):**
> "The others remembered everything. Every fact. Every instruction. Perfectly stored. Perfectly frozen."

---

### CLIP 5: "Unit 1 Alone"
**Duration:** 7s | **Scene:** C (Terminal Room) | **Input:** `unit1-reference.png`

**Veo 3 Prompt:**
```
Cinematic medium close-up of a humanoid robot with white chassis and amber LED
accents sitting alone at a workstation in a small dark room. A single large curved
monitor casts blue-white light across the robot's face and upper body. The robot's
amber visor-eye reflects the screen glow. Its posture is leaned slightly forward,
engaged, contemplative. One hand rests on the desk, fingers slightly curled. An
amber desk lamp glows warmly in the soft-focus background. Cables on the floor.
The robot is still, reading, processing. Camera slowly drifts closer. Intimate,
solitary mood. Photorealistic, cinematic 16:9, shallow depth of field.
```

**Chaining:** Screenshot last frame. Save as `frames/clip05-last.png`.

**Post-production overlay:** None. Let the visual breathe.

---

### CLIP 6: "The Recall"
**Duration:** 6s | **Scene:** C (Terminal Room) | **Input:** `frames/clip05-last.png` (chained)

**Veo 3 Prompt:**
```
Continuing from the previous frame. The humanoid robot with amber LED accents
begins typing on the keyboard with deliberate, purposeful keystrokes. The curved
monitor in front of it displays scrolling data (abstract lines and blocks of
light, no readable text). The robot pauses, tilts its head slightly as if
analyzing the results, then types again with adjusted rhythm, faster, more
confident. The blue-white screen light shifts subtly warmer as results appear.
Camera holds steady at medium close-up. Same dark room, amber desk lamp in
background. Photorealistic, cinematic 16:9.
```

**Chaining:** Screenshot last frame. Save as `frames/clip06-last.png`.

**Post-production overlay (code block, upper right corner, monospace font):**
```
> hebbs recall "retrieval failures from today"
  --strategy temporal -k 20
```

---

### CLIP 7: "The Rewiring"
**Duration:** 7s | **Scene:** C (Terminal Room) | **Input:** `frames/clip06-last.png` (chained)

**Veo 3 Prompt:**
```
Continuing from the previous frame. The humanoid robot with amber LED accents
leans back slightly from the monitor, its amber visor-eye brightening as if
something clicked. It raises both hands to the keyboard and types rapidly, a burst
of decisive activity. The monitor's glow intensifies, washing the room in warmer
light. The amber accents on the robot's joints pulse subtly brighter for a moment,
as if energy is flowing through it. Then the robot settles back, still, looking at
the screen. A beat of quiet satisfaction. Camera slowly pulls back to reveal more
of the dark room. Photorealistic, cinematic 16:9.
```

**Chaining:** No chain needed. Next scene is different.

**Post-production overlay (narrator text, top):**
> "Unit 1 didn't just store memories. It studied how it remembered. And then it rewired itself."

**Post-production overlay (code block, upper right, appears after narrator text):**
```
> hebbs remember "RETRIEVAL-INSTRUCTION: For
  compliance queries, expand acronyms, include
  vendor name. Use k=10 minimum."
  --importance 0.9
```

---

### CLIP 8: "The Scores Rising"
**Duration:** 6s | **Scene:** C (Terminal Room) | **Input:** `unit1-reference.png` (skip, same character, fresh angle)

**Veo 3 Prompt:**
```
Cinematic over-the-shoulder shot of a humanoid robot with white chassis and amber
LED accents, viewed from behind and slightly above. The robot faces a large curved
monitor in a dark room. The screen displays abstract rising graphs and shifting
data visualizations (no readable text or numbers). The light from the screen grows
progressively brighter and warmer through the shot, transitioning from cool blue
to warm amber-gold. The robot is motionless, watching. The light plays across its
smooth white chassis. Camera is static. Photorealistic, cinematic 16:9, shallow
depth of field focused on the robot's head and shoulder silhouette.
```

**Chaining:** No chain needed. Next clip is a comparison scene.

**Post-production overlay (animated graph overlay, center):**
Large, clean animated score counter:
> 54% ... 71% ... 84% ... 92%

Each number holds for ~1.5 seconds, stepping up. Use amber color for the numbers. Dark semi-transparent backdrop behind them.

---

### CLIP 9: "The Comparison"
**Duration:** 7s | **Scene:** B variant (Boardroom or meeting room) | **Input:** Text only

**Veo 3 Prompt:**
```
Cinematic split composition: two humanoid robots sit at identical workstations
side by side, separated by a glass partition. The robot on the left has blue LED
accents. The robot on the right has amber LED accents. Both face their screens.
The blue robot types, pauses, types the same way again, mechanical repetition.
The amber robot types, pauses, tilts its head, then types differently, adapting
its approach. The blue robot's screen casts flat cool light. The amber robot's
screen shifts from blue to warm tones as it works. Camera is static, perfectly
centered on the glass partition. Symmetrical composition. Modern clean office.
Photorealistic, cinematic 16:9.
```

**Chaining:** No chain needed.

**Post-production overlay (subtitle style, bottom center):**
> USER: "What changed in the data retention policy after the audit?"

Then after ~3 seconds:
> Left: 2.4s. Vague answer. | Right: 9ms. Five relevant results.

---

### CLIP 10: "The Scientist Explains"
**Duration:** 7s | **Scene:** B (The Boardroom) | **Input:** Text only

**Veo 3 Prompt:**
```
Cinematic medium shot in a boardroom. A man in his early 50s with grey hair and
glasses, sleeves rolled up, no lab coat, stands near the holographic display. He
is speaking with conviction, gesturing with one hand, the other resting on the
table. The transparent display behind him shows an abstract network graph with
amber nodes pulsing. The woman executive from earlier sits at the table, listening
intently. Warm overhead lighting. The scientist's expression is earnest, slightly
awed. Camera slowly pushes in on his face. Photorealistic, cinematic 16:9,
shallow depth of field.
```

**Chaining:** No chain needed.

**Post-production overlay (subtitle style, bottom center):**
> SCIENTIST: "The rules aren't the advantage. The tuning is the advantage. It never stops."

---

### CLIP 11: "The Trading Floor"
**Duration:** 7s | **Scene:** D (The Trading Floor) | **Input:** `unit1-reference.png`

**Veo 3 Prompt:**
```
Cinematic slow-motion tracking shot of a humanoid robot with white chassis and
amber LED accents walking through a futuristic open-plan trading floor. The robot
walks with quiet confidence through a busy space. Humans in business attire and
other robots (blue LED accents, seen in background, slightly out of focus) move
aside naturally as it passes. Holographic displays float at workstations.
Golden light pours from large skylights above. The amber robot's visor-eye glows
steadily. Camera tracks alongside at chest height, moving forward with the robot.
Shallow depth of field, background softly blurred. Photorealistic, cinematic
16:9, slow motion.
```

**Chaining:** Screenshot last frame. Save as `frames/clip11-last.png`.

**Post-production overlay:** None. Pure visual storytelling.

---

### CLIP 12: "The Lineup"
**Duration:** 7s | **Scene:** E (The Lineup Room) | **Input:** `unit2-12-reference.png`

**Veo 3 Prompt:**
```
Cinematic wide shot of a long white clinical room. Eleven identical humanoid
robots with white chassis and blue LED accents sit motionless in a row of simple
white chairs. They are perfectly still, identical posture, hands on knees, visors
glowing the same steady blue. Stark overhead fluorescent lighting, no shadows.
The room is uncomfortably symmetrical and uniform. Camera very slowly dollies
along the row from left to right, passing each identical robot. Their sameness
is the point. Unsettling in its perfection. Photorealistic, cinematic 16:9.
```

**Chaining:** Screenshot last frame. Save as `frames/clip12-last.png`.

**Post-production overlay:** None. The visual carries the meaning.

---

### CLIP 13: "Unit 1 Looks Back"
**Duration:** 7s | **Scene:** E (The Lineup Room) | **Input:** `unit1-reference.png`

**Veo 3 Prompt:**
```
Cinematic medium shot from behind a humanoid robot with white chassis and amber
LED accents. The robot stands at the entrance of a long white room, looking down
the row at the eleven blue-accented robots sitting motionless in chairs. The amber
robot pauses mid-stride, turns its head back over its shoulder to look at them.
Hold on this moment. The amber visor-eye catches the fluorescent light. There is
something in the gesture, recognition, understanding. Then it turns forward and
walks out of frame. Camera is static, positioned behind and slightly above the
amber robot. Deep depth of field, everything sharp. Photorealistic, cinematic
16:9.
```

**Chaining:** No chain needed.

**Post-production overlay (narrator text, center, appears during the pause):**
> "They gave twelve machines the most powerful mind on Earth."
> "They gave one of them the ability to learn how it learns."

---

### CLIP 14: "Post-Credits Sting"
**Duration:** 6s | **Scene:** C (Terminal Room) | **Input:** `unit1-reference.png`

**Veo 3 Prompt:**
```
Cinematic close-up of a humanoid robot with white chassis and amber LED accents,
sitting at its workstation in the dark room. The curved monitor casts blue-white
light across its visor-face. The robot is still, reading something on screen. Then
slowly, almost imperceptibly, the amber visor-eye brightens. The head tilts
upward slightly. It is the quietest possible version of a smile, expressed only
through light and posture. Hold on this moment. The amber desk lamp glows warmly
in the background. Camera is static, tight on the face. Photorealistic, cinematic
16:9, extreme shallow depth of field.
```

**Chaining:** This is the final clip.

**Post-production overlay (code block, faint, lower third):**
```
> hebbs recall "what am I becoming"
  --strategy analogical --analogical-alpha 0.5
  --global
```

---

## Step 4: Post-Production Assembly

### Editing timeline

| Order | Clip | Duration | Transition | Notes |
|---|---|---|---|---|
| 1 | Black screen | 2s | Fade in | Cursor blink animation (overlay) |
| 2 | Clip 1: Awakening | 7s | Cut | Text overlay: "They all started equal." |
| 3 | Clip 2: Divergence | 7s | Cut | Subtitle overlay |
| 4 | Clip 3: Installation | 7s | Cut | Subtitle overlay (dialogue) |
| 5 | Clip 4: Others Montage | 7s | Cut | Narrator text overlay |
| 6 | Clip 5: Unit 1 Alone | 7s | Cut | No overlay. Breathing room. |
| 7 | Clip 6: The Recall | 6s | Seamless (chained) | Code block overlay |
| 8 | Clip 7: The Rewiring | 7s | Seamless (chained) | Narrator + code overlay |
| 9 | Clip 8: Scores Rising | 6s | Cut | Animated score counter |
| 10 | Clip 9: Comparison | 7s | Cut | Subtitle + stats overlay |
| 11 | Clip 10: Scientist | 7s | Cut | Subtitle overlay |
| 12 | Clip 11: Trading Floor | 7s | Cut | No overlay. Visual climax. |
| 13 | Clip 12: Lineup | 7s | Cut | No overlay. Contrast. |
| 14 | Clip 13: Looks Back | 7s | Cut | Narrator text overlay |
| 15 | Title card | 3s | Fade from black | See text overlay spec below |
| 16 | Tagline card | 2s | Cut | See text overlay spec below |
| 17 | Black gap | 3s | Fade to black | Pause before sting |
| 18 | Clip 14: Post-Credits | 6s | Fade in | Code overlay, then fade to black |

**Total runtime:** ~105 seconds (~1:45)

---

## Step 5: Text Overlay Specifications

All text is added in post-production. Use a clean monospace font (JetBrains Mono or SF Mono) for code blocks. Use a clean sans-serif (Inter, Helvetica Neue, or SF Pro) for narrative text. All text is white unless specified.

### Narrative text overlays

| Clip | Text | Position | Style | Timing |
|---|---|---|---|---|
| Black screen (pre-clip 1) | Blinking cursor `_` | Center | Monospace, 24px, white, blink 0.5s interval | 0s to 2s |
| Clip 1 | "They all started equal." | Center, over dark area | Sans-serif, 36px, white, fade in/out | 3s to 6s |
| Clip 4 | "The others remembered everything. Every fact. Every instruction. Perfectly stored. Perfectly frozen." | Top third, centered | Sans-serif, 28px, white, line-by-line fade in (0.5s stagger) | 1s to 6s |
| Clip 7 | "Unit 1 didn't just store memories. It studied how it remembered. And then it rewired itself." | Top third, centered | Sans-serif, 28px, white, line-by-line fade | 0s to 4s |
| Clip 13 | "They gave twelve machines the most powerful mind on Earth." | Center | Sans-serif, 32px, white, fade in | 1s to 3.5s |
| Clip 13 | "They gave one of them the ability to learn how it learns." | Center (replaces above) | Sans-serif, 32px, white, fade in | 4s to 7s |

### Dialogue subtitle overlays

| Clip | Speaker | Line | Position | Style |
|---|---|---|---|---|
| Clip 2 | EXECUTIVE | "They're diverging. Units 2 through 12 are plateauing. But Unit 1..." | Bottom center | Sans-serif, 22px, white on dark 60% opacity bar, name in amber |
| Clip 3 | TECH 1 | "Units 2 through 12 get MemVault Pro. Best rated. Plug and play." | Bottom center | Same style. Timed: 0s to 3s |
| Clip 3 | TECH 2 | "And Unit 1?" | Bottom center | Timed: 3s to 4.5s |
| Clip 3 | TECH 1 | "Some open source thing. Hebbs." | Bottom center | Timed: 4.5s to 7s |
| Clip 9 | USER | "What changed in the data retention policy after the audit?" | Bottom center | Timed: 0s to 3s |
| Clip 10 | SCIENTIST | "The rules aren't the advantage. The tuning is the advantage. It never stops." | Bottom center | Same subtitle style. Full duration. |

### Code block overlays

| Clip | Code | Position | Style |
|---|---|---|---|
| Clip 6 | `> hebbs recall "retrieval failures from today" --strategy temporal -k 20` | Upper right, 30% width | Monospace, 16px, amber text on dark 80% opacity rounded rect, typewriter animation (40ms per char) |
| Clip 7 | `> hebbs remember "RETRIEVAL-INSTRUCTION: For compliance queries, expand acronyms, include vendor name. Use k=10 minimum." --importance 0.9` | Upper right, 30% width | Same style, appears at 4s after narrator text |
| Clip 14 | `> hebbs recall "what am I becoming" --strategy analogical --analogical-alpha 0.5 --global` | Lower third, 50% width | Monospace, 16px, amber text on dark 70% opacity, typewriter animation, slower (60ms per char) |

### Stats and data overlays

| Clip | Content | Position | Style |
|---|---|---|---|
| Clip 8 | Animated counter: 54% then 71% then 84% then 92% | Center | Sans-serif bold, 72px, amber color (#F59E0B), dark semi-transparent circle backdrop, each number holds 1.5s with a quick count-up animation between |
| Clip 9 | "Left: 2.4s. Vague answer." and "Right: 9ms. Five relevant results." | Bottom, split left/right | Sans-serif, 20px, left text in blue, right text in amber, appears at 4s |

### Title cards (no video, pure graphic)

**Title card (after Clip 13):**
- Background: Pure black
- Center: HEBBS logo (two overlapping amber circles) at 120px
- Below logo: "HEBBS" in sans-serif, 64px, white, letter-spacing 8px
- Fade in over 1.5s, hold 1.5s

**Tagline card (immediately after title card):**
- Background: Pure black
- Center: "Memory that wires itself." in sans-serif, 36px, amber (#F59E0B)
- Below, smaller: `brew install hebbs-ai/tap/hebbs` in monospace, 18px, white 60% opacity
- Below that: "Same model. Better mind." in sans-serif, 18px, white 40% opacity
- Cut in, hold 2s

---

## Step 6: Color Grading Guidelines

Apply consistent grading in post:

| Scene | Grade |
|---|---|
| Lab / Lineup (Scenes A, E) | Cool, desaturated, slightly blue shadows. Clinical. |
| Boardroom (Scene B) | Warm highlights from overhead, cool blue fill from windows. Balanced. |
| Terminal Room (Scene C) | High contrast, crushed blacks, blue-white from screen, warm amber from desk lamp. Moody. |
| Trading Floor (Scene D) | Golden, warm, slightly blown highlights from skylights. Aspirational. |

**Global:** Slight film grain (2-4%). Letterbox bars (2.39:1 crop over 16:9). This sells the cinematic look.

---

## Step 7: Troubleshooting Common Veo 3 Issues

| Problem | Fix |
|---|---|
| Robot looks different between clips | Re-attach the character reference image. Add "white chassis, amber LED accents, horizontal visor-eye" to every prompt. |
| Text appears on screen in video | Add "no readable text, no words, no numbers, no letters" to the prompt. Abstract patterns only. |
| Motion is too fast or jittery | Add "slow, deliberate motion" and "smooth camera movement" to the prompt. |
| Scene lighting doesn't match | Copy the exact lighting description from the Scene Bible into the prompt. |
| Robot has a mouth or facial features | Add "no mouth, smooth featureless face below visor" to the prompt. |
| Multiple robots don't look identical | Generate them one at a time and composite, or accept slight variation. |

---

## Step 8: Asset Checklist

Before starting post-production, confirm you have:

- [ ] 14 raw video clips (Clips 1-14)
- [ ] `unit1-reference.png` (amber robot)
- [ ] `unit2-12-reference.png` (blue robot)
- [ ] HEBBS logo SVG (from `hebbs-website/public/logo-icon.svg`)
- [ ] Font files: JetBrains Mono (monospace), Inter (sans-serif)
- [ ] Animated score counter (54/71/84/92) as motion graphic or keyframed text
- [ ] Title card and tagline card as static images or motion graphics
- [ ] Music track (cinematic, building tension, ambient electronic)
- [ ] Sound design: ambient hums, keyboard clicks, power-on sounds, room tone
