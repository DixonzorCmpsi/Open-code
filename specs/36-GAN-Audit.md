# Spec 36 GAN Audit Log

---

## Round 1: Structural Integrity

### Maker Pass

1. **Tiered escalation is unambiguous** — compile always before test, stuck detection is concrete (set comparison of error codes / test names), no hand-waving.
2. **Both strategies defined with full prompts** — repair and rewrite have literal prompt templates, not vague descriptions.
3. **Budget cap prevents runaway spend** — per-tool, not per-build, which is the right granularity.
4. **Fresh API calls only** — no conversation history accumulation. Token cost is bounded per attempt.
5. **Error truncation is concrete** — 20 lines max, deduplicated by `tsXXXX` code, with "N more errors" note. Actionable.
6. **Telemetry uses `attempt_number`** — the Spec 33 field is finally populated. Repair examples (error + fix pairs) are explicitly identified as high-value training data.
7. **`claw tune` isolation** — retry config is explicitly ignored by tune. Single-shot scoring is preserved.
8. **Last attempt code always saved** — `generated/__repair__/<ToolName>/attempt-<N>.ts` gives users something to debug.

---

### Breaker Pass

**B1-01: Repair prompt includes "all referenced type definitions" — but which types are "referenced"?**
The spec says include "all relevant type definitions from `generated/types.ts`". For a tool returning `SearchResult`, that's just `SearchResult`. But `SearchResult` might contain `Author`, which contains `Affiliation`. The repair pass must resolve the full transitive closure of types referenced in the tool's signature, not just the top-level return type. The spec does not define how deep to go or what algorithm to use.

**B1-02: `compile_repair_limit` default is 2, `max_attempts` default is 1 — contradiction**
The default `max_attempts` is 1 (no retry). The default `compile_repair_limit` is 2. If a user sets only `max_attempts: 3` and leaves `compile_repair_limit` at its default (2), there is 1 attempt left for test repair — which is fine. But what if `max_attempts: 2` and `compile_repair_limit` defaults to 2? There are zero test-repair attempts, and the warning `W-R02` fires. BUT: the spec says "default: 2" for `compile_repair_limit` — this default only makes sense when `max_attempts >= 4`. A better default would be `floor(max_attempts / 2)`, or simply `max_attempts - 1` (all remaining attempts are compile repair, no test repair unless overridden).

**B1-03: `on_stuck: rewrite` calls `rewrite(tool)` — but rewrite uses "the original prompt template". What IS that?**
§3.2 says rewrite uses "the original prompt template". But when the synthesizer has a `tune_prompt` override (Spec 35), which prompt does rewrite use? The tuned prompt or the default template? If the tuned prompt produced broken code, rewriting with the same tuned prompt is counterproductive. If the default template is used, that may produce lower-quality code than the tuned version. This is undefined.

**B1-04: Stuck detection for test tier is too strict — identical failing test names ≠ stuck**
A repair attempt might fix test A but introduce a different failure in test B. The set of failing test names changes (test A removed, test B added) — this is NOT stuck by the spec's definition ("exact set of failing test names matches"). But it IS a lateral move with no net progress. The stuck definition should compare net progress (total passing tests increasing) not set identity.

**B1-05: No definition of what happens when `strategy: repair` and `max_attempts` is reached while still at compile tier**
If `strategy: repair` (no rewrite), `max_attempts: 4`, `compile_repair_limit: 4`, and all 4 attempts fail tsc — the loop exits with E-R01. But `E-R02` fires when "compile_repair_limit reached, strategy does not include rewrite, tsc still fails". With `strategy: repair` and `compile_repair_limit = max_attempts - 1`, the last attempt has no room for test repair. The table in §4.1 says "Abort with E-R02" — but this conflicts with E-R01 (all attempts exhausted). Which error fires? The spec needs to define priority.

**B1-06: `generated/__repair__/` directory grows unboundedly across builds**
Every build writes attempt files to `generated/__repair__/<ToolName>/attempt-<N>.ts`. These accumulate across multiple builds. A developer who runs `claw build` 100 times has 400 stale repair files. The spec does not define cleanup.

**B1-07: Budget tracking requires knowing token prices — but prices change and differ by model**
§7 defines `repair_cost_estimate = (input_tokens / 1M) × input_price + ...`. But `input_price` is not in the synthesizer config — it comes from the API provider and changes. The spec gives no mechanism for the compiler to know the per-token price. Using a hardcoded price table is fragile; calling a pricing API adds latency. This needs a concrete resolution.

---

### Round 1 Fixes

**Fix B1-01: Transitive type closure algorithm**
Add to Spec 36 §5: The repair prompt includes all types reachable by the following algorithm:
1. Start with the tool's return type and all argument types.
2. For each type name encountered: look it up in `document.types`, collect its field types.
3. Repeat recursively until no new names are added (transitive closure, cycle-safe).
4. Emit all collected `TypeDecl`s in declaration order from the document.
5. Cap at 30 type declarations. If exceeded, emit the 30 most recently referenced (breadth-first order).

**Fix B1-02: `compile_repair_limit` default changed to `max(1, max_attempts - 1)`**
Remove the hardcoded default of 2. The default is computed: `min(compile_repair_limit, max_attempts - 1)`. If the user sets `compile_repair_limit` to a value ≥ `max_attempts`, the compiler emits `W-R02` and clamps it to `max_attempts - 1`. Document explicitly:
```
resolved_compile_limit = min(
    user_compile_repair_limit ?? (max_attempts - 1),
    max_attempts - 1
)
```
This means by default, all repair attempts are compile-repair unless overridden. Test repair only happens if the user explicitly sets `compile_repair_limit < max_attempts - 1`.

**Fix B1-03: Rewrite always uses the default template, not the tuned prompt**
Add to §3.2: Rewrite explicitly discards any Spec 35 `tune_prompt` override. The intent of rewrite is to break the LLM's anchoring to the current approach. Using the same tuned prompt that produced broken code defeats this. The rewrite prompt uses the default synthesis template (Spec 33 §3) with the "previous attempts failed" note appended. If the user wants tune prompt rewrites, they can run `claw tune` to get a better base prompt.

**Fix B1-04: Stuck detection uses net progress, not set identity**
Replace the test-tier stuck definition: "stuck" = the number of passing tests in attempt N is the same as in attempt N-1. A lateral move (pass A, fail B vs fail A, pass B) counts as stuck because total passing count did not increase. Formula:
```
test_stuck = (passing_count(attempt_N) <= passing_count(attempt_N-1))
```
For compile tier, keep set-based comparison but change to: "stuck if the count of unique error codes did not decrease".

**Fix B1-05: Error priority — E-R01 fires before E-R02**
Add to §12: E-R02 is a **build-time warning** emitted alongside E-R01, not instead of it. E-R01 (SynthesisExhausted) is the primary error that fails the build. E-R02 is an informational note on WHY it exhausted — "compile repair limit reached with no rewrite fallback." The user sees:
```
error E-R01: WebSearch synthesis failed after 4 attempts
note  E-R02: compile_repair_limit=4 reached, strategy=repair has no rewrite fallback
  → consider: strategy: repair_then_rewrite, or increase max_attempts
```

**Fix B1-06: `generated/__repair__/` cleaned at build start**
Add to §8: At the start of `claw build`, the entire `generated/__repair__/` directory is deleted and recreated. Only files from the current build are retained. This is safe: repair files are diagnostic artifacts, not build outputs. Add to `.gitignore` by default (like `generated/__tune__/`).

**Fix B1-07: Budget uses conservative fixed price table + optional override**
Add to §7: `claw build` uses a built-in conservative price table (prices as of spec date) keyed by model name prefix. Users can override with `retry.price_per_million_tokens { input: 0.25, output: 1.25 }` in the synthesizer config. The table is intentionally conservative (2× actual prices) to prevent accidental overspend when prices drop. The budget cap is therefore a spend ceiling, not an exact tracker.

Built-in table (conservative):
```
"claude-haiku"  → input: $0.50/M,  output: $1.25/M
"claude-sonnet" → input: $3.00/M,  output: $15.00/M
"claude-opus"   → input: $15.00/M, output: $75.00/M
"gpt-4o"        → input: $5.00/M,  output: $15.00/M
"gpt-4o-mini"   → input: $0.30/M,  output: $1.20/M
"*"             → input: $5.00/M,  output: $15.00/M  // fallback
```

---

## Round 2: Integration with Specs 32, 33, 35

### Maker Pass

1. **Repair prompt structure is complete** — signature + types + capability + broken code + errors. Nothing missing after B1-01 fix.
2. **`attempt_number` now actually used** — Spec 33 telemetry gains `repair_tier`, `repair_strategy`, `tsc_errors`, `spend_usd`. All additive fields.
3. **Tune isolation is explicit** — retry config ignored by `claw tune`. No scoring contamination.
4. **Stuck detection is now net-progress-based** — can't loop on lateral moves.
5. **E-R01/E-R02 error hierarchy is clean** — primary error + informational note.

---

### Breaker Pass

**B2-01: The repair loop runs inside `claw build` — but `synth-runner.js` is the synthesis interface (Spec 33). How does repair call synth-runner.js with a DIFFERENT prompt than the original synthesis prompt?**
Currently `synth-runner.js` reads the synthesis request from the artifact and calls the configured client. For repair, the prompt is constructed dynamically (broken code + errors). The `synth-runner.js` interface (NDJSON bridge) accepts a `SynthesisRequest` JSON object — but the repair prompt is not a standard `SynthesisRequest`. Either: repair must bypass `synth-runner.js` and call the LLM directly, or `SynthesisRequest` needs a `repair_context` field that overrides the normal prompt construction. Bypassing is cleaner but creates two separate LLM call paths.

**B2-02: `generated/__repair__/` is inside `generated/` which is under `.gitignore` — but the spec says to add it to `.gitignore` separately**
If `generated/` is already gitignored (it should be — it's a build output), then `generated/__repair__/` is already ignored. The spec's note is redundant but harmless. However, if a user does NOT gitignore `generated/` (they might commit generated TS for review), then `generated/__repair__/` contains stale diagnostic debris that would get committed. The spec should explicitly say: add `generated/__repair__/` to `.gitignore` REGARDLESS of whether `generated/` is ignored.

**B2-03: No mechanism for the user to see repair attempt history interactively**
The repair files go to `generated/__repair__/<Name>/attempt-N.ts` but are deleted at build start (Fix B1-06). If the user wants to debug why synthesis kept failing, they have to catch it during the build. There's no `claw repair-log` or persistent history. Unlike `claw tune` which has `~/.claw/tune-history/`, repair has no persistent log. On a failed build, the user only gets the E-R01 error message and the last attempt file (if the build was slow, they might not notice before it's deleted on the next run).

**B2-04: `repair_then_rewrite` strategy doesn't define which attempt triggers the rewrite**
§3.3 says "repair for all attempts up to `max_attempts - 1`, then a clean rewrite as the final attempt." But with `compile_repair_limit` set, the attempt sequence might be: repair(compile), repair(compile), repair(test), rewrite. The spec doesn't define whether `repair_then_rewrite` counts the rewrite against `max_attempts` or adds one extra attempt. If it counts against `max_attempts`, users might set `max_attempts: 2, strategy: repair_then_rewrite` and get: attempt 1 = cold synthesis, attempt 2 = rewrite. That's effectively just "try twice with different prompts" — no compile or test repair at all.

**B2-05: Error truncation may remove the root error if it appears after line 20**
§6 says "up to 20 error lines total". TypeScript often emits errors in cascading order: the root type mismatch appears first, but in some cases (e.g., a missing import that causes 50+ "cannot find name" errors), the most useful error IS the first one and all 20 lines are useful. However, tsc also prints "N error(s) found" at the end — that line should always be included. The "20 line" cap needs to preserve: line 1, unique error codes, and the summary line.

---

### Round 2 Fixes

**Fix B2-01: `SynthesisRequest` gains `repair_context` optional field**
Extend the `SynthesisRequest` schema (Spec 33 §2) with:
```typescript
interface SynthesisRequest {
  // ... existing fields ...
  repair_context?: {
    attempt:    number;
    tier:       'compile' | 'test';
    strategy:   'repair' | 'rewrite';
    broken_code: string;
    errors:     string;   // truncated error output
  }
}
```
When `repair_context` is present, `synth-runner.js` constructs the repair prompt instead of the standard synthesis prompt. The NDJSON interface is unchanged — same bridge, same protocol. No separate LLM call path needed.

**Fix B2-02: `.gitignore` annotation**
Change §8 note: add `generated/__repair__/` to `.gitignore` explicitly, independently of whether `generated/` is gitignored. `claw build` writes this entry to `.gitignore` on first run if it is not already present (same pattern as other generated `.gitignore` entries).

**Fix B2-03: Persistent repair log in `~/.claw/repair-history/`**
Add to Spec 36 §8: Failed synthesis attempts (where E-R01 fires) are written to `~/.claw/repair-history/<ToolName>/<timestamp>/`:
```
~/.claw/repair-history/WebSearch/2026-03-19T21-00-00/
├── attempt-1.ts       # cold synthesis output
├── attempt-1-tsc.txt  # tsc errors for attempt 1
├── attempt-2.ts       # repair output
├── attempt-2-tsc.txt  # remaining tsc errors
├── attempt-3.ts       # test-repair output
├── attempt-3-vitest.txt  # vitest failures
└── repair-summary.json   # attempt count, strategies used, spend
```
This is written ONLY when E-R01 fires (i.e., all attempts failed). Successful repairs do not clutter the history. Users can inspect `~/.claw/repair-history/` to understand why synthesis is consistently failing and decide whether to run `claw tune` to improve the base prompt.

**Fix B2-04: `repair_then_rewrite` — rewrite is always the LAST attempt**
Clarify §3.3: `repair_then_rewrite` means:
- Attempts 1 through `max_attempts - 1`: repair (compile or test per escalation rules)
- Attempt `max_attempts`: always a rewrite, regardless of which tier was active
This is always one fewer repair attempt than `max_attempts` total. If `max_attempts: 2`, the user gets: attempt 1 = cold synthesis, attempt 2 = rewrite. That is correct behavior for `repair_then_rewrite: max_attempts=2`.

**Fix B2-05: Error line selection algorithm**
Replace "up to 20 error lines" with:
1. Always include line 1 (first error is almost always the root).
2. Collect all lines with a `tsXXXX:` error code prefix — up to 15 unique errors.
3. Always include the final summary line (`Found N error(s).`).
4. Fill remaining capacity (cap = 20 total) with diagnostic context lines (file path + line number) for the first 5 errors.
This guarantees the root error and summary are always present regardless of cascade depth.

---

## Summary: 12 Gaps Found and Fixed

| Round | ID | Category | Fix |
|---|---|---|---|
| 1 | B1-01 | Correctness | Transitive type closure algorithm for repair prompt context |
| 1 | B1-02 | Config | `compile_repair_limit` default = `max_attempts - 1`, clamped |
| 1 | B1-03 | Strategy | Rewrite always uses default template, ignores tune_prompt |
| 1 | B1-04 | Detection | Stuck = net progress (passing count), not set identity |
| 1 | B1-05 | Errors | E-R01 primary, E-R02 informational note with suggestion |
| 1 | B1-06 | Hygiene | `generated/__repair__/` cleared at build start |
| 1 | B1-07 | Budget | Conservative fixed price table + optional override |
| 2 | B2-01 | Interface | `SynthesisRequest.repair_context` optional field |
| 2 | B2-02 | Gitignore | `generated/__repair__/` always gitignored explicitly |
| 2 | B2-03 | Observability | Persistent repair log in `~/.claw/repair-history/` on E-R01 |
| 2 | B2-04 | Strategy | `repair_then_rewrite` rewrite is always final attempt |
| 2 | B2-05 | Truncation | Error selection algorithm preserves root error + summary |
