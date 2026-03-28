# HEBBS Enterprise: Dashboard Wireframes

All screens for the enterprise dashboard UI. Dark theme (near-black background, amber accents matching HEBBS brand).

**Tech stack:** Next.js (static export) + Tailwind CSS, served by Hono platform on port 8080.

**Color palette:**
- Background: `#0a0a0f`
- Cards/panels: `#14141f`
- Borders: `#1e1e2e`
- Primary accent: `#f59e0b` (amber)
- Primary hover: `#d97706`
- Text primary: `#e4e4e7`
- Text secondary: `#71717a`
- Success: `#22c55e`
- Error: `#ef4444`
- Code/mono bg: `#1a1a2e`

---

## Screen 0: Login

**Route:** `/login`
**API:** `POST /v1/auth/login`

```
+-----------------------------------------------------+
|                                                     |
|                                                     |
|                    OO  HEBBS                        |
|                  Enterprise                         |
|                                                     |
|            +----------------------------+            |
|            |                            |            |
|            |  Email                     |            |
|            |  [________________________]|            |
|            |                            |            |
|            |  Password                  |            |
|            |  [________________________]|            |
|            |                            |            |
|            |  [ Sign In               ] |            |
|            |                            |            |
|            +----------------------------+            |
|                                                     |
|            Forgot password? Contact admin.           |
|                                                     |
+-----------------------------------------------------+
```

Redirect logic:
- If onboarding not completed -> redirect to `/onboarding`
- If not logged in -> show login
- If logged in -> redirect to `/`

---

## Screen 1: Onboarding - Step 1 (Create Admin Account)

**Route:** `/onboarding`
**API:** `GET /v1/onboarding/status` (check), `POST /v1/onboarding` (on step 3)

```
+-----------------------------------------------------+
|  HEBBS Enterprise Setup                    Step 1/3 |
+---------+-------------------------------------------+
|         |                                           |
|  (1)    |  Welcome to HEBBS Enterprise              |
|  (2)    |                                           |
|  (3)    |  Create your admin account                |
|         |                                           |
|         |  Email        [________________________]  |
|         |  Password     [________________________]  |
|         |  Confirm      [________________________]  |
|         |                                           |
|         |                        [ Next -> ]        |
|         |                                           |
+---------+-------------------------------------------+
```

---

## Screen 2: Onboarding - Step 2 (Name Workspace)

**Route:** `/onboarding` (step 2)

```
+-----------------------------------------------------+
|  HEBBS Enterprise Setup                    Step 2/3 |
+---------+-------------------------------------------+
|         |                                           |
|  (1)    |  Name your first workspace                |
|  (2)    |                                           |
|  (3)    |  Workspace    [________________________]  |
|         |               e.g. "support-agent"        |
|         |                                           |
|         |               [ <- Back ]  [ Next -> ]    |
|         |                                           |
+---------+-------------------------------------------+
```

---

## Screen 3: Onboarding - Step 3 (Verify + Complete)

**Route:** `/onboarding` (step 3)
**API:** `POST /v1/onboarding`, `GET /v1/system/health`

```
+-----------------------------------------------------+
|  HEBBS Enterprise Setup                    Step 3/3 |
+---------+-------------------------------------------+
|         |                                           |
|  (1)    |  Verify Connection                        |
|  (2)    |                                           |
|  (3)    |  OpenAI API     * Connected (via .env)    |
|         |  Engine         * Healthy                 |
|         |  Workspace      * support-agent created   |
|         |                                           |
|         |  Your API Key:                            |
|         |  +---------------------------------------+|
|         |  | hb_live_sk_abc123...           [Copy] ||
|         |  +---------------------------------------+|
|         |  Save this key. It won't be shown again.  |
|         |                                           |
|         |               [ Go to Dashboard -> ]      |
|         |                                           |
+---------+-------------------------------------------+
```

---

## Screen 4: Dashboard Home

**Route:** `/`
**API:** `GET /v1/workspaces`, `GET /v1/system/health`

```
+-----------------------------------------------------+
|  OO HEBBS | Workspaces  Settings  Team     J.Smith v|
+-----------------------------------------------------+
|                                                     |
|  Workspaces                    [ + New Workspace ]  |
|                                                     |
|  +-----------------------------------------------+  |
|  |  support-agent                        * Ready |  |
|  |  Memories: 145   Files: 8   Entities: 23      |  |
|  |  Last activity: 2 min ago                     |  |
|  |  [ Open ]  [ Memory Palace ]  [ Keys ]        |  |
|  +-----------------------------------------------+  |
|                                                     |
|  +-----------------------------------------------+  |
|  |  sales-agent                          * Ready |  |
|  |  Memories: 12    Files: 0   Entities: 3       |  |
|  |  Last activity: 1 hour ago                    |  |
|  |  [ Open ]  [ Memory Palace ]  [ Keys ]        |  |
|  +-----------------------------------------------+  |
|                                                     |
|  System                                             |
|  +--------------+ +--------------+ +-------------+  |
|  | Engine       | | OpenAI       | | Storage     |  |
|  | * Healthy    | | * Connected  | | 2.4 GB / 50 |  |
|  | v0.3.4      | | gpt-4o-mini  | | ===-- 4.8%  |  |
|  +--------------+ +--------------+ +-------------+  |
|                                                     |
+-----------------------------------------------------+
```

---

## Screen 5: Workspace Detail - Overview Tab

**Route:** `/workspaces/:slug`
**API:** `GET /v1/workspaces/:slug/stats`, `GET /v1/workspaces/:slug/recall` (recent)

```
+-----------------------------------------------------+
|  OO HEBBS | <- Workspaces                  J.Smith v|
+-----------------------------------------------------+
|                                                     |
|  support-agent                                      |
|                                                     |
|  Overview   Files   Search   Entities   Keys        |
|  --------                                           |
|                                                     |
|  +----------+ +----------+ +----------+ +--------+  |
|  |   145    | |    8     | |   23     | |  444   |  |
|  | Memories | |  Files   | | Entities | | Edges  |  |
|  +----------+ +----------+ +----------+ +--------+  |
|                                                     |
|  Indexing: * Complete (8/8 files)                    |
|  Insights: 5 auto-generated profiles                |
|  Contradictions: 2 flagged                          |
|                                                     |
|  [ Open Memory Palace -> ]                          |
|                                                     |
|  Recent Memories                                    |
|  +-----------------------------------------------+  |
|  | * "Support fact: password reset via Settings" |  |
|  |   importance: 0.7  |  episode  |  2 min ago   |  |
|  |-----------------------------------------------|  |
|  | # "Users who reset passwords often also..."   |  |
|  |   confidence: 0.85 |  insight  |  auto-gen    |  |
|  |-----------------------------------------------|  |
|  | * "Ransomware coverage endorsement adds..."   |  |
|  |   importance: 0.5  |  from file |  10 min ago |  |
|  +-----------------------------------------------+  |
|                                                     |
+-----------------------------------------------------+
```

---

## Screen 6: Workspace Detail - Files Tab

**Route:** `/workspaces/:slug/files`
**API:** `GET /v1/workspaces/:slug/files`, `POST /v1/upload`

```
+-----------------------------------------------------+
|  OO HEBBS | <- Workspaces                  J.Smith v|
+-----------------------------------------------------+
|                                                     |
|  support-agent                                      |
|                                                     |
|  Overview   Files   Search   Entities   Keys        |
|             -----                                   |
|                                                     |
|  Files (8)                        [ Upload Files ]  |
|                                                     |
|  +------------------------------------------------+ |
|  |  File                    Sections  Status       | |
|  |------------------------------------------------| |
|  |  audits/vendor-risk-     12        * Indexed    | |
|  |    assessment-cloudvault.md                     | |
|  |------------------------------------------------| |
|  |  compliance/policy-      9         * Indexed    | |
|  |    data-retention-v1.md                         | |
|  |------------------------------------------------| |
|  |  compliance/policy-      10        * Indexed    | |
|  |    data-retention-v2.md                         | |
|  |------------------------------------------------| |
|  |  contracts/truenorth/    15        * Indexed    | |
|  |    cyber-insurance.md                           | |
|  |------------------------------------------------| |
|  |  meetings/2025-q2-       8         * Indexed    | |
|  |    insurance-review.md                          | |
|  +------------------------------------------------+ |
|                                                     |
|  +------------------------------------------------+ |
|  |  Drop files here or click Upload Files          | |
|  |  Supports: .md, .txt, .pdf                      | |
|  +------------------------------------------------+ |
|                                                     |
|  Indexing Progress                                  |
|  ============================  8/8 complete         |
|  Last indexed: 10 min ago                           |
|                                                     |
+-----------------------------------------------------+
```

---

## Screen 7: Workspace Detail - Search Tab

**Route:** `/workspaces/:slug/search`
**API:** `POST /v1/workspaces/:slug/recall`

```
+-----------------------------------------------------+
|  OO HEBBS | <- Workspaces                  J.Smith v|
+-----------------------------------------------------+
|                                                     |
|  support-agent                                      |
|                                                     |
|  Overview   Files   Search   Entities   Keys        |
|                     ------                          |
|                                                     |
|  +--------------------------------------+           |
|  | ransomware coverage limits      [Go] |           |
|  +--------------------------------------+           |
|                                                     |
|  Strategy: [ Similarity v]  k: [ 10 ]              |
|  Entity:   [ (all)       ]                          |
|                                                     |
|  10 results (734ms)                                 |
|                                                     |
|  +-----------------------------------------------+  |
|  | 1. score: 0.635                               |  |
|  | "The endorsement adds coverage for            |  |
|  |  professional ransomware negotiation           |  |
|  |  services."                                    |  |
|  | source: contracts/truenorth/endorsement-001   |  |
|  | importance: 0.5  |  decay: 0.92               |  |
|  |-----------------------------------------------|  |
|  | 2. score: 0.609                               |  |
|  | "The endorsement includes a requirement for   |  |
|  |  post-payment monitoring after a ransomware   |  |
|  |  event."                                       |  |
|  | source: contracts/truenorth/endorsement-001   |  |
|  | importance: 0.5  |  decay: 0.91               |  |
|  |-----------------------------------------------|  |
|  | 3. score: 0.591                               |  |
|  | "The maximum payable for ransomware payments  |  |
|  |  is $500,000 per incident."                    |  |
|  | source: contracts/truenorth/endorsement-001   |  |
|  | importance: 0.5  |  decay: 0.91               |  |
|  +-----------------------------------------------+  |
|                                                     |
+-----------------------------------------------------+
```

---

## Screen 8: Workspace Detail - Entities Tab

**Route:** `/workspaces/:slug/entities`
**API:** `GET /v1/workspaces/:slug/entities`, `GET /v1/workspaces/:slug/insights`

```
+-----------------------------------------------------+
|  OO HEBBS | <- Workspaces                  J.Smith v|
+-----------------------------------------------------+
|                                                     |
|  support-agent                                      |
|                                                     |
|  Overview   Files   Search   Entities   Keys        |
|                              --------               |
|                                                     |
|  Entities (23)                                      |
|                                                     |
|  +-----------------------------------------------+  |
|  |  Entity              Memories  Insights  Last |  |
|  |-----------------------------------------------|  |
|  |  compliance              34       2    10m ago |  |
|  |  > "Organizations must retain PII for min..." |  |
|  |  > "SOC2 Type II audit passed March 2026..."  |  |
|  |-----------------------------------------------|  |
|  |  truenorth-insurance     28       1    10m ago |  |
|  |  > "Cyber liability coverage up to $5M..."    |  |
|  |-----------------------------------------------|  |
|  |  risk-management         22       1    10m ago |  |
|  |  > "Critical risk: legacy auth system..."     |  |
|  |-----------------------------------------------|  |
|  |  cloudvault-vendor       18       1    10m ago |  |
|  |  > "Vendor risk score: MEDIUM (3.2/5)..."     |  |
|  +-----------------------------------------------+  |
|                                                     |
|  Click entity to view full profile and insights     |
|                                                     |
|  Insights (5 auto-generated)                        |
|  +-----------------------------------------------+  |
|  | # compliance: "Data retention policies have   |  |
|  |   evolved from 3-year to 7-year minimum..."   |  |
|  |   confidence: 0.85  |  based on 12 memories   |  |
|  |-----------------------------------------------|  |
|  | # truenorth-insurance: "Coverage has been     |  |
|  |   expanded with ransomware endorsement..."    |  |
|  |   confidence: 0.82  |  based on 8 memories    |  |
|  +-----------------------------------------------+  |
|                                                     |
+-----------------------------------------------------+
```

---

## Screen 9: Workspace Detail - Keys Tab

**Route:** `/workspaces/:slug/keys`
**API:** `GET /v1/keys`, `POST /v1/keys`, `DELETE /v1/keys/:id`

```
+-----------------------------------------------------+
|  OO HEBBS | <- Workspaces                  J.Smith v|
+-----------------------------------------------------+
|                                                     |
|  support-agent                                      |
|                                                     |
|  Overview   Files   Search   Entities   Keys        |
|                                         ----        |
|                                                     |
|  API Keys                              [ + New Key ]|
|                                                     |
|  +-----------------------------------------------+  |
|  |  hb_live_sk_abc1...   support-default         |  |
|  |  Created: Mar 28       Role: workspace         |  |
|  |                                    [ Revoke ] |  |
|  |-----------------------------------------------|  |
|  |  hb_live_sk_xyz9...   ci-pipeline             |  |
|  |  Created: Mar 27       Role: workspace         |  |
|  |                                    [ Revoke ] |  |
|  +-----------------------------------------------+  |
|                                                     |
|  Endpoint                                           |
|  +-----------------------------------------------+  |
|  |  http://44.201.xx.xx:8080              [Copy] |  |
|  +-----------------------------------------------+  |
|                                                     |
|  Quick Start                                        |
|  +-----------------------------------------------+  |
|  |  curl -X POST http://...:8080/v1/recall \     |  |
|  |    -H "Authorization: Bearer hb_live_sk_..." \|  |
|  |    -H "Content-Type: application/json" \      |  |
|  |    -d '{"cue": "your query"}'                 |  |
|  +-----------------------------------------------+  |
|                                                     |
+-----------------------------------------------------+
```

---

## Screen 10: Settings

**Route:** `/settings`
**API:** `GET /v1/config`, `PUT /v1/config`

```
+-----------------------------------------------------+
|  OO HEBBS | Workspaces  Settings  Team     J.Smith v|
+-----------------------------------------------------+
|                                                     |
|  Settings                                           |
|                                                     |
|  LLM Configuration                                  |
|  +-----------------------------------------------+  |
|  | Provider     [ openai          v ]            |  |
|  | Model        [ gpt-4o-mini       ]            |  |
|  | API Key      [ ****************  ] (from .env)|  |
|  |              [ Test Connection ]  * Connected  |  |
|  +-----------------------------------------------+  |
|                                                     |
|  Embeddings                                         |
|  +-----------------------------------------------+  |
|  | Provider     [ openai          v ]            |  |
|  | Model        [ text-embedding-3-small ]       |  |
|  | Dimensions   1536 (fixed)                     |  |
|  +-----------------------------------------------+  |
|                                                     |
|  Engine                                             |
|  +-----------------------------------------------+  |
|  | Concurrency  [--*------] 10                   |  |
|  |              max parallel API requests         |  |
|  |                                               |  |
|  | Decay        [x] Enabled                      |  |
|  | Half-life    [------*--] 30 days              |  |
|  |                                               |  |
|  | Reflection   [ ] Enabled                      |  |
|  | Interval     [--*------] 1 hour               |  |
|  |                                               |  |
|  | Contradictions [x] Enabled                    |  |
|  +-----------------------------------------------+  |
|                                                     |
|  Deployment                                         |
|  +-----------------------------------------------+  |
|  | Deployment ID   acme-corp-001                 |  |
|  | Heartbeat       [x] Enabled                   |  |
|  | Last heartbeat  2 min ago                     |  |
|  | Central URL     https://central.hebbs.ai      |  |
|  +-----------------------------------------------+  |
|                                                     |
|                              [ Save Changes ]       |
|                                                     |
+-----------------------------------------------------+
```

---

## Screen 11: Team Management

**Route:** `/team`
**API:** `GET /v1/accounts`, `POST /v1/accounts`, `PUT /v1/accounts/:id`, `DELETE /v1/accounts/:id`

```
+-----------------------------------------------------+
|  OO HEBBS | Workspaces  Settings  Team     J.Smith v|
+-----------------------------------------------------+
|                                                     |
|  Team                              [ + Add Member ] |
|                                                     |
|  +-----------------------------------------------+  |
|  |  User            Role       Workspaces        |  |
|  |-----------------------------------------------|  |
|  |  admin@acme.com  Admin      All               |  |
|  |                                               |  |
|  |-----------------------------------------------|  |
|  |  dev1@acme.com   Developer  support-agent     |  |
|  |                             sales-agent       |  |
|  |                             [ Edit ] [Remove] |  |
|  |-----------------------------------------------|  |
|  |  dev2@acme.com   Developer  support-agent     |  |
|  |                             [ Edit ] [Remove] |  |
|  +-----------------------------------------------+  |
|                                                     |
+-----------------------------------------------------+
```

### Add Member Modal

```
         +----------------------------------+
         |  Add Team Member                 |
         |                                  |
         |  Email    [____________________] |
         |  Password [____________________] |
         |                                  |
         |  Role     [ Developer      v ]   |
         |                                  |
         |  Workspaces                      |
         |  [x] support-agent              |
         |  [ ] sales-agent                |
         |                                  |
         |  [ Cancel ]       [ Add ]        |
         +----------------------------------+
```

---

## Screen 12: Memory Palace

**Route:** `/workspaces/:slug/palace`
**API:** `/api/panel/graph`, `/api/panel/recall` (proxied to engine)

```
+-----------------------------------------------------+
|  OO HEBBS | <- support-agent | Memory Palace        |
+-----------------------------------------------------+
|  +-----------------------------------------+        |
|  |                                         |        |
|  |           o---o                         |        |
|  |          /     \        o               |        |
|  |     o---o       o------+               |        |
|  |      \   \     / \     o               |        |
|  |       o   o---o   o                    |        |
|  |        \       \   \        o--o       |        |
|  |    o----o       o   o------/    \      |        |
|  |          \       \ /      o      o     |        |
|  |           o-------o        \    /      |        |
|  |                    \        o--o       |        |
|  |                     o                  |        |
|  |                                         |        |
|  +-----------------------------------------+        |
|                                                     |
|  +----------------------------+  Selected Node:     |
|  | Search memories...    [?] |  +------------------+|
|  +----------------------------+  | "Ransomware     ||
|                                  |  coverage limit  ||
|  Filter:                         |  is $500k per    ||
|  [ ] Episodes                    |  incident"       ||
|  [ ] Insights                    |                  ||
|  [ ] From files                  |  score: 0.5      ||
|                                  |  source: contrac ||
|  Nodes: 144  Edges: 444         |  edges: 3        ||
|                                  +------------------+|
+-----------------------------------------------------+
```

---

## Screen 13: New Workspace Modal

**Triggered from:** Dashboard Home "New Workspace" button
**API:** `POST /v1/workspaces`

```
         +----------------------------------+
         |  Create Workspace                |
         |                                  |
         |  Name   [____________________]   |
         |  Slug   [____________________]   |
         |         (auto-generated)         |
         |                                  |
         |  [ Cancel ]    [ Create ]        |
         |                                  |
         |  A new API key will be           |
         |  generated for this workspace.   |
         +----------------------------------+
```

---

## Navigation Structure

```
/ (Dashboard Home)
  /login
  /onboarding
  /workspaces/:slug (Overview tab)
  /workspaces/:slug/files
  /workspaces/:slug/search
  /workspaces/:slug/entities
  /workspaces/:slug/keys
  /workspaces/:slug/palace (Memory Palace)
  /settings
  /team
```

## API Dependencies per Screen

| Screen | APIs Used |
|--------|-----------|
| Login | `POST /v1/auth/login` |
| Onboarding | `GET /v1/onboarding/status`, `POST /v1/onboarding`, `GET /v1/system/health` |
| Dashboard Home | `GET /v1/workspaces`, `GET /v1/system/health` |
| Workspace Overview | `GET /v1/workspaces/:slug/stats` |
| Workspace Files | `GET /v1/workspaces/:slug/files`, `POST /v1/upload` |
| Workspace Search | `POST /v1/workspaces/:slug/recall` |
| Workspace Entities | `GET /v1/workspaces/:slug/entities`, `GET /v1/workspaces/:slug/insights` |
| Workspace Keys | `GET /v1/keys`, `POST /v1/keys`, `DELETE /v1/keys/:id` |
| Settings | `GET /v1/config`, `PUT /v1/config` |
| Team | `GET /v1/accounts`, `POST /v1/accounts`, `PUT /v1/accounts/:id`, `DELETE /v1/accounts/:id` |
| Memory Palace | `/api/panel/graph`, `/api/panel/recall` (engine proxy) |
| New Workspace | `POST /v1/workspaces` |
