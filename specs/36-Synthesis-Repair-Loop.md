# Spec 36: Synthesis Repair Loop

**Status:** Specced 2026-03-19. Defines the tiered feedback-driven repair loop that runs at `claw build` time when synthesized TypeScript fails compilation or unit tests. Adds `retry {}` config to `synthesizer {}`, two repair strategies (`repair` and `rewrite`), and a structured escalation path from compile-time to test-time failures.

---

## 1. Core Concept

The synthesis pipeline (Spec 32/33) currently produces TypeScript in a single shot: synthesize → validate → done or fail. When the LLM produces broken code, the only option is for the user to manually re-run `claw build` or wait for `claw tune` to improve the base prompt (Spec 35).

This spec fills the gap with a **build-time repair loop**: when synthesized code fails, the error output is fed back to the LLM as structured context for a targeted repair attempt. This mirrors the LLMLOOP and RepairAgent patterns from automated program repair research — compile diagnostics and test failure output are the most information-dense feedback signals available without running the code in production.

Two independent failure surfaces are handled in a fixed escalation order:

| Tier | Failure surface | Triggered by |
|---|---|---|
| 1 | Compile-time | `tsc --noEmit` exit code ≠ 0 |
| 2 | Test-time | vitest exits with ≥ 1 failed test |

Compile errors are always resolved first. Test-repair only runs on code that compiles — running vitest on non-compiling TypeScript is meaningless.

The repair loop is **entirely contained within `claw build`**. No new CLI command is needed. It is transparent to the user unless `--verbose` is set.

---

## 2. DSL Grammar

`retry {}` is an optional block inside `synthesizer {}`:

```
synthesizer <Name> {
    client      = <ClientName>
    temperature = <float>
    retry {
        max_attempts:          <int>        // total synthesis attempts including first (default: 1 = no retry)
        strategy:              repair       // repair | rewrite | repair_then_rewrite
        compile_repair_limit:  <int>        // max compile-repair attempts before escalating (default: 2)
        on_stuck:              rewrite      // what to do if no progress across 2+ consecutive attempts (default: rewrite)
        budget_usd:            <float>      // spend cap for repair calls per tool (default: 0.50)
    }
}
```

**Example — full config:**

```claw
synthesizer DefaultSynth {
    client      = MyClaude
    temperature = 0.1
    retry {
        max_attempts:         4
        strategy:             repair
        compile_repair_limit: 2
        on_stuck:             rewrite
        budget_usd:           0.50
    }
}
```

**Minimal config (compile repair only, 3 attempts):**

```claw
synthesizer DefaultSynth {
    client      = MyClaude
    temperature = 0.1
    retry {
        max_attempts: 3
    }
}
```

---

## 3. Repair Strategies

### 3.1 `repair`

The repair strategy sends the **broken code + error output + full synthesis context** to the LLM and asks for a targeted fix. The LLM sees the specific error and the code that produced it. This is the default and preferred strategy: it is faster, cheaper, and works well for most compile errors and many test failures.

Repair prompt (compile tier):

```
SYSTEM:
You are fixing TypeScript code. Output ONLY the corrected TypeScript file. Do not explain. Do not include markdown fences.

USER:
## Synthesis Target
Tool: <Name>(<args>) -> <ReturnType>

## Type Definitions
<all relevant type declarations from types.ts>

## Broken Code (Attempt <N>)
<the broken TypeScript>

## Compilation Errors
<raw tsc --noEmit output>

Fix all compilation errors. Preserve the tool's implementation intent. Output only the corrected TypeScript.
```

Repair prompt (test tier):

```
SYSTEM:
You are fixing TypeScript code that fails unit tests. Output ONLY the corrected TypeScript file. Do not explain.

USER:
## Synthesis Target
Tool: <Name>(<args>) -> <ReturnType>

## Type Definitions
<all relevant type declarations from types.ts>

## Code (passes tsc, fails tests)
<the TypeScript code>

## Test Failures
<raw vitest output — failed test names, expected vs actual values>

Fix the code to pass the failing tests. Preserve working behavior. Output only the corrected TypeScript.
```

### 3.2 `rewrite`

The rewrite strategy discards the broken code entirely and issues a **fresh synthesis request** using the original prompt template (with no broken code in context). This breaks any feedback loop where the LLM keeps anchoring to the same broken pattern. Used as `on_stuck` fallback or when `strategy: rewrite` is explicitly set.

Rewrite prompt:

```
SYSTEM: <original synthesis system prompt>

USER:
## Synthesis Target
Tool: <Name>(<args>) -> <ReturnType>

## Type Definitions
<all relevant type declarations>

## Note
Previous synthesis attempts failed. This is a fresh attempt — generate a completely new implementation.
```

### 3.3 `repair_then_rewrite`

Repair for all attempts up to `max_attempts - 1`, then a clean rewrite as the final attempt. This ensures the last chance is always unanchored from broken prior code.

---

## 4. Tiered Escalation

The loop follows a fixed sequence per synthesis attempt:

```
attempt = 1
current_code = synthesize(tool, original_prompt)    // Tier 0: cold synthesis

loop:
    tsc_result = run_tsc(current_code)
    if tsc_result.ok:
        vitest_result = run_vitest(current_code)
        if vitest_result.ok:
            → success, write to generated/tools/<Name>.ts
        else:
            if attempt >= max_attempts → abort with E-R01
            if is_stuck(vitest_result, prev_vitest_result) → current_code = rewrite(tool)
            else → current_code = repair(current_code, vitest_result, tier=test)
    else:
        if attempt >= max_attempts → abort with E-R01
        if compile_repairs_used >= compile_repair_limit:
            if strategy includes rewrite → current_code = rewrite(tool)
            else → abort with E-R02 (compile limit reached, no rewrite fallback)
        elif is_stuck(tsc_result, prev_tsc_result) → current_code = rewrite(tool)
        else → current_code = repair(current_code, tsc_result, tier=compile)

    prev_tsc_result    = tsc_result
    prev_vitest_result = vitest_result
    attempt++
```

### 4.1 Escalation rules

| Condition | Action |
|---|---|
| Attempt 1 always | Cold synthesis (original prompt, no error context) |
| tsc fail, compile_repairs_used < compile_repair_limit | Compile repair |
| tsc fail, compile_repairs_used >= compile_repair_limit | Rewrite (if available) or E-R02 |
| tsc pass, vitest fail | Test repair |
| Any failure + stuck detected | Rewrite (via `on_stuck`) |
| max_attempts reached | Abort with E-R01, leave stub in place |

### 4.2 Stuck detection

"Stuck" means no meaningful progress between consecutive attempts of the same tier:

- **Compile stuck**: the set of unique `tsXXXX` error codes in attempt N is a superset of or identical to attempt N-1. If fixing one error introduced a new one of a different kind (net error count changed), that is not stuck — that is progress.
- **Test stuck**: the exact set of failing test names in attempt N matches attempt N-1.

Stuck detection triggers the `on_stuck` fallback (default: `rewrite`) instead of continuing to repair the same broken anchor.

---

## 5. Full Synthesis Context in Repair Prompts

A critical requirement from repair research: the LLM cannot fix code it cannot contextualize. Every repair call includes:

1. **Tool signature** — `<Name>(<arg>: <Type>, ...) -> <ReturnType>`
2. **All referenced type definitions** — the same types that were in the original synthesis prompt (from `generated/types.ts` or inline from the artifact)
3. **Capability hint** — the `using:` capability (e.g., `fetch`, `sandbox(Name)`) so the LLM knows what runtime APIs are available
4. **Broken code** — the full TypeScript file, not just the line with the error
5. **Error output** — verbatim, untruncated (up to 8000 chars; truncate at token limit with a note)
6. **Attempt count** — `Attempt N of M` in the header so the LLM knows how many tries remain

Items 1–4 are identical across all repair attempts for the same tool. Items 5–6 vary per attempt.

---

## 6. Progress Detection and Context Growth

Each repair attempt adds ~100–300 lines of TypeScript + error output to the conversation. By attempt 4, the context is substantial. Two safeguards:

**6.1 Independent calls, no conversation history**

Each repair call is a fresh API call (not a multi-turn conversation). The repair prompt is fully self-contained: prior broken code versions are NOT included — only the most recent broken code and its errors. This keeps token cost predictable and prevents the LLM from anchoring to a chain of broken attempts.

**6.2 Error truncation**

`tsc` can emit hundreds of error lines for cascading type errors. The repair prompt includes:
- All unique `tsXXXX` error code lines (deduplicated by code + message prefix)
- Up to 20 error lines total
- If truncated: append `(... N more errors not shown — fix the errors above first)`

Cascade errors usually resolve once the root error is fixed. Feeding 200 error lines to the LLM is counterproductive.

---

## 7. Cost Tracking

Each repair call costs money. `retry.budget_usd` caps spend per tool per build:

```
repair_cost_estimate = (input_tokens / 1_000_000) × input_price
                     + (output_tokens / 1_000_000) × output_price
```

`claw build` tracks cumulative repair spend per tool. If the running total would exceed `budget_usd` before the next attempt, skip the attempt and report the budget limit as the reason for E-R01.

Budget is per-tool, not per-build. A build with 5 tools each capped at $0.50 can spend up to $2.50 total on repairs.

Default: `budget_usd = 0.50` (~50 repair calls at haiku pricing).

---

## 8. Output During Build

By default, repair attempts are silent. With `claw build --verbose`:

```
[synth] WebSearch ... attempt 1/4 FAIL (tsc: 2 errors)
[synth] WebSearch ... attempt 2/4 (compile repair) FAIL (tsc: 1 error)
[synth] WebSearch ... attempt 3/4 (compile repair) PASS (tsc ok) FAIL (vitest: 1 test)
[synth] WebSearch ... attempt 4/4 (test repair) PASS
[synth] WebSearch ... done (4 attempts, ~$0.03)
```

Without `--verbose`, only failures that exhaust all attempts are printed:

```
error E-R01: WebSearch synthesis failed after 4 attempts
  last failure: vitest — 'contract: url matches SearchResult schema' (expected truthy, got undefined)
  see: generated/__repair__/WebSearch/attempt-4.ts for the last code produced
```

The last-attempt code is always saved to `generated/__repair__/<ToolName>/attempt-<N>.ts` regardless of outcome, so the user can inspect it.

---

## 9. Integration with Spec 33 Telemetry

Every attempt (success or failure) writes a telemetry record to `~/.claw/synthesis-telemetry/` with the Spec 33 schema. The `attempt_number` field (previously unused) is now populated:

```json
{
  "tool": "WebSearch",
  "prompt": "<synthesis or repair prompt>",
  "output": "<synthesized TypeScript>",
  "quality": {
    "tests_passed": false,
    "attempt_number": 3,
    "repair_tier": "test",
    "repair_strategy": "repair",
    "test_results": [
      { "name": "contract: url matches SearchResult schema", "passed": false, "error": "expected undefined to be truthy" }
    ],
    "tsc_errors": [],
    "spend_usd": 0.0021
  }
}
```

New fields: `repair_tier` (`compile` | `test` | `cold`), `repair_strategy` (`repair` | `rewrite` | null), `tsc_errors` (array of error lines), `spend_usd`.

Successful repairs where `attempt_number > 1` are especially valuable training data: they capture the exact error + fix pair that constitutes a real repair example.

---

## 10. Integration with Spec 35 (`claw tune`)

`claw tune` already runs synthesis loops offline. It does NOT use the repair loop — tune runs are intentionally single-shot to measure prompt quality. Repair would mask the true synthesis pass rate, defeating the purpose of scoring.

The `retry {}` config on a synthesizer is ignored by `claw tune`. Tune always uses `max_attempts = 1`.

---

## 11. New AST Nodes

```rust
// In ast.rs — add to SynthesizerDecl:
pub retry: Option<RetryConfig>,

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RetryConfig {
    pub max_attempts:         Option<u32>,
    pub strategy:             Option<RetryStrategy>,
    pub compile_repair_limit: Option<u32>,
    pub on_stuck:             Option<RetryStrategy>,
    pub budget_usd:           Option<f64>,
    pub span:                 Span,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum RetryStrategy {
    Repair,
    Rewrite,
    RepairThenRewrite,
}
```

`SynthesizerDecl` gains `pub retry: Option<RetryConfig>`.

---

## 12. New Error/Warning Codes

| Code | Name | Trigger |
|---|---|---|
| E-R01 | SynthesisExhausted | All repair attempts failed — synthesis did not produce valid code |
| E-R02 | CompileLimitNoRewrite | `compile_repair_limit` reached, `strategy` does not include rewrite, tsc still fails |
| W-R01 | RetryIgnoredByTune | `retry {}` config present but ignored because `claw tune` always uses single-shot synthesis |
| W-R02 | InvalidRetryConfig | `compile_repair_limit >= max_attempts` — no room for test repair after compile repair |
| W-R03 | RepairBudgetLow | `budget_usd < 0.10` — may not cover even one repair attempt |

---

## 13. Spec-Check Amendments (Spec 37 criteria)

### 13.0 Parser note: `repair_then_rewrite` keyword ambiguity

The winnow parser must try `repair_then_rewrite` before `repair` — both share the `repair` prefix. Use `alt()` with longest-match ordering:

```rust
fn retry_strategy(input: &mut Input<'_>) -> PResult<RetryStrategy> {
    alt((
        lexeme("repair_then_rewrite").map(|_| RetryStrategy::RepairThenRewrite),
        lexeme("repair").map(|_| RetryStrategy::Repair),
        lexeme("rewrite").map(|_| RetryStrategy::Rewrite),
    )).parse_next(input)
}
```

`repair_then_rewrite` must always be the first branch.

### 13.0b Codegen outputs — files changed by this spec

| File | Change |
|---|---|
| `generated/artifact.clawa.json` | Synthesizer section gains `retry` object per §4 |
| `generated/synth-runner.js` | Gains `buildPrompt()` with repair branch, `truncateTscErrors()` per §5 |
| `generated/__repair__/<ToolName>/attempt-N.ts` | Written at build time (diagnostic only) |
| `~/.claw/repair-history/<ToolName>/<ts>/` | Written on E-R01 only |

Rust files that change: `src/ast.rs`, `src/parser.rs`, `src/semantic/mod.rs`, `src/errors.rs`, `src/codegen/artifact.rs`, `src/codegen/synth_runner.rs`, `src/bin/claw.rs`, `src/codegen/mod.rs`.

### 13.0c Key function signatures

```rust
// src/bin/claw.rs
fn synthesize_with_repair(
    tool: &ToolDecl,
    document: &Document,
    project_root: &Path,
    verbose: bool,
) -> Result<String, SynthesisError>

fn resolve_compile_limit(retry: &RetryConfig) -> u32

fn effective_strategy(strategy: &RetryStrategy, attempt: u32, max_attempts: u32) -> RetryStrategy

fn collect_repair_types<'a>(tool: &ToolDecl, document: &'a Document) -> Vec<&'a TypeDecl>

fn estimate_spend(request: &Value, output: &str, synth: &SynthesizerDecl) -> f64

fn truncate_tsc_errors(raw: &str) -> String   // §13.12 algorithm
```

### 13.0d Offline behavior — `tsc` not installed

If `tsc --noEmit` returns an `ENOENT` / command-not-found error (not a non-zero TypeScript exit code), `claw build` emits `W-R04: TscNotFound`:

```
warning W-R04: tsc not found — tsc: criteria in eval{} and tsc repair tier disabled
  install: npm install -g typescript
```

When `tsc` is not installed: the `tsc` repair tier is skipped (any failed `tsc:` eval criterion is reported as "SKIP: tsc not installed"). Build continues. The repair loop only attempts test repair if vitest reports failures.

---

## 14. GAN Audit Amendments (R1–R2)

See `36-GAN-Audit.md` for the full finding log. 12 gaps found and fixed.

### 13.1 Transitive type closure in repair context (B1-01)

The repair prompt includes all types reachable by transitive closure from the tool's signature:

1. Start with the tool's return type and all argument types.
2. For each type name: look it up in `document.types`, collect its field types.
3. Repeat recursively until no new names are added (cycle-safe).
4. Emit all collected `TypeDecl`s in declaration order.
5. Cap at 30 type declarations (breadth-first order if exceeded).

### 13.2 `compile_repair_limit` default and clamping (B1-02)

The default `compile_repair_limit` is `max_attempts - 1` (not a hardcoded 2). The compiler resolves:

```
resolved_compile_limit = min(
    user_compile_repair_limit ?? (max_attempts - 1),
    max_attempts - 1
)
```

If the user sets `compile_repair_limit >= max_attempts`, `W-R02` fires and the value is clamped. By default all repair attempts are compile-repair; test repair requires explicitly setting `compile_repair_limit < max_attempts - 1`.

### 13.3 Rewrite always uses default synthesis template (B1-03)

`on_stuck: rewrite` and `strategy: repair_then_rewrite` both use the **default synthesis template** (Spec 33 §3), not any Spec 35 `tune_prompt` override. The intent of rewrite is to break anchoring to the approach that produced broken code. Using the same tuned prompt that failed defeats this.

### 13.4 Stuck detection is net-progress based (B1-04)

Compile tier stuck: `error_count(attempt_N) >= error_count(attempt_N-1)`.
Test tier stuck: `passing_count(attempt_N) <= passing_count(attempt_N-1)`.

Lateral moves (fix one error, introduce another of the same count) trigger stuck. Decreasing total error count is always progress.

### 13.5 E-R01 / E-R02 error hierarchy (B1-05)

`E-R01` (SynthesisExhausted) is the primary build error. `E-R02` is an informational note appended to E-R01 when compile_repair_limit was reached with no rewrite fallback. Output:

```
error E-R01: WebSearch synthesis failed after 4 attempts
note  E-R02: compile_repair_limit=4 reached, strategy=repair has no rewrite fallback
  → consider: strategy: repair_then_rewrite, or increase max_attempts
```

### 13.6 `generated/__repair__/` cleared at build start (B1-06)

At the start of every `claw build`, `generated/__repair__/` is deleted and recreated. Only the current build's attempt files survive. `claw build` also writes `generated/__repair__/` to `.gitignore` on first run if not already present (independently of whether `generated/` is gitignored).

### 13.7 Budget uses conservative price table (B1-07)

Built-in conservative price table (2× actual prices as safety margin):

| Model prefix | Input $/M | Output $/M |
|---|---|---|
| `claude-haiku` | $0.50 | $1.25 |
| `claude-sonnet` | $3.00 | $15.00 |
| `claude-opus` | $15.00 | $75.00 |
| `gpt-4o-mini` | $0.30 | $1.20 |
| `gpt-4o` | $5.00 | $15.00 |
| `*` (fallback) | $5.00 | $15.00 |

Override with: `retry { price_per_million_tokens { input: 0.25, output: 1.25 } }`.

### 13.8 `SynthesisRequest.repair_context` field (B2-01)

Extend the Spec 33 `SynthesisRequest` with an optional field:

```typescript
repair_context?: {
    attempt:     number;
    tier:        'compile' | 'test';
    strategy:    'repair' | 'rewrite';
    broken_code: string;
    errors:      string;    // truncated per §13.12
}
```

When present, `synth-runner.js` constructs the repair prompt instead of the standard synthesis prompt. Same NDJSON bridge, no separate call path.

### 13.9 Persistent repair log on failure (B2-03)

When E-R01 fires, all attempt files are written to `~/.claw/repair-history/<ToolName>/<timestamp>/`:

```
~/.claw/repair-history/WebSearch/2026-03-19T21-00-00/
├── attempt-1.ts
├── attempt-1-tsc.txt
├── attempt-2.ts
├── attempt-2-tsc.txt
├── attempt-3.ts
├── attempt-3-vitest.txt
└── repair-summary.json
```

Only written on E-R01 (all attempts exhausted). Successful repairs do not write to history.

### 13.10 `repair_then_rewrite` — rewrite is always the final attempt (B2-04)

`repair_then_rewrite`: attempts 1 through `max_attempts - 1` use repair (compile or test per escalation); attempt `max_attempts` is always a rewrite. With `max_attempts: 2`, this gives: attempt 1 = cold synthesis, attempt 2 = rewrite. No compile or test repair — correct behavior for that config.

### 13.11 `.gitignore` entry for `generated/__repair__/` (B2-02)

`claw build` writes `generated/__repair__/` to `.gitignore` on first run. This is written independently of whether `generated/` is already gitignored — users who commit generated TypeScript for review should still not commit stale repair diagnostics.

### 13.12 Error truncation algorithm (B2-05)

Replace "up to 20 error lines" with the following selection algorithm:
1. Always include line 1 (first error is nearly always the root).
2. Collect lines with `tsXXXX:` prefix — up to 15 unique errors.
3. Always include the final `Found N error(s).` summary line.
4. Fill remaining capacity (cap = 20 total) with file:line context for the first 5 errors.

Root error and summary are always present regardless of cascade depth.
