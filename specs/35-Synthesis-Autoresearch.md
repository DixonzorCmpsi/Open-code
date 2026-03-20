# Spec 35: Synthesis Autoresearch

**Status:** Specced 2026-03-19. Defines the `eval {}` DSL block, the `claw tune` command, and the autoresearch loop that autonomously improves the Synthesis Pass prompt until TypeScript generation reaches near-100% accuracy on declared criteria.

---

## 1. Core Concept

The synthesis pipeline (Spec 32) already has the three ingredients Karpathy's autoresearch framework requires:

| Autoresearch ingredient | Claw equivalent |
|---|---|
| **Objective metric** | Synthesis eval score: sum of binary criteria passes across N runs |
| **Automated measurement tool** | `vitest` contract/behavior tests + LLM judge for quality criteria |
| **Something to change** | The synthesis prompt template (Spec 33 §3) |

The missing piece is the **mutation + evaluation loop** that runs overnight, scores thousands of synthesis outputs, and converges on the highest-scoring prompt. This spec defines that loop as a first-class Claw feature: `claw tune`.

The loop improves two things independently:
1. **Prompt quality** — the synthesis prompt template for a specific tool or synthesizer.
2. **Example quality** — the `examples {}` block content (Spec 34 §4), which the mutator can augment with discovered high-signal examples.

---

## 2. The `eval {}` Block

`test {}` blocks (Spec 32) provide deterministic assertions (`!empty`, `range`, etc.) on concrete output field values. `eval {}` provides **LLM-judge quality criteria** — binary yes/no questions that are difficult to express as field assertions.

### 2.1 DSL Grammar

```
tool <Name>(<inputs>) -> <OutputType> {
    using: <capability>
    eval {
        runs: <int>       // syntheses per iteration (default 10)
        criteria {
            <label>: "<binary yes/no question about the synthesized code>"
            ...
        }
    }
    test { ... }    // unchanged — deterministic assertions, run independently
}
```

**Example:**

```claw
tool WebSearch(query: string) -> SearchResult {
    using:       fetch
    description: "Searches the web for a query. Returns top URL and snippet."
    eval {
        runs: 10
        criteria {
            compiles:      "Does the generated TypeScript compile without errors?"
            no_any:        "Does the code avoid using the TypeScript 'any' type?"
            url_encoded:   "Is the query parameter URL-encoded before use in a fetch call?"
            error_handled: "Does the code have a try/catch or .catch() around the fetch call?"
            no_hardcode:   "Does the code avoid hardcoded URLs, API keys, or credentials?"
        }
    }
    test {
        input:  { query: "rust language" }
        expect: { url: !empty, snippet: !empty }
    }
}
```

### 2.2 Criterion format rules

- Each criterion is a label (identifier) mapped to a **binary yes/no question string**.
- Questions must be answerable by reading only the synthesized TypeScript code — no execution.
- Questions must not reference runtime behavior (e.g., "does this return correct results?" is invalid; "does this call `encodeURIComponent`?" is valid).
- Maximum 20 criteria per `eval {}` block. Compiler warning `W-T01: TooManyCriteria` above 20.
- Criterion labels are used as metric keys in tune reports.

### 2.3 Relationship to `test {}` and `examples {}`

| Block | Runs at | Evaluated by | Purpose |
|---|---|---|---|
| `test {}` | `claw build` (always) | vitest assertions | Functional correctness — did the output match? |
| `eval {}` | `claw tune` only | LLM judge | Code quality — is the implementation good? |
| `examples {}` | `claw build` (injected) | N/A — specification | Few-shot grounding for synthesis prompt |

The `eval {}` block is **never executed during `claw build`**. It is a development-time optimization tool only.

---

## 3. Tune Configuration on `synthesizer {}`

```
synthesizer DefaultSynth {
    client      = MyClaude
    temperature = 0.1
    tune {
        iterations:   20          // mutation cycles (default 20)
        runs:         10          // syntheses per iteration (default 10)
        judge:        MyClaude    // client used to judge eval criteria (default: same as synthesizer client)
        save_prompt:  true        // persist winning prompt to claw.json (default true)
        budget_usd:   5.00        // hard stop when API spend exceeds this (default 5.00)
    }
}
```

`iterations` × `runs` = total synthesis attempts. E.g., 20 × 10 = 200 synthesis runs total for the tune session. At ~$0.01/run for claude-haiku, 200 runs = ~$2.

`judge` — the LLM client used to evaluate binary criteria. Can be different (cheaper) than the synthesis client. Haiku works well as a judge.

`budget_usd` — a hard spend cap. `claw tune` tracks API calls and aborts if the running cost exceeds this. Protects against runaway loops.

---

## 4. The Autoresearch Loop

### 4.1 Loop pseudocode

```
current_prompt ← synthesis_prompt_template (from Spec 33 §3)
best_prompt    ← current_prompt
best_score     ← 0

for iteration in 1..=iterations:
    scores = []
    for run in 1..=runs:
        code ← synthesize(tool, current_prompt)         // invoke SynthesisModelAdapter
        vitest_score ← run_vitest_tests(code)           // 1.0 or 0.0 per test
        eval_score   ← run_llm_judge(code, criteria)    // binary: 0 or 1 per criterion
        scores.push(vitest_score + eval_score)

    iteration_score ← mean(scores)

    if iteration_score > best_score:
        best_score  ← iteration_score
        best_prompt ← current_prompt

    failures ← collect_failing_criteria(scores, criteria)
    current_prompt ← mutate_prompt(current_prompt, failures, meta_llm)

    emit TuneIteration { iteration, score: iteration_score, best_score }
    if iteration_score == max_possible_score: break     // perfect score — stop early

save best_prompt to claw.json (if save_prompt: true)
emit TuneReport { total_iterations, best_score, prompt_delta }
```

### 4.2 Score computation

```
max_score_per_run = vitest_test_count + eval_criteria_count

score_per_run = (vitest_tests_passed / vitest_test_count)
              + (eval_criteria_passed / eval_criteria_count)

iteration_score = mean(score_per_run across all runs in this iteration)
                  normalized to [0.0, 1.0]
```

`claw tune` reports `Pass@1` (fraction of runs that pass all criteria on first synthesis attempt) as the primary headline metric.

### 4.3 LLM judge call

For each synthesized TypeScript file, one judge call evaluates ALL criteria in a single prompt:

```
SYSTEM:
You are a TypeScript code reviewer. Answer each question with exactly "YES" or "NO".
Do not explain. Output one answer per line matching the question order.

USER:
Code to review:
```typescript
<synthesized_code>
```

Questions:
1. Does the generated TypeScript compile without errors?
2. Does the code avoid using the TypeScript 'any' type?
3. Is the query parameter URL-encoded before use in a fetch call?
4. Does the code have a try/catch or .catch() around the fetch call?
5. Does the code avoid hardcoded URLs, API keys, or credentials?
```

Response is parsed as N lines of "YES"/"NO". Any other response format → criterion marked as FAIL.

### 4.4 Prompt mutator call

After each iteration, the mutator LLM reads the failing criteria and suggests an improved synthesis prompt:

```
SYSTEM:
You are optimizing a synthesis prompt. Your goal: maximize the binary eval pass rate.
Output ONLY the improved prompt. Do not explain.

USER:
Current prompt:
---
<current_synthesis_prompt>
---

Failing criteria this iteration (criteria where < 50% of runs passed):
- "Does the code avoid using the TypeScript 'any' type?" — passed 3/10 runs
- "Is the query parameter URL-encoded?" — passed 5/10 runs

Synthesized code samples that FAILED these criteria:
<failed_code_sample_1>
<failed_code_sample_2>

Rewrite the synthesis prompt to fix these failures.
```

The mutator is given the 2 worst-performing failing criteria per iteration (not all failures — too much context is counterproductive). It sees 2 example code samples that failed those criteria.

### 4.5 Prompt versioning

Each iteration's prompt is saved to `~/.claw/tune-history/<tool-name>/<timestamp>/`:

```
~/.claw/tune-history/WebSearch/2026-03-19T20-00-00/
├── iteration-01-prompt.md    # prompt used in iteration 1
├── iteration-01-score.json   # scores and criterion breakdown
├── iteration-02-prompt.md
├── ...
├── best-prompt.md            # overall winner
└── tune-report.json          # final summary
```

The best prompt is persisted to `claw.json` under `synthesizer.prompts.<ToolName>` when `save_prompt: true`:

```json
{
  "synthesizer": {
    "prompts": {
      "WebSearch": "## SYNTHESIS TARGET\n...(winning prompt content)..."
    }
  }
}
```

At synthesis time, `claw build` checks `claw.json` for a per-tool prompt override before using the default template.

---

## 5. `claw tune` CLI Command

```bash
claw tune [options]
```

Options:

| Flag | Default | Description |
|---|---|---|
| `--tool <name>` | all tools with `eval {}` | Tune only the named tool |
| `--iterations <n>` | from synthesizer tune{} or 20 | Override iteration count |
| `--runs <n>` | from synthesizer tune{} or 10 | Override runs per iteration |
| `--dry-run` | false | Run evals but don't mutate prompt or save |
| `--report` | false | Print detailed criterion breakdown after each iteration |
| `--config <path>` | claw.json | Config file path |

Output during execution:

```
claw tune — optimizing WebSearch synthesis prompt
 source: example.claw
 synthesizer: DefaultSynth (anthropic/claude-haiku-4-5)
 judge: DefaultSynth
 iterations: 20 × 10 runs = 200 synthesis attempts
 budget: $5.00

[iter  1/20] score: 0.62  best: 0.62  (62/100 criteria passed)
              failing: no_any (3/10), url_encoded (5/10)
[iter  2/20] score: 0.74  best: 0.74  ↑ new best
              failing: url_encoded (6/10), error_handled (7/10)
[iter  3/20] score: 0.74  best: 0.74  (no improvement)
...
[iter  8/20] score: 0.98  best: 0.98  ↑ new best
[iter  9/20] score: 1.00  best: 1.00  ↑ PERFECT — stopping early

✓ Tuning complete in 9 iterations (90 synthesis runs)
  Pass@1:  100%  (was 62% before tuning)
  Spend:   ~$0.47
  Prompt saved to: claw.json → synthesizer.prompts.WebSearch
  History: ~/.claw/tune-history/WebSearch/2026-03-19T20-00-00/
```

---

## 6. Integration with Spec 33 Training Data

Every tune iteration produces labeled training examples:

- **Positive examples**: runs that passed all vitest tests AND all eval criteria
- **Negative examples**: runs that failed one or more criteria, annotated with which criteria failed

These are written to `~/.claw/synthesis-telemetry/` (same schema as Spec 33 §5) with an additional `tune` field:

```json
{
  "quality": {
    "tests_passed": true,
    "attempt_number": 1,
    "test_results": [...],
    "eval_results": [
      { "criterion": "no_any",        "passed": true  },
      { "criterion": "url_encoded",   "passed": false },
      { "criterion": "error_handled", "passed": true  }
    ],
    "tune_iteration": 3,
    "tune_session": "WebSearch/2026-03-19T20-00-00"
  }
}
```

The future Claw-native fine-tuned model (Spec 33 §7) is trained on this corpus. Tune sessions with `Pass@1 = 100%` produce the highest-quality positive training examples because every output passes both deterministic tests AND LLM quality criteria.

---

## 7. Integration with Spec 34 Examples

The tune loop can optionally **augment the `examples {}` block** in the source `.claw` file when it discovers a synthesized code output that is both:
- Passes all eval criteria (score = 1.0)
- Differs meaningfully from existing examples (the judge confirms it covers a new pattern)

This is opt-in via:

```
synthesizer DefaultSynth {
    tune {
        augment_examples: true    // add discovered high-signal examples to .claw source
    }
}
```

When enabled, `claw tune` appends up to 3 new `examples {}` entries to the tool declaration in the source file after a successful tune session. This closes the loop: tuning improves the prompt AND enriches the grounding examples for future synthesis.

---

## 8. Artifact additions

The artifact (`generated/artifact.clawa.json`) gains a `tune_metadata` section per tool:

```json
{
  "tools": [
    {
      "name": "WebSearch",
      "eval": {
        "runs": 10,
        "criteria": [
          { "label": "compiles",      "question": "Does the generated TypeScript compile without errors?" },
          { "label": "no_any",        "question": "Does the code avoid using the TypeScript 'any' type?" },
          { "label": "url_encoded",   "question": "Is the query parameter URL-encoded before use?" },
          { "label": "error_handled", "question": "Does the code have a try/catch around the fetch call?" },
          { "label": "no_hardcode",   "question": "Does the code avoid hardcoded URLs or credentials?" }
        ]
      },
      "tune_prompt": "... winning prompt override ..."
    }
  ]
}
```

`tune_prompt` is populated from `claw.json → synthesizer.prompts.<ToolName>` if present. When populated, `synth-runner.js` uses it instead of the default template.

---

## 9. New AST nodes

```rust
// In ast.rs:

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EvalBlock {
    pub runs:     Option<u32>,
    pub criteria: Vec<EvalCriterion>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EvalCriterion {
    pub label:    String,
    pub question: String,
    pub span:     Span,
}

// In SynthesizerDecl: add optional TuneConfig
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TuneConfig {
    pub iterations:       Option<u32>,
    pub runs:             Option<u32>,
    pub judge:            Option<String>,    // client name
    pub save_prompt:      Option<bool>,
    pub budget_usd:       Option<f64>,
    pub augment_examples: Option<bool>,
}
```

`ToolDecl` gains `pub eval_block: Option<EvalBlock>`.
`SynthesizerDecl` gains `pub tune: Option<TuneConfig>`.

---

## 10. New error/warning codes

| Code | Name | Trigger |
|---|---|---|
| W-T01 | TooManyCriteria | `eval {}` has > 20 criteria |
| W-T02 | EvalWithoutUsing | Tool has `eval {}` but no `using:` — eval only applies to synthesis-path tools |
| E-T01 | UndefinedJudgeClient | `tune.judge` references a client not declared |
| E-T02 | InvalidBudget | `tune.budget_usd` is ≤ 0 |

---

## 11. GAN Audit Amendments (R1–R2)

See `35-GAN-Audit.md` for the full finding log. 12 gaps found and fixed.

### 11.1 Mechanical `tsc:` criteria (B1-01, B2-01)

Criteria prefixed with `tsc:` run the TypeScript compiler instead of the LLM judge:

```claw
eval {
    criteria {
        tsc:compiles:  "tsc --noEmit"   // runs actual compiler — exit code 0 = YES
        no_any:        "Does the code avoid the 'any' type?"  // LLM judge
    }
}
```

`claw tune` writes `generated/__tune__/tsconfig.json` (strict, noEmit, NodeNext) at session start, cleaned up at end.

### 11.2 `runs` precedence (B1-02)

`eval { runs }` on a tool overrides `tune { runs }` on the synthesizer. Both override the default (10).

### 11.3 Mutator always receives full current prompt (B1-03)

The mutator prompt always includes: full current synthesis prompt + all failing criteria (< 70% pass rate) + 2 failed code samples per failing criterion.

### 11.4 `augment_examples` stages, never auto-applies (B1-04)

`augment_examples: true` writes to `~/.claw/tune-history/<tool>/best-examples.json`. Never writes to source. Apply with `claw apply-examples <ToolName>` (shows diff + requires confirmation).

### 11.5 `tune_prompt` privacy and artifact flow (B1-05, B2-05)

`tune_prompt` lives in `claw.json → synthesizer.prompts`. At `claw build` time, if `tune.include_prompt_in_artifact = true` (default false), the prompt is embedded in the artifact. Otherwise `synth-runner.js` reads from `claw.json` directly. Users with sensitive prompts set `include_prompt_in_artifact = false` and add `claw.json` to `.gitignore`.

### 11.6 Offline / local model support (B1-06)

`claw tune` checks API keys at startup before the loop. `--dry-run` runs eval on existing generated code with zero synthesis calls. Local Ollama clients run the full loop offline.

### 11.7 Small judge model warning (B2-02)

`W-T03: SmallJudgeModel` — warning when judge client is a local model < 14B. Suppressed with `tune.judge_warning = false`.

### 11.8 Score formula (B2-03)

Three-term normalized score — each term is 0 if no criteria of that type exist:

```
score = (vitest_passes / vitest_count    if vitest_count    > 0 else 1.0)
      + (tsc_passes    / tsc_count       if tsc_count       > 0 else 0.0)
      + (llm_passes    / llm_count       if llm_count       > 0 else 0.0)
all three normalized so total ∈ [0.0, 1.0]
```

### 11.9 Tune writes to isolated temp directory (B2-04)

Synthesized files go to `generated/__tune__/iter-<N>/`, never to `generated/tools/`. Cleaned up at session end.

---

## 12. What This Spec Does NOT Cover

- The full implementation of `claw tune` as a Rust binary (this is a separate CLI module, large scope)
- Parallel synthesis execution (future: run all N synthesis calls concurrently)
- A hosted dashboard for viewing tune history across projects
- Cross-tool tuning (tuning the synthesizer globally across all tools at once — future)
- Integration with the Spec 33 benchmark suite `tests/synthesis-bench/` (future: `claw bench`)
