# Implementation Prompt: Spec 35 — Synthesis Autoresearch

**Project:** Claw DSL compiler (`clawc`) in Rust — `/Users/dixon.zor/Documents/Open-code`
**Specs to implement:** `specs/35-Synthesis-Autoresearch.md` (with GAN fixes from `specs/35-GAN-Audit.md`)
**Prerequisites:**
- Spec 34 implemented (registry, sandbox, examples — `specs/34-Advanced-Tool-Patterns.md`)
- All tests pass: run `INSTA_UPDATE=always ~/.cargo/bin/cargo test` to verify before starting.

---

## What you are implementing

Two things:

1. **`eval {}` block** — a new DSL block on `tool` declarations that declares binary LLM-judge quality criteria for the synthesis autoresearch loop. Never executes during `claw build`; only consumed by `claw tune`.
2. **`claw tune` CLI command** — an autoresearch loop (`src/bin/tune.rs`) that: synthesizes code, runs vitest, invokes an LLM judge, scores iterations, calls a mutator LLM to improve the synthesis prompt, persists the best prompt to `claw.json`, and writes full history to `~/.claw/tune-history/`.

The `tune {}` block inside `synthesizer {}` configures the loop (iterations, runs, judge client, budget cap).

---

## Existing codebase orientation

Read these files FIRST before writing any code:

- `src/ast.rs` — all AST node definitions
- `src/parser.rs` — winnow 0.7 parser (use `verify_map` not `try_map`, no single-element `alt()`)
- `src/semantic/mod.rs` — symbol table, duplicate detection, error/warning collection
- `src/semantic/types.rs` — statement/expression type checking
- `src/codegen/mod.rs` — codegen module registry, `document_ast_hash`, `write_document`
- `src/codegen/artifact.rs` — generates `generated/artifact.clawa.json` (add `eval` section here)
- `src/codegen/synth_runner.rs` — generates `generated/synth-runner.js` (add tune_prompt reading here)
- `src/bin/claw.rs` — CLI entry point, `run_compile_once`, `BuildLanguage`
- `specs/35-Synthesis-Autoresearch.md` — the full spec with all GAN fixes
- `specs/33-Synthesis-Model-Interface.md` — SynthesisRequest interface reference
- `specs/32-Code-Synthesis-Pipeline.md` — artifact format reference

---

## Implementation order

Work in this exact sequence. Run `cargo test` after each task group. Do NOT batch.

---

### Task 1: AST changes (`src/ast.rs`)

**1a. Add `EvalCriterion`:**

```rust
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EvalCriterion {
    pub label:        String,
    pub question:     String,
    pub is_tsc:       bool,    // true when label starts with "tsc:" prefix
    pub span:         Span,
}
```

**1b. Add `EvalBlock`:**

```rust
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EvalBlock {
    pub runs:     Option<u32>,
    pub criteria: Vec<EvalCriterion>,
    pub span:     Span,
}
```

**1c. Add `TuneConfig`:**

```rust
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TuneConfig {
    pub iterations:               Option<u32>,
    pub runs:                     Option<u32>,
    pub judge:                    Option<String>,    // client name reference
    pub save_prompt:              Option<bool>,
    pub budget_usd:               Option<f64>,
    pub augment_examples:         Option<bool>,
    pub include_prompt_in_artifact: Option<bool>,
    pub judge_warning:            Option<bool>,
    pub span:                     Span,
}
```

**1d. Update `ToolDecl`** — add one field:

```rust
pub eval_block: Option<EvalBlock>,
```

**1e. Update `SynthesizerDecl`** — add one field:

```rust
pub tune: Option<TuneConfig>,
```

**After 1e:** Run `cargo test`. Fix all struct literal missing-field errors in test fixtures:
- Add `eval_block: None` to all `ToolDecl` fixtures.
- Add `tune: None` to all `SynthesizerDecl` fixtures.
- Update the insta snapshot with `INSTA_UPDATE=always cargo test`.

---

### Task 2: Parser changes (`src/parser.rs`)

Read the existing `tool_property_parser`, `synthesizer_decl`, and `brace_delimited` patterns before writing anything.

**2a. Add `ToolProperty::Eval(EvalBlock)` variant** to the existing `ToolProperty` enum.

**2b. Add `eval_block` parser:**

```rust
fn eval_block(input: &mut Input<'_>) -> PResult<EvalBlock> {
    // Parses:
    // eval {
    //     runs: <int>          // optional
    //     criteria {
    //         <label>: "<question>"
    //         ...
    //     }
    // }
}
```

Criteria parsing rules:
- Each criterion is `<label>: "<string>"` where label is an identifier (may contain `:`).
- A label starting with `tsc:` sets `is_tsc = true` and strips the prefix from the stored label.
  - Example: `tsc:compiles` → label `"compiles"`, is_tsc `true`.
- Labels must be unique within one `eval {}` block — use `verify_map` to reject duplicates.
- The question string must be non-empty — use `verify_map` to reject empty strings.

**2c. Extend `tool_property_parser`** — add:

```rust
"eval" => eval_block.map(ToolProperty::Eval),
```

And in the fold that builds `ToolDecl`, handle:

```rust
ToolProperty::Eval(eb) => { decl.eval_block = Some(eb); }
```

**2d. Add `tune_block` parser:**

```rust
fn tune_block(input: &mut Input<'_>) -> PResult<TuneConfig> {
    // Parses:
    // tune {
    //     iterations:                <int>     // optional
    //     runs:                      <int>     // optional
    //     judge:                     <ident>   // optional — client name
    //     save_prompt:               true|false
    //     budget_usd:                <float>
    //     augment_examples:          true|false
    //     include_prompt_in_artifact: true|false
    //     judge_warning:             true|false
    // }
}
```

**2e. Extend `synthesizer_decl` parser** to recognize `tune { ... }` as an optional block:

Inside the synthesizer property fold, add:

```rust
"tune" => { decl.tune = Some(tune_block.parse_next(input)?); }
```

**After Task 2:** Run `cargo test`. Fix any failures. Update snapshot if parser tests changed.

---

### Task 3: Semantic validation (`src/semantic/mod.rs` and `src/semantic/types.rs`)

**3a. Add new error/warning codes** to `src/errors.rs`:

```rust
// Warnings
W_T01_TooManyCriteria { tool: String, count: usize, span: Span },
W_T02_EvalWithoutUsing { tool: String, span: Span },
W_T03_SmallJudgeModel { synthesizer: String, judge: String, span: Span },

// Errors
E_T01_UndefinedJudgeClient { synthesizer: String, client: String, span: Span },
E_T02_InvalidBudget { synthesizer: String, value: f64, span: Span },
```

**3b. Add `validate_eval_blocks`** function called from the main `validate` entry point:

```rust
fn validate_eval_blocks(
    document: &Document,
    symbols: &SymbolTable,
    errors: &mut Vec<CompilerError>,
    warnings: &mut Vec<CompilerWarning>,
) {
    for tool in &document.tools {
        let Some(eval) = &tool.eval_block else { continue };

        // W-T01: more than 20 criteria
        if eval.criteria.len() > 20 {
            warnings.push(W_T01_TooManyCriteria {
                tool: tool.name.clone(),
                count: eval.criteria.len(),
                span: eval.span.clone(),
            });
        }

        // W-T02: eval block on a tool without using:
        if tool.using.is_none() {
            warnings.push(W_T02_EvalWithoutUsing {
                tool: tool.name.clone(),
                span: eval.span.clone(),
            });
        }
    }
}
```

**3c. Add `validate_tune_configs`** function:

```rust
fn validate_tune_configs(
    document: &Document,
    symbols: &SymbolTable,
    errors: &mut Vec<CompilerError>,
    warnings: &mut Vec<CompilerWarning>,
) {
    for synth in &document.synthesizers {
        let Some(tune) = &synth.tune else { continue };

        // E-T01: judge client not declared
        if let Some(judge_name) = &tune.judge {
            if !symbols.clients.contains_key(judge_name) {
                errors.push(E_T01_UndefinedJudgeClient {
                    synthesizer: synth.name.clone(),
                    client: judge_name.clone(),
                    span: tune.span.clone(),
                });
            }
        }

        // E-T02: budget_usd <= 0
        if let Some(budget) = tune.budget_usd {
            if budget <= 0.0 {
                errors.push(E_T02_InvalidBudget {
                    synthesizer: synth.name.clone(),
                    value: budget,
                    span: tune.span.clone(),
                });
            }
        }

        // W-T03: judge is a local/small model — detected by client provider
        // Check: if judge client's provider is "ollama" or "local", emit warning
        // unless tune.judge_warning == Some(false)
        if tune.judge_warning != Some(false) {
            if let Some(judge_name) = &tune.judge {
                if let Some(client) = symbols.clients.get(judge_name) {
                    let provider = client.provider.to_lowercase();
                    if provider == "ollama" || provider == "local" {
                        warnings.push(W_T03_SmallJudgeModel {
                            synthesizer: synth.name.clone(),
                            judge: judge_name.clone(),
                            span: tune.span.clone(),
                        });
                    }
                }
            }
        }
    }
}
```

**3d. Wire both new validators** into the main `validate` function alongside existing validators.

**After Task 3:** Run `cargo test`.

---

### Task 4: Artifact codegen (`src/codegen/artifact.rs`)

Update `build_artifact` to include the `eval` section per tool and respect `tune_prompt` privacy.

**4a. Extend `emit_tool`** — add the `eval` section:

```rust
fn emit_tool(t: &ToolDecl) -> Value {
    let mut obj = /* existing fields */;

    if let Some(eval) = &t.eval_block {
        obj["eval"] = json!({
            "runs": eval.runs.unwrap_or(10),
            "criteria": eval.criteria.iter().map(|c| json!({
                "label":    c.label,
                "question": c.question,
                "is_tsc":   c.is_tsc,
            })).collect::<Vec<_>>(),
        });
    }

    obj
}
```

**4b. Add `tune_prompt` injection (conditional)**

The artifact gains a `tune_prompts` section at the root that maps tool name → prompt string. This is only populated when `synthesizer.tune.include_prompt_in_artifact == true`. The prompt value comes from `claw.json` (read at compile time if it exists).

```rust
fn build_tune_prompts(document: &Document, project_root: &Path) -> Value {
    // Check if any synthesizer has include_prompt_in_artifact: true
    let should_include = document.synthesizers.iter().any(|s| {
        s.tune.as_ref()
            .and_then(|t| t.include_prompt_in_artifact)
            .unwrap_or(false)
    });

    if !should_include {
        return json!({});
    }

    // Read claw.json if it exists
    let claw_json_path = project_root.join("claw.json");
    if !claw_json_path.exists() {
        return json!({});
    }

    let raw = std::fs::read_to_string(&claw_json_path).unwrap_or_default();
    let config: serde_json::Value = serde_json::from_str(&raw).unwrap_or_default();
    config["synthesizer"]["prompts"].clone()
}
```

Add `"tune_prompts": build_tune_prompts(document, project_root)` to the root artifact object.

**4c. Update `generate_artifact` function signature** to take `project_root: &Path` (it already does if wired from `claw.rs`; if not, add it).

---

### Task 5: `synth-runner.js` codegen update (`src/codegen/synth_runner.rs`)

The synth runner must know to look up per-tool prompt overrides when `include_prompt_in_artifact` is false. Update the generated script:

**5a. Generated `synth-runner.js` — add tune prompt resolution:**

At the top of the generated script, add a prompt resolver that reads from `claw.json` directly when the artifact has no `tune_prompts` entry for the tool:

```javascript
// Resolve per-tool synthesis prompt override
function resolvePrompt(toolName, defaultPrompt) {
  // First: check artifact (only present if include_prompt_in_artifact = true)
  if (ARTIFACT.tune_prompts && ARTIFACT.tune_prompts[toolName]) {
    return ARTIFACT.tune_prompts[toolName];
  }
  // Second: check claw.json at runtime (for privacy-preserving mode)
  try {
    const { createRequire } = await import('module');
    const clawJsonPath = new URL('../claw.json', import.meta.url);
    const clawConfig = JSON.parse(await fs.readFile(new URL(clawJsonPath), 'utf8'));
    if (clawConfig?.synthesizer?.prompts?.[toolName]) {
      return clawConfig.synthesizer.prompts[toolName];
    }
  } catch { /* claw.json not present — use default */ }
  return defaultPrompt;
}
```

This is only emitted when any synthesizer in the document has `tune` configured. Otherwise the resolver is omitted to keep the script lean.

---

### Task 6: `claw tune` CLI module (`src/bin/tune.rs`)

This is the main deliverable. Create `src/bin/tune.rs` as a **separate binary** (not a subcommand of `claw.rs`).

Add to `Cargo.toml`:

```toml
[[bin]]
name = "claw-tune"
path = "src/bin/tune.rs"
```

The binary is invoked as `claw tune ...` from a shell wrapper (or directly as `claw-tune`). This spec treats it as a standalone implementation.

**6a. CLI argument parsing** — use `clap` (already in `Cargo.toml`):

```rust
#[derive(Parser)]
#[command(name = "claw-tune")]
struct Cli {
    /// Tune only the named tool
    #[arg(long)]
    tool: Option<String>,

    /// Override iteration count
    #[arg(long)]
    iterations: Option<u32>,

    /// Override runs per iteration
    #[arg(long)]
    runs: Option<u32>,

    /// Run evals but do not mutate prompt or save
    #[arg(long)]
    dry_run: bool,

    /// Print detailed criterion breakdown after each iteration
    #[arg(long)]
    report: bool,

    /// Path to claw.json config
    #[arg(long, default_value = "claw.json")]
    config: PathBuf,

    /// Path to the .clawa artifact (default: generated/artifact.clawa.json)
    #[arg(long, default_value = "generated/artifact.clawa.json")]
    artifact: PathBuf,
}
```

**6b. Core data structures:**

```rust
struct TuneSession {
    tool_name:        String,
    session_id:       String,         // "<ToolName>/<timestamp>"
    history_dir:      PathBuf,        // ~/.claw/tune-history/<tool>/<timestamp>/
    synthesizer_cfg:  SynthConfig,
    tune_cfg:         TuneParams,
    eval_cfg:         EvalParams,
    current_prompt:   String,
    best_prompt:      String,
    best_score:       f64,
    total_spend_usd:  f64,
}

struct TuneParams {
    iterations:  u32,
    runs:        u32,
    save_prompt: bool,
    budget_usd:  f64,
    augment_examples: bool,
}

struct EvalParams {
    criteria:        Vec<CriterionDef>,
    vitest_test_count: u32,
}

struct CriterionDef {
    label:    String,
    question: String,
    is_tsc:   bool,
}

struct IterationResult {
    iteration:      u32,
    score:          f64,
    run_results:    Vec<RunResult>,
    failing_criteria: Vec<FailingCriterion>,
}

struct RunResult {
    code:          String,
    vitest_passes: u32,
    tsc_passes:    u32,
    llm_passes:    u32,
    score:         f64,
    failed_criteria: Vec<String>,   // labels
}

struct FailingCriterion {
    criterion: CriterionDef,
    passes:    u32,
    runs:      u32,     // total runs (for "3/10" display)
    failed_samples: Vec<String>,    // up to 2 code samples that failed
}
```

**6c. Main loop:**

```rust
async fn run_tune_session(session: &mut TuneSession) -> anyhow::Result<TuneReport> {
    println!("claw tune — optimizing {} synthesis prompt", session.tool_name);
    println!(" synthesizer: {}", session.synthesizer_cfg.model);
    println!(" iterations: {} × {} runs = {} attempts",
        session.tune_cfg.iterations,
        session.tune_cfg.runs,
        session.tune_cfg.iterations * session.tune_cfg.runs);
    println!(" budget: ${:.2}", session.tune_cfg.budget_usd);
    println!();

    let max_iterations = session.tune_cfg.iterations;

    for iteration in 1..=max_iterations {
        // Check budget
        if session.total_spend_usd >= session.tune_cfg.budget_usd {
            println!("  budget limit reached (${:.2}), stopping.", session.total_spend_usd);
            break;
        }

        let iter_result = run_iteration(session, iteration).await?;

        let is_new_best = iter_result.score > session.best_score;
        if is_new_best {
            session.best_score = iter_result.score;
            session.best_prompt = session.current_prompt.clone();
        }

        // Print iteration summary
        let arrow = if is_new_best { "  ↑ new best" } else { "" };
        println!("[iter {:2}/{:2}] score: {:.2}  best: {:.2}{}",
            iteration, max_iterations,
            iter_result.score, session.best_score, arrow);
        if !iter_result.failing_criteria.is_empty() {
            let failing_str: Vec<String> = iter_result.failing_criteria.iter()
                .take(2)
                .map(|f| format!("{} ({}/{})", f.criterion.label, f.passes, f.runs))
                .collect();
            println!("              failing: {}", failing_str.join(", "));
        }

        // Save iteration to history
        save_iteration_history(session, iteration, &iter_result).await?;

        // Perfect score — stop early
        if iter_result.score >= 1.0 {
            println!("\n[iter {:2}/{:2}] score: 1.00 — PERFECT — stopping early", iteration, max_iterations);
            break;
        }

        // Mutate prompt (unless --dry-run or last iteration)
        if !session.dry_run_mode && iteration < max_iterations && !iter_result.failing_criteria.is_empty() {
            session.current_prompt = mutate_prompt(session, &iter_result).await?;
        }
    }

    // Save best prompt
    if session.tune_cfg.save_prompt && !session.dry_run_mode {
        save_best_prompt_to_claw_json(session).await?;
    }

    save_tune_report(session).await
}
```

**6d. Synthesize one run:**

```rust
async fn synthesize_once(session: &TuneSession, prompt: &str) -> anyhow::Result<String> {
    // Write synthesis request to temp dir: generated/__tune__/iter-<N>/
    // Call synth-runner.js (Node.js process) via stdin/stdout NDJSON bridge
    // (Same protocol as the existing synthesis pass in synth_runner.rs)
    // Returns: TypeScript source code string
    let temp_dir = session.history_dir
        .parent().unwrap()  // ~/.claw/tune-history/ToolName/
        .join(format!("__tune__/iter-{}", session.current_iteration));
    std::fs::create_dir_all(&temp_dir)?;

    // Write prompt override + synthesis request
    // Invoke: node generated/synth-runner.js with modified prompt
    // Parse synthesized TypeScript from response
    todo!("invoke synth-runner.js over NDJSON bridge")
}
```

**6e. `tsc:` criterion check:**

```rust
fn check_tsc_criterion(code: &str, tune_tsconfig: &Path) -> bool {
    // Write code to temp file
    let ts_file = tune_tsconfig.parent().unwrap().join("__eval__.ts");
    std::fs::write(&ts_file, code).unwrap();

    // Run tsc --project <tsconfig>
    let status = std::process::Command::new("tsc")
        .arg("--project")
        .arg(tune_tsconfig)
        .arg("--noEmit")
        .status();

    match status {
        Ok(s) => s.success(),
        Err(_) => false,   // tsc not installed → criterion fails
    }
}
```

The tune session writes `generated/__tune__/tsconfig.json` at startup:

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "NodeNext",
    "moduleResolution": "NodeNext",
    "strict": true,
    "noEmit": true
  },
  "include": ["../tools/*.ts", "../types.ts", "./__eval__.ts"]
}
```

Cleaned up at session end.

**6f. LLM judge call:**

```rust
async fn judge_code(
    code: &str,
    criteria: &[CriterionDef],
    judge_client: &SynthConfig,
) -> anyhow::Result<Vec<bool>> {
    // Build judge prompt:
    //   SYSTEM: "You are a TypeScript code reviewer. Answer each question with exactly 'YES' or 'NO'. ..."
    //   USER: "Code:\n```typescript\n<code>\n```\n\nQuestions:\n1. <question>\n2. ..."
    // Parse response: N lines of YES/NO
    // Any parse failure → criterion marked false
    // Returns: Vec<bool> parallel to criteria (tsc: criteria are excluded — already evaluated)
    todo!("call judge LLM client")
}
```

**6g. Score computation** — implement §11.8 formula exactly:

```rust
fn compute_score(
    vitest_passes: u32, vitest_count: u32,
    tsc_passes: u32,    tsc_count: u32,
    llm_passes: u32,    llm_count: u32,
) -> f64 {
    let vitest_term = if vitest_count > 0 { vitest_passes as f64 / vitest_count as f64 } else { 1.0 };
    let tsc_term    = if tsc_count    > 0 { tsc_passes    as f64 / tsc_count    as f64 } else { 0.0 };
    let llm_term    = if llm_count    > 0 { llm_passes    as f64 / llm_count    as f64 } else { 0.0 };

    // Number of active term types (for normalization to [0.0, 1.0])
    let term_count = 1.0  // vitest always active
        + if tsc_count > 0 { 1.0 } else { 0.0 }
        + if llm_count > 0 { 1.0 } else { 0.0 };

    (vitest_term + tsc_term + llm_term) / term_count
}
```

**6h. `runs` precedence** — implement §11.2 rule:

```rust
fn resolve_runs(eval_runs: Option<u32>, tune_runs: Option<u32>) -> u32 {
    // tool-level eval.runs > synthesizer-level tune.runs > default 10
    eval_runs.or(tune_runs).unwrap_or(10)
}
```

**6i. Prompt mutator call** — implement §4.4 + §11.3 (always includes full current prompt):

```rust
async fn mutate_prompt(
    session: &TuneSession,
    iter_result: &IterationResult,
) -> anyhow::Result<String> {
    // Collect ALL failing criteria (< 70% pass rate), not just worst 2
    let failing: Vec<&FailingCriterion> = iter_result.failing_criteria.iter()
        .filter(|f| (f.passes as f64 / f.runs as f64) < 0.70)
        .collect();

    if failing.is_empty() {
        return Ok(session.current_prompt.clone());
    }

    // Sort by pass rate ascending → worst 2 get example code samples
    let worst_two: Vec<&FailingCriterion> = failing.iter()
        .take(2)
        .copied()
        .collect();

    // Build mutator prompt (ALWAYS includes full current_prompt)
    let failing_lines: String = failing.iter().map(|f| {
        format!("- \"{}\" — passed {}/{} runs", f.criterion.question, f.passes, f.runs)
    }).collect::<Vec<_>>().join("\n");

    let sample_code: String = worst_two.iter().flat_map(|f| {
        f.failed_samples.iter().take(2).enumerate()
            .map(|(i, code)| format!("=== Failed sample {} (criterion: {}) ===\n{}", i+1, f.criterion.label, code))
    }).collect::<Vec<_>>().join("\n\n");

    let mutator_prompt = format!(
        "Current prompt:\n---\n{}\n---\n\nFailing criteria this iteration (< 70% pass rate):\n{}\n\nSynthesized code samples that FAILED the worst criteria:\n{}\n\nRewrite the synthesis prompt to fix these failures.",
        session.current_prompt,
        failing_lines,
        sample_code,
    );

    // Call mutator LLM (same client as synthesizer by default)
    // SYSTEM: "You are optimizing a synthesis prompt. Your goal: maximize the binary eval pass rate. Output ONLY the improved prompt. Do not explain."
    // Returns: new prompt string
    todo!("call mutator LLM")
}
```

**6j. History persistence:**

```rust
async fn save_iteration_history(
    session: &TuneSession,
    iteration: u32,
    result: &IterationResult,
) -> anyhow::Result<()> {
    let dir = &session.history_dir;
    std::fs::create_dir_all(dir)?;

    // iteration-NN-prompt.md
    let prompt_path = dir.join(format!("iteration-{:02}-prompt.md", iteration));
    std::fs::write(&prompt_path, &session.current_prompt)?;

    // iteration-NN-score.json
    let score_path = dir.join(format!("iteration-{:02}-score.json", iteration));
    let score_data = json!({
        "iteration": iteration,
        "score": result.score,
        "run_count": result.run_results.len(),
        "criteria_breakdown": result.failing_criteria.iter().map(|f| json!({
            "label": f.criterion.label,
            "passes": f.passes,
            "runs": f.runs,
            "pass_rate": f.passes as f64 / f.runs as f64,
        })).collect::<Vec<_>>(),
    });
    std::fs::write(&score_path, serde_json::to_string_pretty(&score_data)?)?;

    Ok(())
}
```

**6k. `claw.json` prompt persistence:**

```rust
async fn save_best_prompt_to_claw_json(session: &TuneSession) -> anyhow::Result<()> {
    let claw_json_path = session.config_path.clone(); // default: claw.json

    let mut config: serde_json::Value = if claw_json_path.exists() {
        serde_json::from_str(&std::fs::read_to_string(&claw_json_path)?)?
    } else {
        json!({})
    };

    // Ensure nested path: synthesizer.prompts.<ToolName>
    config
        .as_object_mut().unwrap()
        .entry("synthesizer").or_insert_with(|| json!({}))
        .as_object_mut().unwrap()
        .entry("prompts").or_insert_with(|| json!({}))
        .as_object_mut().unwrap()
        .insert(session.tool_name.clone(), json!(session.best_prompt));

    std::fs::write(&claw_json_path, serde_json::to_string_pretty(&config)?)?;
    println!("  Prompt saved to: {} → synthesizer.prompts.{}", claw_json_path.display(), session.tool_name);
    Ok(())
}
```

**6l. `augment_examples` staging** — implement §11.4 (NEVER writes to source):

```rust
async fn stage_discovered_examples(
    session: &TuneSession,
    best_run_codes: Vec<String>,
) -> anyhow::Result<()> {
    if !session.tune_cfg.augment_examples {
        return Ok(());
    }

    // Write to ~/.claw/tune-history/<tool>/best-examples.json — NOT to source .claw
    let examples_path = session.history_dir.join("best-examples.json");
    let examples = json!({
        "tool": session.tool_name,
        "session": session.session_id,
        "examples": best_run_codes.iter().take(3).map(|code| json!({ "code": code })).collect::<Vec<_>>(),
    });
    std::fs::write(&examples_path, serde_json::to_string_pretty(&examples)?)?;

    println!("\n  New examples discovered:");
    println!("    → review at {}", examples_path.display());
    println!("    → to apply: claw apply-examples {}", session.tool_name);
    Ok(())
}
```

**6m. Synthesis telemetry** — write to `~/.claw/synthesis-telemetry/` with the Spec 33 schema + `eval_results` field:

```rust
fn write_telemetry_record(
    session: &TuneSession,
    iteration: u32,
    run: u32,
    code: &str,
    run_result: &RunResult,
) -> anyhow::Result<()> {
    let telemetry_dir = home_dir()
        .unwrap_or_default()
        .join(".claw/synthesis-telemetry");
    std::fs::create_dir_all(&telemetry_dir)?;

    let record = json!({
        "tool":     session.tool_name,
        "prompt":   session.current_prompt,
        "output":   code,
        "quality": {
            "tests_passed": run_result.vitest_passes == session.eval_cfg.vitest_test_count,
            "attempt_number": run,
            "test_results": [],
            "eval_results": run_result.failed_criteria.iter().map(|label| json!({
                "criterion": label,
                "passed": false,
            })).chain(
                // passed criteria not in failed list
                session.eval_cfg.criteria.iter()
                    .filter(|c| !run_result.failed_criteria.contains(&c.label))
                    .map(|c| json!({ "criterion": c.label, "passed": true }))
            ).collect::<Vec<_>>(),
            "tune_iteration": iteration,
            "tune_session": session.session_id,
        }
    });

    let file_name = format!("{}-iter{}-run{}.json",
        session.tool_name, iteration, run);
    std::fs::write(telemetry_dir.join(file_name), serde_json::to_string(&record)?)?;
    Ok(())
}
```

**6n. `--dry-run` support** — when `--dry-run` is set:
- Skip the synthesis step — use existing `generated/tools/<ToolName>.ts` as the code sample.
- Run vitest, tsc, and LLM judge on existing code.
- Do NOT call the mutator.
- Do NOT save prompt to `claw.json`.
- Emit one iteration of eval results and exit.

**6o. API key check at startup:**

```rust
fn check_api_keys(synthesizer_cfg: &SynthConfig) -> anyhow::Result<()> {
    match synthesizer_cfg.provider.as_str() {
        "anthropic" => {
            if std::env::var("ANTHROPIC_API_KEY").is_err() {
                anyhow::bail!(
                    "ANTHROPIC_API_KEY is not set.\n\
                     Set the key or use a local client (provider = \"ollama\") for offline operation."
                );
            }
        }
        "openai" => {
            if std::env::var("OPENAI_API_KEY").is_err() {
                anyhow::bail!("OPENAI_API_KEY is not set.");
            }
        }
        "ollama" | "local" => {}  // no key required
        _ => {}
    }
    Ok(())
}
```

Call at the top of `main()` before the loop starts.

---

### Task 7: `claw apply-examples` command

Add a new subcommand to `src/bin/claw.rs` (or a separate `src/bin/apply-examples.rs`):

```rust
// claw apply-examples <ToolName>
// 1. Reads ~/.claw/tune-history/<ToolName>/*/best-examples.json (latest session)
// 2. Shows a unified diff of what would be added to the source .claw file
// 3. Prompts: "Apply these examples? [y/N]"
// 4. Only on 'y': appends examples {} entries to the tool declaration in source file
```

The diff shows the exact DSL syntax that would be added:

```
+ examples {
+     { input: { query: "rust programming" }, output: { url: "...", snippet: "..." } }
+ }
```

This is a safe alternative to `augment_examples` auto-writing to source.

---

### Task 8: `write_document` canonical hash update (`src/codegen/mod.rs`)

The `eval_block` and `tune` config are part of the document state and should affect cache invalidation.

**8a. Update `write_tool_decl`** to include eval_block in the hash:

```rust
fn write_tool_decl(output: &mut String, declaration: &ToolDecl) {
    // ... existing fields ...
    if let Some(eval) = &declaration.eval_block {
        write_tag(output, "eval_runs", &eval.runs.unwrap_or(10).to_string());
        for c in &eval.criteria {
            write_tag(output, "criterion", &format!("{}:{}", c.label, c.question));
        }
    }
}
```

**8b. Update `write_synthesizer_decl`** to include tune config:

```rust
fn write_synthesizer_decl(output: &mut String, s: &SynthesizerDecl) {
    // ... existing fields ...
    if let Some(tune) = &s.tune {
        write_tag(output, "tune_iterations", &tune.iterations.unwrap_or(20).to_string());
        write_tag(output, "tune_runs",       &tune.runs.unwrap_or(10).to_string());
        if let Some(j) = &tune.judge {
            write_tag(output, "tune_judge", j);
        }
    }
}
```

---

### Task 9: Test coverage

**9a. Parser tests** — add to `src/parser.rs` test module:

```rust
#[test]
fn parses_eval_block() {
    let source = r#"
tool WebSearch(query: string) -> SearchResult {
    using: fetch
    eval {
        runs: 10
        criteria {
            no_any:      "Does the code avoid 'any'?"
            url_encoded: "Is the query URL-encoded?"
        }
    }
}
"#;
    let doc = parse(source).expect("parse");
    let eval = doc.tools[0].eval_block.as_ref().expect("eval block");
    assert_eq!(eval.runs, Some(10));
    assert_eq!(eval.criteria.len(), 2);
    assert_eq!(eval.criteria[0].label, "no_any");
    assert!(!eval.criteria[0].is_tsc);
}

#[test]
fn parses_tsc_criterion() {
    let source = r#"
tool Foo(x: string) -> string {
    using: fetch
    eval {
        criteria {
            tsc:compiles: "tsc --noEmit"
            no_any: "Does the code avoid 'any'?"
        }
    }
}
"#;
    let doc = parse(source).expect("parse");
    let eval = doc.tools[0].eval_block.as_ref().expect("eval block");
    assert_eq!(eval.criteria[0].label, "compiles");
    assert!(eval.criteria[0].is_tsc);
    assert!(!eval.criteria[1].is_tsc);
}

#[test]
fn parses_tune_block_in_synthesizer() {
    let source = r#"
client MyClaude { provider = "anthropic" model = "claude-haiku-4-5-20251001" }
synthesizer DefaultSynth {
    client      = MyClaude
    temperature = 0.1
    tune {
        iterations:  15
        runs:        8
        judge:       MyClaude
        budget_usd:  3.50
        save_prompt: true
    }
}
"#;
    let doc = parse(source).expect("parse");
    let tune = doc.synthesizers[0].tune.as_ref().expect("tune config");
    assert_eq!(tune.iterations, Some(15));
    assert_eq!(tune.runs, Some(8));
    assert_eq!(tune.judge.as_deref(), Some("MyClaude"));
    assert!((tune.budget_usd.unwrap() - 3.50).abs() < 0.001);
    assert_eq!(tune.save_prompt, Some(true));
}

#[test]
fn rejects_eval_with_empty_question() {
    let source = r#"
tool Foo(x: string) -> string {
    using: fetch
    eval {
        criteria {
            empty_q: ""
        }
    }
}
"#;
    assert!(parse(source).is_err());
}
```

**9b. Semantic tests** — add to `src/semantic/mod.rs` test module:

```rust
#[test]
fn warns_eval_without_using() {
    let source = r#"
tool NoUsing(x: string) -> string {
    eval {
        criteria { no_any: "Does the code avoid 'any'?" }
    }
}
"#;
    let doc = parse(source).expect("parse");
    let result = validate(&doc);
    assert!(result.warnings.iter().any(|w| matches!(w, W_T02_EvalWithoutUsing { .. })));
}

#[test]
fn warns_too_many_criteria() {
    // Build a tool with 21 criteria
    // ...
    let result = validate(&doc);
    assert!(result.warnings.iter().any(|w| matches!(w, W_T01_TooManyCriteria { .. })));
}

#[test]
fn errors_on_undefined_judge_client() {
    let source = r#"
client MyClaude { provider = "anthropic" model = "claude-haiku-4-5-20251001" }
synthesizer DefaultSynth {
    client = MyClaude
    tune { judge: NonexistentClient }
}
"#;
    let doc = parse(source).expect("parse");
    let result = validate(&doc);
    assert!(result.errors.iter().any(|e| matches!(e, E_T01_UndefinedJudgeClient { .. })));
}

#[test]
fn errors_on_invalid_budget() {
    // tune { budget_usd: -1.0 }
    // ...
    let result = validate(&doc);
    assert!(result.errors.iter().any(|e| matches!(e, E_T02_InvalidBudget { .. })));
}
```

**9c. Score formula tests** (`src/bin/tune.rs` test module):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn score_all_active_terms() {
        // vitest: 8/10, tsc: 1/1, llm: 7/10
        let s = compute_score(8, 10, 1, 1, 7, 10);
        let expected = (0.8 + 1.0 + 0.7) / 3.0;
        assert!((s - expected).abs() < 0.001);
    }

    #[test]
    fn score_no_tsc_criteria() {
        // vitest: 5/10, tsc: none, llm: 8/10
        let s = compute_score(5, 10, 0, 0, 8, 10);
        let expected = (0.5 + 0.8) / 2.0;
        assert!((s - expected).abs() < 0.001);
    }

    #[test]
    fn score_no_vitest_tests() {
        // vitest: none → 1.0, tsc: none, llm: 6/10
        let s = compute_score(0, 0, 0, 0, 6, 10);
        let expected = (1.0 + 0.6) / 2.0;
        assert!((s - expected).abs() < 0.001);
    }

    #[test]
    fn score_only_vitest() {
        // vitest: 10/10, no tsc, no llm
        let s = compute_score(10, 10, 0, 0, 0, 0);
        assert!((s - 1.0).abs() < 0.001);
    }

    #[test]
    fn runs_precedence() {
        assert_eq!(resolve_runs(Some(5), Some(20)), 5);   // eval wins
        assert_eq!(resolve_runs(None,    Some(20)), 20);  // tune wins
        assert_eq!(resolve_runs(None,    None     ), 10); // default
    }
}
```

---

### Task 10: Final verification

```bash
# All tests pass (including new ones)
INSTA_UPDATE=always ~/.cargo/bin/cargo test

# Both binaries build
~/.cargo/bin/cargo build --bin claw
~/.cargo/bin/cargo build --bin claw-tune

# End-to-end: compile a .claw file with eval{} and tune{}
cat > /tmp/test35.claw << 'EOF'
type SearchResult {
    url:     string
    snippet: string
}

client MyClaude {
    provider = "anthropic"
    model    = "claude-haiku-4-5-20251001"
}

synthesizer DefaultSynth {
    client      = MyClaude
    temperature = 0.1
    tune {
        iterations:  5
        runs:        3
        judge:       MyClaude
        budget_usd:  1.00
        save_prompt: true
    }
}

tool WebSearch(query: string) -> SearchResult {
    using:       fetch
    synthesizer: DefaultSynth
    description: "Searches the web for a query. Returns top URL and snippet."
    eval {
        runs: 3
        criteria {
            tsc:compiles:  "tsc --noEmit"
            no_any:        "Does the code avoid the TypeScript 'any' type?"
            url_encoded:   "Is the query parameter URL-encoded before use in a fetch call?"
            error_handled: "Does the code have a try/catch or .catch() around the fetch call?"
        }
    }
    test {
        input:  { query: "rust language" }
        expect: { url: !empty, snippet: !empty }
    }
}
EOF

cd /tmp && ~/.cargo/bin/claw build test35.claw

# Verify artifact has eval section
node -e "
  const a = JSON.parse(require('fs').readFileSync('/tmp/generated/artifact.clawa.json', 'utf8'));
  const tool = a.tools.find(t => t.name === 'WebSearch');
  console.log('eval.runs:', tool.eval?.runs);
  console.log('criteria count:', tool.eval?.criteria?.length);
  console.log('tsc criterion:', tool.eval?.criteria?.find(c => c.is_tsc)?.label);
"

# Expected output:
# eval.runs: 3
# criteria count: 4
# tsc criterion: compiles
```

---

## Invariants — never violate these

1. **All tests pass after every task group.** Run `INSTA_UPDATE=always ~/.cargo/bin/cargo test` after each task. Fix all failures before moving on.
2. **`eval {}` is NEVER executed during `claw build`.** It is deserialized into the artifact only. The autoresearch loop is `claw-tune` exclusively.
3. **`tsc:` prefix is stripped at parse time** — the stored `EvalCriterion.label` contains only the part after `tsc:`. The `is_tsc: true` flag signals the evaluation path.
4. **Score formula always divides by an active term count** — never divide by zero. If no vitest tests exist, that term is 1.0 (passes trivially). If no tsc/llm criteria exist, those terms are 0.0 and excluded from normalization.
5. **`augment_examples` NEVER writes to the source `.claw` file** — only to `~/.claw/tune-history/<tool>/best-examples.json`. The `claw apply-examples` command is the only way to apply them.
6. **`tune_prompt` is NOT in the artifact by default** — `include_prompt_in_artifact` defaults to `false`. Only embed when explicitly set to `true`. Users add `claw.json` to `.gitignore` for sensitive prompts.
7. **`runs` precedence**: `eval { runs }` > `tune { runs }` > default 10. Never ignore tool-level override.
8. **Mutator always receives the full current synthesis prompt** — not a summary. Failing criteria threshold is < 70% pass rate (not just "worst 2").
9. **Tune writes synthesized files to `generated/__tune__/iter-<N>/`** — never to `generated/tools/`. The main build output is never polluted by tune runs. Temp directory is cleaned up at session end.
10. **API key check at startup** — `claw tune` aborts with a clear error message before the loop if a cloud provider has no key set.
11. **All `Statement` match arms include `Statement::Reason { .. }`** — never add a match without it.
12. **`verify_map` is always used in parser validation** — never `try_map` returning `Err(ContextError)`.
