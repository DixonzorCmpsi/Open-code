# Implementation Prompt: Specs 36 + 37 — Synthesis Repair Loop & Spec Autoresearch

**Project:** Claw DSL compiler (`clawc`) in Rust — `/Users/dixon.zor/Documents/Open-code`
**Specs:** `specs/36-Synthesis-Repair-Loop.md` + `specs/37-Spec-Autoresearch.md` (with all GAN + spec-check amendments)
**Prerequisites:**
- Specs 32/33 synthesis pipeline implemented (`synth-runner.js`, `artifact.clawa.json`, NDJSON bridge)
- All tests pass: run `INSTA_UPDATE=always ~/.cargo/bin/cargo test` before starting

---

## Orientation — read these first

Before writing any code, read:

1. `specs/36-Synthesis-Repair-Loop.md` — full spec + §13 spec-check amendments
2. `specs/36-GAN-Audit.md` — 12 gaps and their fixes
3. `specs/37-Spec-Autoresearch.md` — full spec + §10 spec-check amendments
4. `src/ast.rs` — existing AST (you are adding `RetryConfig` to `SynthesizerDecl`)
5. `src/codegen/synth_runner.rs` — existing synth-runner.js codegen (you are extending it)
6. `src/bin/claw.rs` — existing CLI and synthesis wiring

Use `AGENT.md §8` (Spec Index) to find additional specs when a cross-reference is unclear.

---

## Part A: Spec 36 — Synthesis Repair Loop

Work Tasks A1–A8 in order. Run `cargo test` after each task.

---

### A1: AST changes (`src/ast.rs`)

Add these types and update `SynthesizerDecl`:

```rust
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum RetryStrategy {
    Repair,
    Rewrite,
    RepairThenRewrite,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PriceOverride {
    pub input_per_million:  f64,
    pub output_per_million: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RetryConfig {
    pub max_attempts:         Option<u32>,
    pub strategy:             Option<RetryStrategy>,
    pub compile_repair_limit: Option<u32>,
    pub on_stuck:             Option<RetryStrategy>,
    pub budget_usd:           Option<f64>,
    pub price_override:       Option<PriceOverride>,
    pub span:                 Span,
}

// In SynthesizerDecl — add:
pub retry: Option<RetryConfig>,
```

Fix all test fixtures: add `retry: None` to every `SynthesizerDecl` literal. Run `INSTA_UPDATE=always cargo test` to update snapshots.

---

### A2: Parser (`src/parser.rs`)

**CRITICAL:** `repair_then_rewrite` must be tried before `repair` — they share a prefix. Always alt in longest-match order:

```rust
fn retry_strategy(input: &mut Input<'_>) -> PResult<RetryStrategy> {
    alt((
        lexeme("repair_then_rewrite").map(|_| RetryStrategy::RepairThenRewrite),
        lexeme("repair").map(|_| RetryStrategy::Repair),
        lexeme("rewrite").map(|_| RetryStrategy::Rewrite),
    )).parse_next(input)
}

fn retry_block(input: &mut Input<'_>) -> PResult<RetryConfig> {
    // Parse: retry { max_attempts: <int>, strategy: <RetryStrategy>,
    //   compile_repair_limit: <int>, on_stuck: <RetryStrategy>,
    //   budget_usd: <float>,
    //   price_per_million_tokens { input: <float>, output: <float> } }
}
```

Add `"retry" => { decl.retry = Some(retry_block.parse_next(input)?); }` in the synthesizer property fold.

Parser tests to add:

```rust
#[test]
fn parses_retry_repair_then_rewrite() {
    // Verify repair_then_rewrite parses correctly (not just repair)
    let source = r#"
client C { provider = "anthropic" model = "x" }
synthesizer S { client = C retry { strategy: repair_then_rewrite max_attempts: 4 } }
"#;
    let doc = parse(source).expect("parse");
    assert_eq!(
        doc.synthesizers[0].retry.as_ref().unwrap().strategy,
        Some(RetryStrategy::RepairThenRewrite)
    );
}

#[test]
fn parses_retry_repair_not_confused_with_repair_then_rewrite() {
    let source = r#"
client C { provider = "anthropic" model = "x" }
synthesizer S { client = C retry { strategy: repair max_attempts: 3 } }
"#;
    let doc = parse(source).expect("parse");
    assert_eq!(
        doc.synthesizers[0].retry.as_ref().unwrap().strategy,
        Some(RetryStrategy::Repair)
    );
}

#[test]
fn parses_full_retry_block() {
    let source = r#"
client C { provider = "anthropic" model = "x" }
synthesizer S {
    client = C
    retry {
        max_attempts:         4
        strategy:             repair_then_rewrite
        compile_repair_limit: 2
        on_stuck:             rewrite
        budget_usd:           0.75
    }
}
"#;
    let doc = parse(source).expect("parse");
    let r = doc.synthesizers[0].retry.as_ref().unwrap();
    assert_eq!(r.max_attempts, Some(4));
    assert_eq!(r.compile_repair_limit, Some(2));
    assert!((r.budget_usd.unwrap() - 0.75).abs() < 0.001);
}
```

---

### A3: Semantic validation (`src/semantic/mod.rs`, `src/errors.rs`)

Add error/warning codes:

```rust
// Errors
E_R01_SynthesisExhausted { tool: String, attempts: u32, last_failure: String, span: Span },
E_R02_CompileLimitNoRewrite { tool: String, limit: u32, span: Span },   // note attached to E-R01

// Warnings
W_R01_RetryIgnoredByTune { synthesizer: String, span: Span },
W_R02_InvalidRetryConfig { synthesizer: String, reason: String, span: Span },
W_R03_RepairBudgetLow { synthesizer: String, budget: f64, span: Span },
W_R04_TscNotFound { span: Span },   // emitted at runtime, not compile time
```

Add `validate_retry_configs`:

```rust
fn validate_retry_configs(document: &Document, warnings: &mut Vec<CompilerWarning>) {
    for synth in &document.synthesizers {
        let Some(retry) = &synth.retry else { continue };
        let max = retry.max_attempts.unwrap_or(1);

        // W-R02: compile_repair_limit will be clamped
        if let Some(crl) = retry.compile_repair_limit {
            if crl >= max {
                warnings.push(W_R02_InvalidRetryConfig {
                    synthesizer: synth.name.clone(),
                    reason: format!("compile_repair_limit={crl} >= max_attempts={max}, clamped to {}", max.saturating_sub(1)),
                    span: retry.span.clone(),
                });
            }
        }

        // W-R02: max_attempts=1 with strategy set
        if max == 1 && retry.strategy.is_some() {
            warnings.push(W_R02_InvalidRetryConfig {
                synthesizer: synth.name.clone(),
                reason: "max_attempts=1 means no retries — strategy is ignored".to_owned(),
                span: retry.span.clone(),
            });
        }

        // W-R03: budget too low
        if matches!(retry.budget_usd, Some(b) if b < 0.10) {
            warnings.push(W_R03_RepairBudgetLow {
                synthesizer: synth.name.clone(),
                budget: retry.budget_usd.unwrap(),
                span: retry.span.clone(),
            });
        }
    }
}
```

---

### A4: Artifact codegen (`src/codegen/artifact.rs`)

Extend `emit_synthesizer` to include resolved retry config:

```rust
if let Some(retry) = &s.retry {
    let max = retry.max_attempts.unwrap_or(1);
    let compile_limit = retry.compile_repair_limit
        .map(|v| v.min(max.saturating_sub(1)))
        .unwrap_or(max.saturating_sub(1));

    obj["retry"] = json!({
        "max_attempts":         max,
        "strategy":             emit_retry_strategy(&retry.strategy),
        "compile_repair_limit": compile_limit,
        "on_stuck":             emit_retry_strategy(&retry.on_stuck),
        "budget_usd":           retry.budget_usd.unwrap_or(0.50),
    });
}

fn emit_retry_strategy(s: &Option<RetryStrategy>) -> &'static str {
    match s {
        None | Some(RetryStrategy::Repair)           => "repair",
        Some(RetryStrategy::Rewrite)                 => "rewrite",
        Some(RetryStrategy::RepairThenRewrite)       => "repair_then_rewrite",
    }
}
```

---

### A5: `synth-runner.js` codegen (`src/codegen/synth_runner.rs`)

Extend the generated `synth-runner.js` template with two additions:

**5a. `buildPrompt(request)` function** — branches on `request.repair_context`:

```javascript
function buildPrompt(request) {
  if (!request.repair_context) {
    return buildSynthesisPrompt(request);   // existing function
  }
  const ctx = request.repair_context;
  const typeDefs = (request.type_definitions || []).map(t => t.definition).join('\n\n');
  const sig = `${request.tool_name}(${formatArgs(request.arguments)}) -> ${request.return_type}`;

  if (ctx.strategy === 'rewrite') {
    return {
      system: SYNTHESIS_SYSTEM_PROMPT,
      user: [
        `## Synthesis Target\nTool: ${sig}`,
        `\n## Type Definitions\n${typeDefs}`,
        `\n## Capability\nusing: ${request.using ?? 'none'}`,
        `\n## Note\nPrevious synthesis attempts failed. Generate a completely new implementation.`,
      ].join(''),
    };
  }

  const tierLabel = ctx.tier === 'compile' ? 'Compilation Errors' : 'Test Failures';
  const system = ctx.tier === 'compile'
    ? 'You are fixing TypeScript compilation errors. Output ONLY the corrected TypeScript file. Do not explain. Do not include markdown fences.'
    : 'You are fixing TypeScript code that fails unit tests. Output ONLY the corrected TypeScript file. Do not explain.';

  return {
    system,
    user: [
      `## Synthesis Target\nTool: ${sig}\nAttempt ${ctx.attempt} of ${request.retry?.max_attempts ?? 1}`,
      `\n## Type Definitions\n${typeDefs}`,
      `\n## Capability\nusing: ${request.using ?? 'none'}`,
      `\n## Broken Code\n${ctx.broken_code}`,
      `\n## ${tierLabel}\n${ctx.errors}`,
      `\n${ctx.tier === 'compile'
        ? "Fix all compilation errors. Preserve the tool's implementation intent. Output only the corrected TypeScript."
        : 'Fix the code to pass the failing tests. Preserve working behavior. Output only the corrected TypeScript.'}`,
    ].join(''),
  };
}
```

**5b. `truncateTscErrors(raw)` function** — per §13.12 algorithm:

```javascript
function truncateTscErrors(raw) {
  const lines = raw.split('\n');
  const result = [];
  if (lines.length > 0) result.push(lines[0]);  // always line 1

  const seen = new Set();
  for (const l of lines.slice(1)) {
    const m = l.match(/error TS(\d+):/);
    if (m && !seen.has(m[1]) && result.length < 16) {
      seen.add(m[1]);
      result.push(l);
    }
  }

  const summary = lines.find(l => /Found \d+ error/.test(l));
  if (summary && !result.includes(summary)) result.push(summary);

  const ctx = lines.filter(l => /\.ts\(\d+,\d+\)/.test(l)).slice(0, 5);
  for (const l of ctx) {
    if (result.length >= 20) break;
    if (!result.includes(l)) result.push(l);
  }

  return result.join('\n');
}
```

Only emit these functions when any synthesizer in the document has `retry` configured.

---

### A6: Repair orchestrator (`src/bin/claw.rs`)

This is the largest task. Add `synthesize_with_repair` and its helpers.

**Key types:**

```rust
enum SynthesisError {
    Exhausted       { tool: String, attempts: u32, last_failure: String },
    CompileLimitNoRewrite { tool: String, limit: u32 },
    BudgetExceeded  { tool: String, spend: f64 },
    NoSynthesizer,
    Io(std::io::Error),
}

#[derive(Clone, Copy, PartialEq)]
enum RepairTier { Cold, Compile, Test }

struct AttemptResult {
    code:           String,
    tsc_ok:         bool,
    tsc_errors:     String,
    vitest_ok:      bool,
    vitest_output:  String,
    spend_usd:      f64,
}
```

**`resolve_compile_limit`:**

```rust
fn resolve_compile_limit(retry: &RetryConfig) -> u32 {
    let max = retry.max_attempts.unwrap_or(1).saturating_sub(1);
    retry.compile_repair_limit.map(|v| v.min(max)).unwrap_or(max)
}
```

**`effective_strategy`** — handles `repair_then_rewrite` last-attempt rewrite:

```rust
fn effective_strategy(strategy: &RetryStrategy, attempt: u32, max: u32) -> RetryStrategy {
    match strategy {
        RetryStrategy::RepairThenRewrite if attempt >= max - 1 => RetryStrategy::Rewrite,
        RetryStrategy::RepairThenRewrite                       => RetryStrategy::Repair,
        other => other.clone(),
    }
}
```

**Stuck detection:**

```rust
fn is_compile_stuck(cur_errors: u32, prev: Option<u32>) -> bool {
    prev.map(|p| cur_errors >= p).unwrap_or(false)
}
fn is_test_stuck(cur_passing: u32, prev: Option<u32>) -> bool {
    prev.map(|p| cur_passing <= p).unwrap_or(false)
}
```

**Price table** (conservative 2×):

```rust
fn price_per_million(model: &str) -> (f64, f64) {
    if model.starts_with("claude-haiku")  { return (0.50,  1.25)  }
    if model.starts_with("claude-sonnet") { return (3.00,  15.00) }
    if model.starts_with("claude-opus")   { return (15.00, 75.00) }
    if model.starts_with("gpt-4o-mini")   { return (0.30,  1.20)  }
    if model.starts_with("gpt-4o")        { return (5.00,  15.00) }
    (5.00, 15.00)
}
```

**`synthesize_with_repair` loop — implement exactly per spec §4 and §13:**

```rust
fn synthesize_with_repair(
    tool: &ToolDecl,
    document: &Document,
    project_root: &Path,
    verbose: bool,
) -> Result<String, SynthesisError> {
    let synth          = resolve_synthesizer(tool, document).ok_or(SynthesisError::NoSynthesizer)?;
    let retry          = synth.retry.as_ref();
    let max_attempts   = retry.and_then(|r| r.max_attempts).unwrap_or(1);
    let strategy       = retry.and_then(|r| r.strategy.clone()).unwrap_or(RetryStrategy::Repair);
    let compile_limit  = retry.map(resolve_compile_limit).unwrap_or(0);
    let on_stuck       = retry.and_then(|r| r.on_stuck.clone()).unwrap_or(RetryStrategy::Rewrite);
    let budget         = retry.and_then(|r| r.budget_usd).unwrap_or(0.50);

    clear_repair_dir(project_root, &tool.name);
    ensure_repair_gitignore(project_root);

    let mut total_spend     = 0.0_f64;
    let mut compile_repairs = 0_u32;
    let mut prev_tsc_count: Option<u32>  = None;
    let mut prev_pass_count: Option<u32> = None;
    let mut history: Vec<(RepairTier, AttemptResult)> = Vec::new();
    let mut repair_ctx: Option<RepairContextJson> = None;

    for attempt in 1..=max_attempts {
        if total_spend >= budget {
            save_repair_history(project_root, &tool.name, &history);
            return Err(SynthesisError::BudgetExceeded { tool: tool.name.clone(), spend: total_spend });
        }

        let result = run_one_synthesis(tool, document, project_root, repair_ctx.as_ref())?;
        save_attempt_file(project_root, &tool.name, attempt, &result.code);
        total_spend += result.spend_usd;

        if verbose { print_attempt(tool, attempt, max_attempts, &result); }

        // ── Compile check ─────────────────────────────────────────────
        if !result.tsc_ok {
            if attempt >= max_attempts {
                save_repair_history(project_root, &tool.name, &history);
                return Err(SynthesisError::Exhausted {
                    tool: tool.name.clone(), attempts: max_attempts,
                    last_failure: result.tsc_errors.clone(),
                });
            }

            let cur = count_tsc_errors(&result.tsc_errors);
            let stuck = is_compile_stuck(cur, prev_tsc_count);
            prev_tsc_count = Some(cur);
            history.push((RepairTier::Compile, result.clone()));

            if stuck {
                repair_ctx = Some(make_rewrite_ctx(attempt));
            } else if compile_repairs >= compile_limit {
                let strat = &strategy;
                if matches!(strat, RetryStrategy::Rewrite | RetryStrategy::RepairThenRewrite)
                    || matches!(on_stuck, RetryStrategy::Rewrite)
                {
                    repair_ctx = Some(make_rewrite_ctx(attempt));
                } else {
                    save_repair_history(project_root, &tool.name, &history);
                    return Err(SynthesisError::CompileLimitNoRewrite {
                        tool: tool.name.clone(), limit: compile_limit,
                    });
                }
            } else {
                compile_repairs += 1;
                let eff = effective_strategy(&strategy, attempt, max_attempts);
                repair_ctx = Some(make_repair_ctx(
                    attempt, RepairTier::Compile, eff,
                    &result.code, &truncate_tsc_errors(&result.tsc_errors),
                ));
            }
            continue;
        }

        // ── Test check ────────────────────────────────────────────────
        if !result.vitest_ok {
            if attempt >= max_attempts {
                save_repair_history(project_root, &tool.name, &history);
                return Err(SynthesisError::Exhausted {
                    tool: tool.name.clone(), attempts: max_attempts,
                    last_failure: extract_vitest_failure(&result.vitest_output),
                });
            }

            let cur = count_passing_tests(&result.vitest_output);
            let stuck = is_test_stuck(cur, prev_pass_count);
            prev_pass_count = Some(cur);
            history.push((RepairTier::Test, result.clone()));

            if stuck {
                repair_ctx = Some(make_rewrite_ctx(attempt));
            } else {
                let eff = effective_strategy(&strategy, attempt, max_attempts);
                repair_ctx = Some(make_repair_ctx(
                    attempt, RepairTier::Test, eff,
                    &result.code, &result.vitest_output,
                ));
            }
            continue;
        }

        // ── Success ───────────────────────────────────────────────────
        if verbose && attempt > 1 {
            println!("[synth] {} done ({} attempts, ~${:.2})", tool.name, attempt, total_spend);
        }
        return Ok(result.code);
    }

    unreachable!()
}
```

**Repair history on failure** (only on `SynthesisError::Exhausted` or `CompileLimitNoRewrite`):

```rust
fn save_repair_history(project_root: &Path, tool_name: &str, history: &[(RepairTier, AttemptResult)]) {
    let ts = chrono::Utc::now().format("%Y-%m-%dT%H-%M-%S");
    let dir = dirs::home_dir().unwrap_or_default()
        .join(format!(".claw/repair-history/{tool_name}/{ts}"));
    let _ = std::fs::create_dir_all(&dir);

    for (i, (tier, r)) in history.iter().enumerate() {
        let n = i + 1;
        let _ = std::fs::write(dir.join(format!("attempt-{n}.ts")), &r.code);
        match tier {
            RepairTier::Compile | RepairTier::Cold =>
                { let _ = std::fs::write(dir.join(format!("attempt-{n}-tsc.txt")), &r.tsc_errors); }
            RepairTier::Test =>
                { let _ = std::fs::write(dir.join(format!("attempt-{n}-vitest.txt")), &r.vitest_output); }
        }
    }

    let summary = serde_json::json!({
        "tool": tool_name, "timestamp": ts.to_string(),
        "attempts": history.len(),
        "total_spend": history.iter().map(|(_, r)| r.spend_usd).sum::<f64>(),
    });
    let _ = std::fs::write(dir.join("repair-summary.json"), serde_json::to_string_pretty(&summary).unwrap());
}
```

**Wire into `run_compile_once`** — replace the current single-shot synthesis call:

```rust
match synthesize_with_repair(tool, &document, project_root, verbose) {
    Ok(ts_code) => write_tool_ts(project_root, &tool.name, &ts_code)?,
    Err(SynthesisError::Exhausted { tool, attempts, last_failure }) => {
        errors.push(CompilerError::E_R01_SynthesisExhausted { tool, attempts, last_failure, span: tool_decl.span.clone() });
    }
    Err(SynthesisError::CompileLimitNoRewrite { tool, limit }) => {
        errors.push(CompilerError::E_R01_SynthesisExhausted { tool: tool.clone(), attempts: 0, last_failure: format!("compile_repair_limit={limit} reached"), span: tool_decl.span.clone() });
        notes.push(format!("E-R02: compile_repair_limit={limit} reached, strategy has no rewrite fallback\n  → consider: strategy: repair_then_rewrite"));
    }
    Err(SynthesisError::BudgetExceeded { tool, spend }) => {
        warnings.push(CompilerWarning::RepairBudgetExceeded { tool, spend });
    }
    Err(SynthesisError::NoSynthesizer) => { /* skip — tool not synthesis-path */ }
    Err(SynthesisError::Io(e)) => return Err(e.into()),
}
```

---

### A7: `write_document` hash update (`src/codegen/mod.rs`)

```rust
fn write_synthesizer_decl(output: &mut String, s: &SynthesizerDecl) {
    // ... existing fields ...
    if let Some(r) = &s.retry {
        write_tag(output, "retry_max",      &r.max_attempts.unwrap_or(1).to_string());
        write_tag(output, "retry_strategy", &format!("{:?}", r.strategy));
        write_tag(output, "retry_budget",   &r.budget_usd.unwrap_or(0.50).to_string());
    }
}
```

---

### A8: Tests

**Unit tests (add to `src/bin/claw.rs` or `tests/repair.rs`):**

```rust
#[test] fn resolve_compile_limit_defaults()           { /* max=4, no user value → 3 */ }
#[test] fn resolve_compile_limit_clamps()             { /* user=10, max=4 → 3 */ }
#[test] fn effective_strategy_repair_then_rewrite()   { /* attempt=3 of 4 → Rewrite */ }
#[test] fn effective_strategy_repair_not_last()       { /* attempt=2 of 4 → Repair */ }
#[test] fn stuck_compile_same_count()                 { /* cur=5, prev=5 → true */ }
#[test] fn stuck_compile_more_errors()                { /* cur=6, prev=5 → true */ }
#[test] fn stuck_compile_fewer_errors()               { /* cur=4, prev=5 → false */ }
#[test] fn stuck_test_same_passing()                  { /* cur=3, prev=3 → true */ }
#[test] fn stuck_test_more_passing()                  { /* cur=4, prev=3 → false */ }
#[test] fn price_table_haiku()                        { /* "claude-haiku-4-5..." → (0.50, 1.25) */ }
#[test] fn price_table_fallback()                     { /* "unknown-xyz" → (5.0, 15.0) */ }
```

---

## Part B: Spec 37 — Spec Autoresearch

Work Tasks B1–B5 in order after Part A passes all tests.

---

### B1: CLI subcommands (`src/bin/claw.rs`)

Add three new subcommands to the `claw` binary using `clap`:

```
claw spec-check <file> [--criteria <path>] [--client <name>] [--report]
claw spec-tune  <file> [--iterations <n>] [--dry-run] [--client <name>]
claw spec-cross-check <dir> [--client <name>]
```

Implement as separate handler functions, not a new binary. These commands do NOT touch `src/ast.rs`, `src/parser.rs`, or any codegen module.

---

### B2: API key check at startup

Before any LLM call in `spec-check` / `spec-tune`:

```rust
fn check_spec_check_api_key(client_provider: &str) -> anyhow::Result<()> {
    match client_provider {
        "anthropic" => {
            if std::env::var("ANTHROPIC_API_KEY").is_err() {
                anyhow::bail!("ANTHROPIC_API_KEY not set.\nSet the key or use --client with a local Ollama client.");
            }
        }
        "openai" => {
            if std::env::var("OPENAI_API_KEY").is_err() {
                anyhow::bail!("OPENAI_API_KEY not set.");
            }
        }
        "ollama" | "local" => {}
        _ => {}
    }
    Ok(())
}
```

---

### B3: `spec-check` implementation

```rust
async fn cmd_spec_check(
    file: &Path,
    criteria_path: Option<&Path>,
    client_name: Option<&str>,
    report: bool,
) -> anyhow::Result<()> {
    // 1. Read spec file — error if missing, warn W-SC02 if < 3 sections
    let spec_text = std::fs::read_to_string(file)
        .with_context(|| format!("error: file not found: {}", file.display()))?;

    let section_count = spec_text.lines().filter(|l| l.starts_with("## ")).count();
    if spec_text.trim().is_empty() || section_count < 3 {
        eprintln!("warning W-SC02: spec may be incomplete ({section_count} sections)");
        println!("Score: 0/16 (spec too short to evaluate)");
        return Ok(());
    }

    // 2. Load criteria (built-in or custom)
    let criteria = load_criteria(criteria_path)?;

    // 3. Check API key
    let provider = resolve_provider(client_name)?;
    check_spec_check_api_key(&provider)?;

    // 4. Call LLM judge — single call, all criteria
    let result = call_spec_judge(&spec_text, &criteria, client_name).await?;

    // 5. Print results
    print_spec_check_results(&result, report);

    // 6. Write history
    write_spec_check_history(file, &result).await?;

    Ok(())
}
```

**Judge prompt:**

```
SYSTEM:
You are auditing a technical specification document.
For each question below, answer with exactly "YES" or "NO" on its own line.
If NO, append a single sentence explanation after a tab character.
Format: YES  or  NO\t<one-sentence explanation>

USER:
Specification:
---
<spec_text>
---

Questions:
1. Does every new DSL construct have a .claw syntax example?
2. Does every new AST node have a Rust struct/enum definition?
3. Are all error/warning codes in a table with name, code, and trigger?
4. Does the spec note any parser constraints (token ambiguities, precedence)?
5. Does the spec list every file that codegen will produce?
6. Do all references to other specs name real sections?
7. Does the spec avoid contradicting itself within a single section?
8. Are field names used consistently throughout?
9. Does every optional config field state its default value explicitly?
10. Is there at least one example of expected behavior for every error code?
11. Are all function/method signatures shown (not just described in prose)?
12. Does each new feature state which existing codegen files need changes?
13. Does the spec avoid unresolvable vague phrases like "reasonable" or "appropriate"?
14. Does the spec define behavior when optional blocks are absent?
15. Does the spec define behavior when two features interact?
16. Does the spec define behavior when the LLM/API is unavailable?
```

Parse response: 16 lines, each starting with `YES` or `NO`. Any other format → mark FAIL.

**Output format:**

```
claw spec-check specs/36-Synthesis-Repair-Loop.md

  grammar_examples     YES
  ast_defined          YES
  error_codes          YES
  ...
  codegen_outputs       NO   spec does not list which src/ files change
  ...

Score: 14/16 (88%)
```

---

### B4: `spec-tune` implementation

```rust
async fn cmd_spec_tune(
    file: &Path,
    iterations: u32,   // default 10
    dry_run: bool,
    client_name: Option<&str>,
) -> anyhow::Result<()> {
    let spec_text = std::fs::read_to_string(file)?;
    let criteria  = load_criteria(None)?;   // built-in 16
    check_spec_check_api_key(&resolve_provider(client_name)?)?;

    let mut current = spec_text.clone();
    let mut best    = current.clone();
    let mut best_score = 0u32;

    for i in 1..=iterations {
        let result = call_spec_judge(&current, &criteria, client_name).await?;
        let score  = result.results.iter().filter(|r| r.passed).count() as u32;

        if score > best_score {
            best_score = score;
            best = current.clone();
        }

        println!("[iter {i:2}/{iterations}] score: {score}/16  best: {best_score}/16");

        if score == 16 {
            println!("  PERFECT — stopping early");
            break;
        }

        if dry_run { break; }

        let failures: Vec<_> = result.results.iter().filter(|r| !r.passed).collect();
        current = call_spec_mutator(&current, &failures, client_name).await?;
    }

    // Write to <file>.tuned.md — NEVER overwrite original
    let tuned_path = file.with_extension("").with_extension("tuned.md");
    std::fs::write(&tuned_path, &best)?;
    println!("  Written: {}", tuned_path.display());
    Ok(())
}
```

**Mutator prompt:**

```
SYSTEM:
You are improving a technical specification document. Your goal: ensure it passes all quality criteria.
Output ONLY the improved spec. Do not explain.
IMPORTANT: Only add missing information. Never remove or contradict existing content.

USER:
Current spec:
---
<current>
---

Failing criteria:
<list of NO criteria with explanations>

Improve the spec to address these gaps.
```

---

### B5: `spec-cross-check` implementation

```rust
async fn cmd_spec_cross_check(dir: &Path, client_name: Option<&str>) -> anyhow::Result<()> {
    let specs: Vec<(PathBuf, String)> = glob::glob(&format!("{}/*.md", dir.display()))?
        .filter_map(|p| p.ok())
        .filter_map(|p| std::fs::read_to_string(&p).ok().map(|s| (p, s)))
        .collect();

    if specs.is_empty() {
        anyhow::bail!("E-SC02: no .md files found in {}", dir.display());
    }

    check_spec_check_api_key(&resolve_provider(client_name)?)?;

    // Call LLM with all specs concatenated (truncated to 32k chars if needed)
    let combined = specs.iter()
        .map(|(p, s)| format!("=== {} ===\n{}", p.display(), s))
        .collect::<Vec<_>>()
        .join("\n\n");

    let cross_criteria = [
        ("interface_stable",  "Do all specs that reference SynthesisRequest agree on its field names?"),
        ("error_code_unique", "Is every error code (E-R01, W-T02, etc.) defined in exactly one spec?"),
        ("version_consistent","Do all specs reference the same model versions (e.g. claude-haiku-4-5)?"),
        ("no_orphan_refs",    "Does every spec cross-reference point to a real section in another spec?"),
    ];

    let result = call_cross_check_judge(&combined, &cross_criteria, client_name).await?;
    print_cross_check_results(&result);
    Ok(())
}
```

---

## Verification checklist

Run these after all tasks complete:

```bash
# All tests pass
INSTA_UPDATE=always ~/.cargo/bin/cargo test

# Binary builds clean
~/.cargo/bin/cargo build --bin claw

# End-to-end: retry config compiles and appears in artifact
cat > /tmp/test36.claw << 'EOF'
type SearchResult { url: string  snippet: string }
client MyClaude { provider = "anthropic" model = "claude-haiku-4-5-20251001" }
synthesizer DefaultSynth {
    client = MyClaude
    retry {
        max_attempts:         4
        strategy:             repair_then_rewrite
        compile_repair_limit: 2
        on_stuck:             rewrite
        budget_usd:           0.50
    }
}
tool WebSearch(query: string) -> SearchResult {
    using: fetch
    synthesizer: DefaultSynth
    test { input: { query: "rust" } expect: { url: !empty, snippet: !empty } }
}
EOF
~/.cargo/bin/claw build /tmp/test36.claw

# Verify artifact retry section
node -e "
const a = JSON.parse(require('fs').readFileSync('/tmp/generated/artifact.clawa.json','utf8'));
const r = a.synthesizers[0].retry;
console.assert(r.max_attempts === 4,          'max_attempts');
console.assert(r.strategy === 'repair_then_rewrite', 'strategy');
console.assert(r.compile_repair_limit === 2,  'compile_limit');
console.assert(r.budget_usd === 0.50,         'budget');
console.log('artifact retry: OK');
"

# Spec-check on a spec (requires ANTHROPIC_API_KEY)
~/.cargo/bin/claw spec-check specs/36-Synthesis-Repair-Loop.md --report
# Expected: 15-16/16

~/.cargo/bin/claw spec-check specs/37-Spec-Autoresearch.md --report
# Expected: 15-16/16
```

---

## Invariants — never violate

1. **`repair_then_rewrite` is always first in `alt()`** — never let the `repair` branch match it.
2. **`tsc` always before `vitest`.** Never run vitest on non-compiling code.
3. **`compile_repair_limit` is always clamped to `max_attempts - 1`.** Cannot consume all attempts.
4. **Rewrite uses the default synthesis template** — never the Spec 35 `tune_prompt` override.
5. **Repair history written ONLY on total failure** (E-R01). Successful repairs are silent.
6. **`generated/__repair__/` is cleared at build start, every build.**
7. **`spec-tune` writes `<file>.tuned.md` — NEVER overwrites the original spec.**
8. **Spec mutator only ADDS content** — never removes or contradicts existing spec text.
9. **API key checked before the loop** — never fail mid-loop on a missing key.
10. **All `Statement` match arms include `Statement::Reason { .. }`** in every match you write.
11. **All `RetryStrategy` match arms cover all three variants** in every match you write.
12. **`verify_map` always, never `try_map` returning `Err(ContextError)`** in parser code.

---

## Spec-check scorecard (self-verification)

After implementation, run `claw spec-check` on both specs and confirm:

| Spec | Expected score | Criterion to watch |
|---|---|---|
| `36-Synthesis-Repair-Loop.md` | 15–16/16 | `codegen_outputs`, `offline_behavior` (§13 amendments) |
| `37-Spec-Autoresearch.md` | 15–16/16 | `error_codes`, `empty_inputs`, `offline_behavior` (§10 amendments) |

If either scores below 14/16, run `claw spec-tune` and apply the suggested improvements before closing the task.
