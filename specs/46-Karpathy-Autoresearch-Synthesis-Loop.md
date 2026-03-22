# Spec 46: Karpathy Autoresearch Pattern for Tool Synthesis

**Status:** ACTIVE 2026-03-20. Replaces the naive "retry 3x on failure" synthesis loop in Spec 32 §7.
**Depends on:** Spec 32 (pipeline), Spec 43 (OpenCode interface), Spec 45 (web tools + GPT Researcher)

---

## 0. The Core Insight

Karpathy's autoresearch reduces to three primitives:

| Primitive | Karpathy (ML training) | Claw (tool synthesis) |
|---|---|---|
| **One editable file** | `train.py` — the model code | `generated/tools/ToolName.ts` |
| **One skill spec** | `program.md` — instructions, constraints, stopping criteria | `generated/specs/tools/ToolName.md` |
| **One scalar metric** | `val_bpb` — validation bits per byte | synthesis score (0.0 → 1.0) |

The loop is identical regardless of domain:
1. Read the skill spec
2. Form a hypothesis about one improvement
3. Make ONE atomic change to the editable file
4. Measure the scalar metric
5. If improved → commit (keep)
6. If worse → revert (discard)
7. Repeat until metric = 1.0 or max iterations

**Applied to Claw:** instead of "write the whole tool implementation and hope it works, retry 3x on failure" — the synthesis loop writes an initial implementation, measures it against contract tests, and makes ONE focused targeted change per iteration, converging toward a passing implementation. The spec markdown IS the program.md. The tool TypeScript IS the train.py.

This is what "gradient descent toward the best code artifact" means. Not metaphorically — the loop literally descends the error surface defined by the test metric.

---

## 1. The Synthesis Score (Scalar Metric)

The scalar metric must be: mechanical, directional (higher = better), verifiable, and extractable from command output.

```
synthesis_score = (test_pass_rate × 0.70)
               + (tsc_pass        × 0.20)
               + (security_clean  × 0.10)
```

Where:
- `test_pass_rate`: fraction of contract test assertions passing (0.0 – 1.0)
- `tsc_pass`: 1.0 if TypeScript compiles without errors, 0.0 if not
- `security_clean`: 1.0 if no blocking security findings, else (1.0 - findings/10)

A score of **1.0 = done**. All tests pass, TypeScript compiles, no security issues.
A score of **0.0 = broken**. Nothing works.

The score is calculated by `claw build` after each synthesis iteration and stored in `.claw-cache/synthesis/<hash>/score_history.tsv`:

```tsv
iteration  score   tsc_pass  test_pass  security  change_summary
0          0.42    1.0       0.35       1.0       initial synthesis
1          0.63    1.0       0.60       1.0       fixed empty url field
2          0.84    1.0       0.85       0.9       added error handling
3          1.00    1.0       1.00       1.0       fixed confidence range
```

---

## 2. The Skill Spec: `generated/specs/tools/ToolName.md`

This IS Karpathy's `program.md`. It carries three registers simultaneously:

### Register 1 — Instructions (what to implement)
```markdown
## Goal
Implement `WebSearch(inputs: { query: string }) -> SearchResult`.

## Implementation Pattern
Use DuckDuckGo JSON API (no key required).
URL: `https://api.duckduckgo.com/?q={query}&format=json&no_html=1`
Fall back to StealthyFetcher if standard HTTP fails.
```

### Register 2 — Constraints (what must not change)
```markdown
## Invariants (DO NOT CHANGE)
- Export name: `WebSearch` (exact)
- Input type: `{ query: string }` (exact)
- Return type: `SearchResult` with fields: url (string), snippet (string), confidence_score (float 0-1)
- No eval(), no hardcoded credentials
- Import types from `../types.js`
```

### Register 3 — Stopping criteria (when done)
```markdown
## Done When
- `url` is non-empty for query "rust language"
- `snippet` is non-empty for query "rust language"
- `confidence_score` is in range [0.0, 1.0]
- TypeScript compiles with `tsc --noEmit`
- synthesis_score >= 1.0
```

The compiler generates this file automatically from the `.claw` tool declaration. The user can also edit it manually — if the spec file is newer than the source `.claw`, it takes precedence (Spec 45 §3.2 N2).

---

## 3. The Autoresearch Synthesis Loop

### 3.1 Full loop

```
┌─────────────────────────────────────────────────────────────────┐
│  FOR EACH tool with using: in .claw file                        │
│                                                                 │
│  1. READ   generated/specs/tools/ToolName.md  (skill spec)      │
│                                                                 │
│  2. INIT   if generated/tools/ToolName.ts does not exist:       │
│            → call OpenCode: "Write an initial implementation    │
│              according to this skill spec. Focus on correctness │
│              over completeness."                                │
│            → measure score → set as baseline                    │
│                                                                 │
│  3. LOOP   while score < 1.0 and iteration < max_iterations:    │
│                                                                 │
│     a. READ  score_history.tsv — what was tried, what worked    │
│     b. READ  last failing test output — what is still broken    │
│     c. CALL  OpenCode:                                          │
│              "Current score: {score}. Failing test: {failure}.  │
│               Make ONE focused change to fix this.              │
│               Do not rewrite the file — modify only what's      │
│               needed to fix the failing assertion."             │
│     d. MEASURE  new score                                       │
│     e. IF   new_score > old_score:                              │
│              → git commit (message: "synthesis: iter {n} +{Δ}") │
│              → update baseline                                  │
│        ELSE:                                                    │
│              → git revert (discard the change)                  │
│              → log as failed attempt                            │
│     f. IF   no improvement in last 3 iterations:               │
│              → break (plateau detected)                         │
│                                                                 │
│  4. DONE   if score >= 1.0 → accept, proceed to bundle          │
│            if score < 1.0  → emit E-SYN02 with full history     │
└─────────────────────────────────────────────────────────────────┘
```

### 3.2 What makes each iteration different from naive retry

| Naive retry (old) | Karpathy loop (new) |
|---|---|
| Full rewrite on failure | ONE focused change per iteration |
| Failure message = "it failed" | Failure message = exact failing assertion |
| 3 attempts max | N iterations (default 10, configurable) |
| No memory of what was tried | Git history = memory of all attempts |
| Random direction | Metric-guided direction |
| Accepts first passing attempt | Continues until score = 1.0 |
| Cannot distinguish partial progress | Score tracks incremental improvement |

### 3.3 The hypothesis formation prompt

Each iteration's OpenCode prompt is built from:

```
═══════════════════════════════════════════
SYNTHESIS ITERATION {n} of {max}
═══════════════════════════════════════════

SKILL SPEC:
{generated/specs/tools/ToolName.md contents}

CURRENT IMPLEMENTATION:
{generated/tools/ToolName.ts last 80 lines}

CURRENT SCORE: {score} / 1.0

WHAT IS STILL FAILING:
{failing test assertions — exact output from vitest}

WHAT HAS BEEN TRIED (from git log):
{last 5 commits and their score deltas}

INSTRUCTION:
Make exactly ONE targeted change to generated/tools/ToolName.ts
to fix the failing assertions above. Do not rewrite the file.
Do not change the function signature or export name.
Make the smallest change that could fix the specific failure.

After making the change, verify it compiles:
  npx tsc --noEmit generated/tools/ToolName.ts

Then output EXACTLY:
SYNTHESIS_ITERATION_COMPLETE: ToolName
CHANGE_SUMMARY: <one sentence describing what you changed>
═══════════════════════════════════════════
```

The git history inside the prompt IS Karpathy's insight — it tells the agent what was already tried, what worked, what didn't. This is the "memory" that guides the hypothesis toward untried directions.

---

## 4. Predicting the Code for a Declared Tool

The user's key question: **can we predict the code needed to implement the tool being called dynamically by the `.claw` language?**

Yes — the Karpathy pattern + NER enrichment (Spec 44) + the skill spec (this spec) together form a prediction system:

### 4.1 What the prediction input is

From the `.claw` declaration:
```
tool WebSearch(query: string) -> SearchResult {
    using: fetch
    // Searches DuckDuckGo. Returns top result URL and snippet.
    test {
        input:  { query: "rust language" }
        expect: { url: !empty, snippet: !empty, confidence_score: { range: [0, 1] } }
    }
}
```

The compiler extracts:
- **Signature** — exact TypeScript function signature to generate
- **Capability** — `fetch` → HTTP, no browser
- **Return type schema** — field names, types, constraints (from the type declaration)
- **Test cases** — concrete input/output pairs
- **NER entities** — URLs, API names, env vars from comments (Spec 44)
- **User variables** — which workflow args flow into this tool (Spec 45 §3.4)

### 4.2 What the prediction produces

The skill spec (`program.md` equivalent) encodes all of this as a structured markdown document. This is NOT a prompt — it is a **specification** that happens to be readable both by humans and by the synthesis agent.

The synthesis agent reads the spec and **predicts** the implementation — what library to use, what API to call, what error handling is needed — based on the constraints in the spec. The autoresearch loop then **validates and refines** this prediction against the scalar metric.

```
Declaration → Skill Spec → Initial Prediction → Metric Measurement → Refinement Loop → Best Code
    .claw       .md          OpenCode iter 0       vitest score         Karpathy loop    .ts
```

### 4.3 The prediction is constrained, not freeform

The agent cannot predict outside the constraints because:
1. Export name is invariant → wrong name → 0 test pass rate → reverted
2. Return type is invariant → wrong fields → 0 test pass rate → reverted
3. TypeScript must compile → type errors → tsc_pass = 0 → reverted
4. Security constraints are invariant → flagged → security_clean < 1 → reverted

The loop **self-corrects** toward the correct implementation. The constraints in the skill spec are enforced mechanically by the metric, not by prompt following. This is why it's reliable.

---

## 5. Integrating GPT Researcher with the Loop

GPT Researcher (Spec 45 §2.4) feeds into the skill spec generation, not into the loop itself:

```
BEFORE the loop starts:
  GPT Researcher researches: "{tool_capability} {return_type} implementation Python 2026"
  → findings appended to skill spec under "## Implementation Research"
  → cached — never re-run for same (tool_hash, query)

DURING the loop:
  If score plateaus for 3 iterations:
    GPT Researcher researches: "why might {failing_assertion} fail in {capability} implementation"
    → findings injected into the next hypothesis prompt as "## Research on Failure Pattern"
```

This is where GPT Researcher's "gradient descent" role is precise: it researches failure modes, feeding better hypotheses into the loop. It doesn't run the loop — the loop uses the research to inform better atomic changes.

---

## 6. The Spec Markdown IS the Compiled `.claw` Output

The full compilation chain:

```
.claw source
    │
    ▼  Stage 1: Rust compiler
    │
    ├─ generated/types.ts                    (deterministic — no spec needed)
    │
    ├─ generated/specs/tools/WebSearch.md    ← THIS IS program.md
    │   ├─ ## Goal (from tool declaration)
    │   ├─ ## Invariants (from type system)
    │   ├─ ## Done When (from test {} blocks)
    │   ├─ ## User Variables (traced from workflow args)
    │   ├─ ## Implementation Research (from GPT Researcher — added by Stage 1.5)
    │   └─ ## Attempt History (populated by synthesis loop — empty at Stage 1)
    │
    ├─ generated/specs/agents/Researcher.md
    ├─ generated/specs/workflows/FindPurse.md
    │
    ▼  Stage 2: Synthesis loop (Karpathy pattern)
    │
    ├─ generated/tools/WebSearch.ts          ← THIS IS train.py
    │   (modified atomically per iteration, git-tracked)
    │
    ▼  Stage 3: Contract tests (the scalar metric measurement)
    │
    ▼  Stage 4: Bundle
```

The spec markdown files are the intermediate representation between the `.claw` compiler and the synthesis agent. They are human-readable, editable, and version-controllable. The synthesis agent reads them, not the `.claw` source directly.

---

## 7. DSL: No New Syntax Required

The Karpathy loop is entirely in the synthesis pipeline infrastructure — no new `.claw` syntax is needed. The existing `using:`, `test {}`, and `synthesize { note: "..." }` declarations feed into the skill spec automatically.

The developer's experience:
1. Write `.claw` with `using:` and `test {}` blocks (already supported)
2. Run `claw build`
3. The loop runs — progress reported to stderr:
   ```
   [claw] synthesizing WebSearch...
     iter 0: score 0.42 → initial implementation written
     iter 1: score 0.63 → fixed empty url field
     iter 2: score 0.84 → added error handling
     iter 3: score 1.00 → done ✓
   [claw] WebSearch synthesized in 4 iterations (38s)
   ```
4. The developer gets a working implementation without writing any code

The developer can also:
- Edit `generated/specs/tools/WebSearch.md` to add hints ("use the AbstractURL field, not Results[0]")
- Run `claw build --resume` to continue the loop from where it left off
- Run `claw build --reset WebSearch` to start over from scratch

---

## 8. Configuration

```json
// claw.json
{
  "synthesis": {
    "loop": {
      "max_iterations":  10,
      "plateau_patience": 3,
      "target_score":    1.0,
      "commit_prefix":   "synthesis:",
      "timeout_ms":      300000
    },
    "research": {
      "enabled":         true,
      "on_plateau":      true,
      "cache_ttl_hours": 24
    }
  }
}
```

---

## 9. GAN Audit

### Gaps
- **G1:** Git operations (commit/revert) during synthesis dirty the project's git history. Synthesis should run in a git worktree or a temp branch (`claw-synthesis-<hash>`) and merge only the final result.
- **G2:** The score formula weights (0.70 / 0.20 / 0.10) are arbitrary. They need empirical calibration from real synthesis runs.
- **G3:** "Make ONE change" is an instruction to the model, not a hard constraint. The model may make multiple changes. The diff size should be bounded and checked — if the diff exceeds N lines, treat it as a full rewrite (which is allowed at iteration 0, disallowed at iteration > 0).

### Assumptions
- **A1:** OpenCode can make targeted, atomic edits to an existing file. Confirmed — OpenCode's `edit` tool makes precise line-level edits.
- **A2:** `git revert` is instant and clean. True for single-file changes in a synthesis branch.
- **A3:** The scalar metric is extractable from vitest output via regex. True — vitest emits structured JSON with `--reporter=json`.

### Downstream consequences
- **N1:** The synthesis loop produces a readable git history of exactly what was tried. This history becomes training data for a future fine-tuned synthesis model (Spec 32 §14 reference to fine-tuned model). Every successful synthesis run is a (skill_spec, iteration_history, final_code) training example.
- **N2:** Developer trust improves significantly. Instead of "the model wrote this code", the developer can `git log generated/tools/WebSearch.ts` and see exactly what was tried, what each change fixed, and why the final version was accepted.
- **N3:** The loop naturally produces the minimal correct implementation — it stops at score = 1.0, not at "more features". This prevents over-engineering in synthesized tools.
