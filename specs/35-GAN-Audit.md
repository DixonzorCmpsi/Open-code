# Spec 35 GAN Audit Log

---

## Round 1: Structural Integrity

### Maker Pass

1. **Three-ingredient mapping is exact** — vitest tests, synthesis prompt, and pass rate map perfectly to Karpathy's framework. No hand-waving.
2. **`eval {}` vs `test {}` separation** — clean: `test {}` is deterministic (vitest), `eval {}` is LLM-judged (judge call). Different lifecycles, different runners.
3. **Budget cap** — `budget_usd` prevents runaway loops. Good safety rail.
4. **Prompt versioning** — full history in `~/.claw/tune-history/`. Can inspect and revert.
5. **Training data integration** — tune iterations produce labeled training examples that feed Spec 33 §7 (future native model). Loop is self-reinforcing.
6. **Per-tool prompt override in artifact** — `tune_prompt` field lets the winning prompt flow into `synth-runner.js` without re-architecting the synthesis pipeline.

### Breaker Pass

**B1-01: The judge evaluates compiled vs uncompiled code — but synthesis may produce invalid TypeScript**
The LLM judge is asked "does the generated TypeScript compile?" by reading the code. But a text-reading judge cannot actually compile TypeScript — it can only guess. The criterion `compiles` should be checked by actually running `tsc --noEmit`, not by the LLM judge. If both vitest and tsc run automatically, the judge criteria should be restricted to quality questions that genuinely require LLM judgment (style, patterns, security) and the `compiles` criterion should be a special first-class mechanical check.

**B1-02: `runs_per_iteration` in `eval {}` vs `tune {}` — conflict**
Both `eval { runs: 10 }` on the tool and `tune { runs: 10 }` on the synthesizer declare `runs`. The spec is ambiguous about which one wins when both are present. A tool with `eval { runs: 5 }` in a synthesizer with `tune { runs: 20 }` — what happens?

**B1-03: Mutator is given "2 worst-performing criteria" — but it may not know WHY they fail**
The mutator prompt (§4.4) gives failing criteria + 2 failed code samples. But the mutator doesn't receive the CURRENT synthesis prompt that produced those failures. Without seeing the current prompt, the mutator cannot know whether the failures stem from missing instructions or contradicting instructions. The spec shows `<current_synthesis_prompt>` but the earlier text said "2 worst-performing criteria per iteration" — the current prompt must always be included.

**B1-04: `augment_examples: true` mutates the source .claw file — dangerous side effect**
§7 says `claw tune` can append to `examples {}` in the source `.claw` file. This is a destructive write to user source code. The user may have the file under version control. The spec gives no warning about this, no confirmation prompt, no diff preview. This is too aggressive.

**B1-05: `tune_prompt` in artifact is visible to anyone who reads artifact.clawa.json**
The winning prompt encodes hard-won synthesis intelligence. For commercial Claw users, their tuned prompts are proprietary. Storing them in `generated/artifact.clawa.json` (which is committed to git and readable by anyone) leaks them. Spec 33 §9 scrubs `system_prompt` — but `tune_prompt` has no equivalent protection.

**B1-06: `claw tune` requires synthesis API calls — cannot run without internet/keys**
The loop calls the synthesis LLM (to generate code) and the judge LLM (to evaluate it) on every run. If the user is on a plane, offline, or has no API key, `claw tune` silently fails or hangs. There's no `--offline` mode or local model check.

---

### Round 1 Fixes

**Fix B1-01: Split `eval {}` criteria into mechanical and LLM-judged tiers**
Add to Spec 35 §2.1: A special criterion prefix `compile:` or `tsc:` triggers TypeScript compilation check instead of LLM judgment:

```claw
eval {
    runs: 10
    criteria {
        tsc:compiles:      "tsc --noEmit"          // special: runs tsc, not LLM judge
        no_any:        "Does the code avoid the TypeScript 'any' type?"
        url_encoded:   "Is the query URL-encoded?"
    }
}
```

Any criterion labeled with `tsc:` prefix runs `tsc --noEmit` on the synthesized TypeScript file and records the exit code (0 = YES, non-zero = NO). This replaces the LLM judge for compilation correctness. The judge is reserved for code quality questions that require language understanding.

Additionally, vitest pass/fail is always included in the score as a first-class mechanical check, separate from the judge call.

**Fix B1-02: `runs` precedence rule**
Add to Spec 35 §3: Tool-level `eval { runs }` overrides synthesizer-level `tune { runs }` for that specific tool. If only one is specified, that value is used. If neither is specified, default is 10. Document explicitly:

```
tool-level eval.runs > synthesizer-level tune.runs > default (10)
```

**Fix B1-03: Mutator always receives current prompt**
The mutator call (§4.4) always includes the full `current_synthesis_prompt`. The spec already shows this in the pseudocode but the text was ambiguous. Clarify: the mutator receives:
- Full current prompt (always)
- Failing criteria (those with < 70% pass rate, not just "2 worst")
- 2 failed code samples per failing criterion
- Summary of scores across all runs

**Fix B1-04: `augment_examples` writes to a staging file, not source**
Change §7: when `augment_examples: true`, `claw tune` writes discovered examples to `~/.claw/tune-history/<tool>/best-examples.json` — NOT to the source `.claw` file. The user can review this file and manually copy examples into their `.claw` source. `claw tune` emits a message:

```
  New examples discovered (3):
    → review at ~/.claw/tune-history/WebSearch/2026-03-19T20-00-00/best-examples.json
    → to apply: claw apply-examples WebSearch
```

`claw apply-examples <ToolName>` is a new safe command that shows a diff and requires confirmation before writing to the source file.

**Fix B1-05: `tune_prompt` excluded from artifact by default**
Change §8: `tune_prompt` is NOT stored in `artifact.clawa.json`. Instead it lives only in `claw.json` (which should be `.gitignore`d for sensitive projects) and in `~/.claw/tune-history/`. The `synth-runner.js` generator reads from `claw.json` directly, not from the artifact. Add to Spec 35 §8: a note that users who want synthesis prompt privacy should add `claw.json` to `.gitignore`.

**Fix B1-06: Local model support + offline check**
Add to Spec 35 §5: `claw tune` checks at startup:
1. If synthesizer client is `local.` (Ollama), it uses the local model for synthesis — no internet required.
2. If judge client is local too, the entire loop runs offline.
3. If a cloud client is configured but no API key is set → error at startup with helpful message, not mid-loop failure.
4. `--dry-run` mode runs the eval loop without the mutation step and without synthesis (uses existing generated code) — lets users evaluate their current synthesis quality without any API calls.

---

## Round 2: Integration with Specs 32, 33, 34

### Maker Pass

1. **Training data flows cleanly** — tune sessions augment `~/.claw/synthesis-telemetry/` with eval-labeled examples. The Spec 33 §5 schema gains `eval_results` which is purely additive.
2. **Per-tool prompt override in claw.json** — synthesizer resolves prompt: `claw.json` custom prompt → default template. Clean priority chain.
3. **`tsc:` prefix criteria are mechanically correct** — actual TypeScript compiler, not LLM guess. This is the highest-value criterion.
4. **`eval {}` block is zero-cost at `claw build`** — the block exists in the AST but does nothing during normal build. No performance regression.
5. **Spec 34 `examples {}` integration** — discovered examples staged, not auto-applied. Safe.

### Breaker Pass

**B2-01: `tsc --noEmit` requires a tsconfig.json — not guaranteed to exist**
The `tsc:compiles` criterion runs `tsc --noEmit` on the synthesized file. But `tsc` needs a `tsconfig.json` in scope to know what settings to use. The generated code is in `generated/tools/`, which may not have a `tsconfig.json` if the user hasn't configured TypeScript. `claw tune` must either ship a minimal embedded tsconfig or generate one at tune-time.

**B2-02: Judge call model must support long-context TypeScript**
The judge LLM reads synthesized TypeScript (potentially 100-200 lines) and answers N questions. If the judge is `claude-haiku-4-5`, its 200k context is fine. But if the user configures a small local model (e.g., Ollama `qwen2.5:7b`) as the judge, long-context + structured output may be unreliable. The spec recommends haiku but doesn't prevent local models from being judges.

**B2-03: Score normalization when `eval {}` has no criteria (only `tsc:` prefix)**
If `eval {}` has `runs: 10` but only `tsc:compiles` (a mechanical check, not LLM), the score formula in §4.2 divides by `eval_criteria_count` which is 0 for LLM criteria. Division by zero.

**B2-04: `claw tune` must know where to find the synthesized TypeScript to compile**
The tune loop synthesizes TypeScript via `synth-runner.js` — but the synthesis pipeline writes code to `generated/tools/<Name>.ts`. If `claw tune` runs synthesis but doesn't trigger a vitest run, the generated files may not exist yet (first run). The loop must handle: synthesis → write to temp dir → run tsc + vitest → score → next iteration.

**B2-05: Prompt override in claw.json is not in the `.clawa` artifact**
Fix B1-05 removed `tune_prompt` from the artifact. But `synth-runner.js` is generated from the artifact. If the prompt override lives only in `claw.json`, then `synth-runner.js` needs to read `claw.json` at runtime — coupling two separate files. The original design kept everything in the artifact for a clean interface. This needs a clean resolution.

---

### Round 2 Fixes

**Fix B2-01: Embedded tsconfig for tune runs**
Add to Spec 35 §4: when `claw tune` runs, it writes a temporary `generated/__tune__/tsconfig.json`:

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "NodeNext",
    "moduleResolution": "NodeNext",
    "strict": true,
    "noEmit": true
  },
  "include": ["../tools/*.ts", "../types.ts"]
}
```

`tsc:compiles` criterion runs `tsc --project generated/__tune__/tsconfig.json`. The temp directory is cleaned up after each tune session.

**Fix B2-02: Judge quality warning for small local models**
Add to Spec 35 §3: If `tune.judge` is set to a local Ollama client (detected by `client.provider = "ollama"` or model name prefix), `claw tune` emits:

```
warning W-T03: judge client 'OllamaLocal' is a local model.
  LLM-judged eval criteria may be unreliable for models < 14B parameters.
  Recommendation: use claude-haiku-4-5 or a 14B+ local model as judge.
  To suppress: set tune.judge_warning = false in synthesizer block.
```

**Fix B2-03: Score formula guards against zero criteria counts**
Update §4.2:
```
llm_criteria_count  = count of criteria WITHOUT tsc: prefix
mech_criteria_count = count of criteria WITH tsc: prefix

score_per_run = (vitest_passes / vitest_test_count if vitest_test_count > 0 else 1.0)
              + (tsc_passes    / mech_criteria_count if mech_criteria_count > 0 else 0.0)
              + (llm_passes    / llm_criteria_count  if llm_criteria_count  > 0 else 0.0)

All three terms normalized so total is always in [0.0, 1.0].
```

**Fix B2-04: Tune loop writes to isolated temp directory**
Add to Spec 35 §4: Each tune iteration writes synthesized files to `generated/__tune__/iter-<N>/` rather than `generated/tools/`. This keeps tune runs isolated from the main build output. The temp directory is cleaned up at session end.

**Fix B2-05: Prompt override path — claw.json feeds artifact regeneration**
The clean resolution: `claw build` reads `claw.json → synthesizer.prompts` and injects any custom prompts INTO the artifact at build time. The artifact becomes the single source of truth again. `synth-runner.js` continues to read only from the artifact. The flow is:

```
claw.json (synthesizer.prompts) ─→ claw build ─→ artifact.clawa.json (tune_prompt field)
                                                         ↓
                                               synth-runner.js reads it
```

The concern from Fix B1-05 (prompts leaking in git) is addressed by: `claw build` embeds the prompt in the artifact only when `tune.include_prompt_in_artifact = true` (default: false). When false, `claw.json` is the authoritative location and `synth-runner.js` is generated to read it directly.

---

## Summary: 12 Gaps Found and Fixed

| Round | ID | Category | Fix |
|---|---|---|---|
| 1 | B1-01 | Correctness | `tsc:` prefix criteria run actual compiler, not LLM judge |
| 1 | B1-02 | Precedence | `eval.runs` > `tune.runs` > default 10 |
| 1 | B1-03 | Mutator | Full current prompt always included in mutator call |
| 1 | B1-04 | Safety | `augment_examples` stages to file, not source; `claw apply-examples` command |
| 1 | B1-05 | Privacy | `tune_prompt` not in artifact by default; lives in `claw.json` |
| 1 | B1-06 | Offline | Local model support; startup key check; `--dry-run` mode |
| 2 | B2-01 | Tooling | Embedded tsconfig generated at tune-time |
| 2 | B2-02 | Quality | Warning W-T03 for small local judge models |
| 2 | B2-03 | Math | Score formula guards against zero criterion count |
| 2 | B2-04 | Isolation | Tune writes to `generated/__tune__/` not `generated/tools/` |
| 2 | B2-05 | Architecture | `claw.json` → `claw build` → artifact; `include_prompt_in_artifact` flag |
