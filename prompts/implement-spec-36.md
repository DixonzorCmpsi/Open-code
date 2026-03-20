# Implementation Prompt: Spec 36 — Synthesis Repair Loop

**Project:** Claw DSL compiler (`clawc`) in Rust — `/Users/dixon.zor/Documents/Open-code`
**Specs to implement:** `specs/36-Synthesis-Repair-Loop.md` (with GAN fixes from `specs/36-GAN-Audit.md`)
**Prerequisites:**
- Spec 32/33 synthesis pipeline implemented (synth-runner.js, artifact.clawa.json)
- All tests pass: run `INSTA_UPDATE=always ~/.cargo/bin/cargo test` before starting.

---

## What you are implementing

Build-time tiered repair loop that activates when `claw build` synthesis produces broken TypeScript:

1. **`retry {}` block** on `synthesizer {}` — new AST node parsed into the synthesizer config.
2. **Repair orchestrator** inside `src/bin/claw.rs` — wraps the existing synthesis call with a feedback loop.
3. **`SynthesisRequest.repair_context`** optional field — extends the Spec 33 interface so `synth-runner.js` can construct repair prompts instead of the standard synthesis prompt.
4. **Tiered escalation** — compile errors first (tsc), test failures second (vitest), stuck detection, `on_stuck: rewrite` fallback.
5. **Persistent repair log** at `~/.claw/repair-history/` on total failure.

The repair loop is **entirely inside `claw build`** — no new CLI command. Users see it only on `--verbose` or when all attempts are exhausted.

---

## Existing codebase orientation

Read these files FIRST:

- `src/ast.rs` — all AST node definitions (add `RetryConfig` to `SynthesizerDecl`)
- `src/parser.rs` — winnow 0.7 parser (use `verify_map` not `try_map`)
- `src/semantic/mod.rs` — validation entry point
- `src/codegen/artifact.rs` — generates `artifact.clawa.json` (add retry config to synthesizer section)
- `src/codegen/synth_runner.rs` — generates `synth-runner.js` (add repair prompt construction)
- `src/bin/claw.rs` — `run_compile_once`, synthesis pipeline wiring
- `specs/36-Synthesis-Repair-Loop.md` — full spec with all GAN fixes
- `specs/33-Synthesis-Model-Interface.md` — `SynthesisRequest` schema reference

---

## Implementation order

Work in this exact sequence. Run `cargo test` after each task group.

---

### Task 1: AST changes (`src/ast.rs`)

**1a. Add `RetryStrategy` enum:**

```rust
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum RetryStrategy {
    Repair,
    Rewrite,
    RepairThenRewrite,
}
```

**1b. Add `PriceOverride`:**

```rust
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PriceOverride {
    pub input_per_million:  f64,
    pub output_per_million: f64,
}
```

**1c. Add `RetryConfig`:**

```rust
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
```

**1d. Update `SynthesizerDecl`** — add:

```rust
pub retry: Option<RetryConfig>,
```

**After 1d:** Run `cargo test`. Add `retry: None` to all `SynthesizerDecl` fixtures. Update insta snapshot with `INSTA_UPDATE=always cargo test`.

---

### Task 2: Parser changes (`src/parser.rs`)

Read `synthesizer_decl` and `tune_block` (if Spec 35 is implemented) before writing.

**2a. Add `retry_block` parser:**

```rust
fn retry_block(input: &mut Input<'_>) -> PResult<RetryConfig> {
    // Parses:
    // retry {
    //     max_attempts:          <int>
    //     strategy:              repair | rewrite | repair_then_rewrite
    //     compile_repair_limit:  <int>
    //     on_stuck:              repair | rewrite | repair_then_rewrite
    //     budget_usd:            <float>
    //     price_per_million_tokens { input: <float>, output: <float> }
    // }
}

fn retry_strategy(input: &mut Input<'_>) -> PResult<RetryStrategy> {
    alt((
        lexeme("repair_then_rewrite").map(|_| RetryStrategy::RepairThenRewrite),
        lexeme("repair").map(|_| RetryStrategy::Repair),
        lexeme("rewrite").map(|_| RetryStrategy::Rewrite),
    )).parse_next(input)
}
```

Note: `repair_then_rewrite` must be tried before `repair` — otherwise the prefix `repair` matches first.

**2b. Extend `synthesizer_decl` parser** — add `retry` branch:

```rust
"retry" => { decl.retry = Some(retry_block.parse_next(input)?); }
```

**After Task 2:** Run `cargo test`. Update snapshot if needed.

---

### Task 3: Semantic validation (`src/semantic/mod.rs`)

**3a. Add new error/warning codes** to `src/errors.rs`:

```rust
// Errors (build-time)
E_R01_SynthesisExhausted { tool: String, attempts: u32, last_failure: String, span: Span },
E_R02_CompileLimitNoRewrite { tool: String, limit: u32, span: Span },

// Warnings (compile-time config validation)
W_R01_RetryIgnoredByTune { synthesizer: String, span: Span },
W_R02_InvalidRetryConfig { synthesizer: String, reason: String, span: Span },
W_R03_RepairBudgetLow { synthesizer: String, budget: f64, span: Span },
```

**3b. Add `validate_retry_configs`:**

```rust
fn validate_retry_configs(
    document: &Document,
    errors: &mut Vec<CompilerError>,
    warnings: &mut Vec<CompilerWarning>,
) {
    for synth in &document.synthesizers {
        let Some(retry) = &synth.retry else { continue };

        let max = retry.max_attempts.unwrap_or(1);

        // W-R02: compile_repair_limit >= max_attempts (no room for test repair or any repair)
        if let Some(crl) = retry.compile_repair_limit {
            if crl >= max {
                warnings.push(W_R02_InvalidRetryConfig {
                    synthesizer: synth.name.clone(),
                    reason: format!(
                        "compile_repair_limit={} >= max_attempts={}, clamped to {}",
                        crl, max, max - 1
                    ),
                    span: retry.span.clone(),
                });
            }
        }

        // W-R03: budget_usd < 0.10
        if let Some(budget) = retry.budget_usd {
            if budget < 0.10 {
                warnings.push(W_R03_RepairBudgetLow {
                    synthesizer: synth.name.clone(),
                    budget,
                    span: retry.span.clone(),
                });
            }
        }

        // W-R02: max_attempts = 1 with non-trivial strategy (nothing to retry)
        if max == 1 {
            if retry.strategy.is_some() || retry.compile_repair_limit.is_some() {
                warnings.push(W_R02_InvalidRetryConfig {
                    synthesizer: synth.name.clone(),
                    reason: "max_attempts=1 means no retries — strategy and compile_repair_limit are ignored".to_owned(),
                    span: retry.span.clone(),
                });
            }
        }
    }
}
```

**3c. Wire into main `validate` function.**

---

### Task 4: Artifact codegen update (`src/codegen/artifact.rs`)

The artifact must include retry config so `synth-runner.js` can read it:

**4a. Extend `emit_synthesizer`:**

```rust
fn emit_synthesizer(s: &SynthesizerDecl) -> Value {
    let mut obj = json!({
        "name":        s.name,
        "client":      s.client,
        "temperature": s.temperature.unwrap_or(0.1),
    });

    if let Some(retry) = &s.retry {
        obj["retry"] = json!({
            "max_attempts":         retry.max_attempts.unwrap_or(1),
            "strategy":             emit_strategy(&retry.strategy),
            "compile_repair_limit": resolve_compile_limit(retry),  // see §13.2
            "on_stuck":             emit_strategy(&retry.on_stuck),
            "budget_usd":           retry.budget_usd.unwrap_or(0.50),
        });
    }

    obj
}

fn emit_strategy(s: &Option<RetryStrategy>) -> &'static str {
    match s {
        None | Some(RetryStrategy::Repair)            => "repair",
        Some(RetryStrategy::Rewrite)                  => "rewrite",
        Some(RetryStrategy::RepairThenRewrite)        => "repair_then_rewrite",
    }
}

fn resolve_compile_limit(retry: &RetryConfig) -> u32 {
    let max = retry.max_attempts.unwrap_or(1);
    let limit = retry.compile_repair_limit.unwrap_or(max.saturating_sub(1));
    limit.min(max.saturating_sub(1))  // clamp per §13.2
}
```

---

### Task 5: `SynthesisRequest` extension (`src/codegen/synth_runner.rs`)

The generated `synth-runner.js` must handle `repair_context` in the request.

**5a. Add repair prompt construction in `synth-runner.js` template:**

In the Rust template string that generates `synth-runner.js`, add a `buildPrompt` function that branches on the presence of `repair_context`:

```javascript
function buildPrompt(request) {
  if (!request.repair_context) {
    // Standard synthesis prompt (existing code)
    return buildSynthesisPrompt(request);
  }

  const ctx = request.repair_context;
  const typeDefsSection = (request.type_definitions || [])
    .map(t => t.definition).join('\n\n');

  if (ctx.strategy === 'rewrite') {
    return {
      system: SYNTHESIS_SYSTEM_PROMPT,
      user: [
        `## Synthesis Target`,
        `Tool: ${request.tool_name}(${formatArgs(request.arguments)}) -> ${request.return_type}`,
        ``,
        `## Type Definitions`,
        typeDefsSection,
        ``,
        `## Capability`,
        `using: ${request.using ?? 'none'}`,
        ``,
        `## Note`,
        `Previous synthesis attempts failed. This is a fresh attempt — generate a completely new implementation.`,
      ].join('\n'),
    };
  }

  // repair (compile or test)
  const tierLabel = ctx.tier === 'compile'
    ? 'Compilation Errors'
    : 'Test Failures';

  return {
    system: ctx.tier === 'compile'
      ? 'You are fixing TypeScript compilation errors. Output ONLY the corrected TypeScript file. Do not explain. Do not include markdown fences.'
      : 'You are fixing TypeScript code that fails unit tests. Output ONLY the corrected TypeScript file. Do not explain.',
    user: [
      `## Synthesis Target`,
      `Tool: ${request.tool_name}(${formatArgs(request.arguments)}) -> ${request.return_type}`,
      `Attempt ${ctx.attempt} of ${request.retry?.max_attempts ?? 1}`,
      ``,
      `## Type Definitions`,
      typeDefsSection,
      ``,
      `## Capability`,
      `using: ${request.using ?? 'none'}`,
      ``,
      `## Broken Code`,
      ctx.broken_code,
      ``,
      `## ${tierLabel}`,
      ctx.errors,
      ``,
      ctx.tier === 'compile'
        ? 'Fix all compilation errors. Preserve the tool\'s implementation intent. Output only the corrected TypeScript.'
        : 'Fix the code to pass the failing tests. Preserve working behavior. Output only the corrected TypeScript.',
    ].join('\n'),
  };
}
```

**5b. Add error truncation function:**

```javascript
function truncateTscErrors(rawOutput) {
  const lines = rawOutput.split('\n');
  const result = [];

  // Always include line 1
  if (lines.length > 0) result.push(lines[0]);

  // Collect up to 15 lines with tsXXXX error codes
  const errorLines = lines.filter((l, i) => i > 0 && /error TS\d+:/.test(l));
  const unique = [...new Map(errorLines.map(l => {
    const code = l.match(/TS(\d+)/)?.[1] ?? l.substring(0, 60);
    return [code, l];
  })).values()].slice(0, 15);
  result.push(...unique);

  // Always include summary line
  const summary = lines.find(l => /Found \d+ error/.test(l));
  if (summary && !result.includes(summary)) result.push(summary);

  // Fill to cap=20 with file:line context for first 5 errors
  const contextLines = lines.filter(l => /^\s+at /.test(l) || /\.ts\(\d+,\d+\)/.test(l)).slice(0, 5);
  for (const l of contextLines) {
    if (result.length >= 20) break;
    if (!result.includes(l)) result.push(l);
  }

  return result.join('\n');
}
```

---

### Task 6: Repair orchestrator (`src/bin/claw.rs`)

This is the main logic. Add a `synthesize_with_repair` function that wraps the existing single-shot synthesis call.

**6a. Core data structures:**

```rust
#[derive(Debug)]
struct RepairContext {
    attempt:      u32,
    tier:         RepairTier,
    strategy:     RetryStrategy,
    broken_code:  String,
    errors:       String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum RepairTier { Cold, Compile, Test }

#[derive(Debug)]
struct AttemptResult {
    code:          String,
    tsc_ok:        bool,
    tsc_errors:    String,
    vitest_ok:     bool,
    vitest_output: String,
    spend_usd:     f64,
}

#[derive(Debug, Default)]
struct RepairState {
    compile_repairs_used: u32,
    prev_tsc_error_count: Option<u32>,
    prev_passing_tests:   Option<u32>,
    total_spend_usd:      f64,
    history:              Vec<(RepairTier, AttemptResult)>,
}
```

**6b. `synthesize_with_repair` function:**

```rust
fn synthesize_with_repair(
    tool: &ToolDecl,
    document: &Document,
    project_root: &Path,
    verbose: bool,
) -> Result<String, SynthesisError> {
    let synth = resolve_synthesizer(tool, document);
    let retry = synth.and_then(|s| s.retry.as_ref());

    let max_attempts = retry.and_then(|r| r.max_attempts).unwrap_or(1);
    let strategy = retry.and_then(|r| r.strategy.clone()).unwrap_or(RetryStrategy::Repair);
    let compile_limit = retry.map(|r| resolve_compile_limit(r)).unwrap_or(max_attempts - 1);
    let on_stuck = retry.and_then(|r| r.on_stuck.clone()).unwrap_or(RetryStrategy::Rewrite);
    let budget = retry.and_then(|r| r.budget_usd).unwrap_or(0.50);

    // Clear generated/__repair__/<ToolName>/ at start
    clear_repair_dir(project_root, &tool.name);

    let mut state = RepairState::default();
    let mut repair_ctx: Option<RepairContext> = None;   // None = cold synthesis

    for attempt in 1..=max_attempts {
        if state.total_spend_usd >= budget {
            return Err(SynthesisError::BudgetExceeded { tool: tool.name.clone(), spend: state.total_spend_usd });
        }

        let result = run_one_synthesis(tool, document, project_root, repair_ctx.as_ref())?;

        // Save attempt file
        save_repair_attempt(project_root, &tool.name, attempt, &result.code);

        // Write telemetry
        write_repair_telemetry(tool, &result, attempt, repair_ctx.as_ref());

        state.total_spend_usd += result.spend_usd;

        if verbose {
            print_attempt_status(&tool.name, attempt, max_attempts, &result);
        }

        // Check tsc
        if !result.tsc_ok {
            if attempt >= max_attempts {
                // All attempts exhausted
                write_repair_history_on_failure(project_root, &tool.name, &state);
                return Err(SynthesisError::Exhausted {
                    tool: tool.name.clone(),
                    attempts: max_attempts,
                    last_failure: truncate_tsc_errors(&result.tsc_errors),
                });
            }

            // Stuck detection (compile)
            let cur_error_count = count_tsc_errors(&result.tsc_errors);
            let stuck = state.prev_tsc_error_count
                .map(|prev| cur_error_count >= prev)
                .unwrap_or(false);
            state.prev_tsc_error_count = Some(cur_error_count);

            if stuck {
                repair_ctx = Some(build_rewrite_context(attempt));
            } else if state.compile_repairs_used >= compile_limit {
                // Compile limit reached
                if strategy_has_rewrite(&strategy) || strategy_has_rewrite(&on_stuck) {
                    repair_ctx = Some(build_rewrite_context(attempt));
                } else {
                    // E-R02 note will be attached to E-R01
                    write_repair_history_on_failure(project_root, &tool.name, &state);
                    return Err(SynthesisError::CompileLimitNoRewrite {
                        tool: tool.name.clone(),
                        limit: compile_limit,
                    });
                }
            } else {
                state.compile_repairs_used += 1;
                repair_ctx = Some(RepairContext {
                    attempt,
                    tier: RepairTier::Compile,
                    strategy: effective_strategy(&strategy, attempt, max_attempts),
                    broken_code: result.code.clone(),
                    errors: truncate_tsc_errors(&result.tsc_errors),
                });
            }
            state.history.push((RepairTier::Compile, result));
            continue;
        }

        // tsc passed — check vitest
        if !result.vitest_ok {
            if attempt >= max_attempts {
                write_repair_history_on_failure(project_root, &tool.name, &state);
                return Err(SynthesisError::Exhausted {
                    tool: tool.name.clone(),
                    attempts: max_attempts,
                    last_failure: extract_vitest_failure(&result.vitest_output),
                });
            }

            // Stuck detection (test)
            let cur_passing = count_passing_tests(&result.vitest_output);
            let stuck = state.prev_passing_tests
                .map(|prev| cur_passing <= prev)
                .unwrap_or(false);
            state.prev_passing_tests = Some(cur_passing);

            if stuck {
                repair_ctx = Some(build_rewrite_context(attempt));
            } else {
                repair_ctx = Some(RepairContext {
                    attempt,
                    tier: RepairTier::Test,
                    strategy: effective_strategy(&strategy, attempt, max_attempts),
                    broken_code: result.code.clone(),
                    errors: result.vitest_output.clone(),
                });
            }
            state.history.push((RepairTier::Test, result));
            continue;
        }

        // Both tsc and vitest passed
        if verbose {
            println!("[synth] {} ... done ({} attempt{}, ~${:.2})",
                tool.name, attempt,
                if attempt == 1 { "" } else { "s" },
                state.total_spend_usd);
        }
        return Ok(result.code);
    }

    unreachable!("loop should have returned or errored before exhausting")
}
```

**6c. `effective_strategy` — handles `repair_then_rewrite`:**

```rust
fn effective_strategy(strategy: &RetryStrategy, attempt: u32, max_attempts: u32) -> RetryStrategy {
    match strategy {
        RetryStrategy::RepairThenRewrite if attempt == max_attempts - 1 => RetryStrategy::Rewrite,
        RetryStrategy::RepairThenRewrite => RetryStrategy::Repair,
        other => other.clone(),
    }
}
```

**6d. Type closure for repair context:**

```rust
fn collect_repair_types(tool: &ToolDecl, document: &Document) -> Vec<&TypeDecl> {
    let mut seen = std::collections::HashSet::new();
    let mut queue = std::collections::VecDeque::new();
    let mut result = Vec::new();

    // Seed with return type and argument types
    if let Some(rt) = &tool.return_type {
        queue.push_back(type_name_str(rt));
    }
    for arg in &tool.arguments {
        queue.push_back(type_name_str(&arg.data_type));
    }

    while let Some(name) = queue.pop_front() {
        if seen.contains(&name) { continue; }
        seen.insert(name.clone());

        if let Some(decl) = document.types.iter().find(|t| t.name == name) {
            result.push(decl);
            // Enqueue field types
            for field in &decl.fields {
                queue.push_back(type_name_str(&field.data_type));
            }
        }

        if result.len() >= 30 { break; }  // cap at 30
    }

    result
}
```

**6e. `run_one_synthesis` — the actual LLM call:**

```rust
fn run_one_synthesis(
    tool: &ToolDecl,
    document: &Document,
    project_root: &Path,
    repair_ctx: Option<&RepairContext>,
) -> Result<AttemptResult, SynthesisError> {
    let synth = resolve_synthesizer(tool, document).ok_or(SynthesisError::NoSynthesizer)?;
    let type_defs = collect_repair_types(tool, document);

    // Build SynthesisRequest JSON
    let mut request = build_synthesis_request(tool, &type_defs, synth);

    // Inject repair_context if this is a repair attempt
    if let Some(ctx) = repair_ctx {
        request["repair_context"] = json!({
            "attempt":     ctx.attempt,
            "tier":        match ctx.tier { RepairTier::Compile => "compile", RepairTier::Test => "test", _ => "cold" },
            "strategy":    match ctx.strategy { RetryStrategy::Repair => "repair", RetryStrategy::Rewrite => "rewrite", _ => "rewrite" },
            "broken_code": ctx.broken_code,
            "errors":      ctx.errors,
        });

        // Inject retry config so synth-runner.js can access max_attempts
        if let Some(retry) = &synth.retry {
            request["retry"] = json!({
                "max_attempts": retry.max_attempts.unwrap_or(1),
            });
        }
    }

    // Call synth-runner.js via NDJSON bridge
    let ts_code = call_synth_runner(project_root, &request)?;

    // Run tsc
    let (tsc_ok, tsc_errors) = run_tsc_check(project_root, &ts_code)?;

    // Run vitest only if tsc passes
    let (vitest_ok, vitest_output) = if tsc_ok {
        run_vitest_tests(project_root, &tool.name)?
    } else {
        (false, String::new())
    };

    // Estimate spend
    let spend = estimate_spend(&request, &ts_code, synth);

    Ok(AttemptResult { code: ts_code, tsc_ok, tsc_errors, vitest_ok, vitest_output, spend_usd: spend })
}
```

**6f. Budget price table:**

```rust
fn price_per_million(model: &str) -> (f64, f64) {
    // (input, output) — conservative 2× table per §13.7
    if model.starts_with("claude-haiku")  { return (0.50,  1.25) }
    if model.starts_with("claude-sonnet") { return (3.00,  15.00) }
    if model.starts_with("claude-opus")   { return (15.00, 75.00) }
    if model.starts_with("gpt-4o-mini")   { return (0.30,  1.20) }
    if model.starts_with("gpt-4o")        { return (5.00,  15.00) }
    (5.00, 15.00)  // fallback
}

fn estimate_spend(request: &Value, output: &str, synth: &SynthesizerDecl) -> f64 {
    let input_chars  = request.to_string().len() as f64;
    let output_chars = output.len() as f64;
    // Rough: 4 chars ≈ 1 token
    let input_tokens  = input_chars  / 4.0;
    let output_tokens = output_chars / 4.0;

    let model = synth.client.as_str();
    let (input_price, output_price) = price_per_million(model);
    (input_tokens / 1_000_000.0) * input_price + (output_tokens / 1_000_000.0) * output_price
}
```

**6g. Persistent repair history on E-R01:**

```rust
fn write_repair_history_on_failure(project_root: &Path, tool_name: &str, state: &RepairState) {
    let timestamp = chrono::Utc::now().format("%Y-%m-%dT%H-%M-%S").to_string();
    let history_dir = home_dir()
        .unwrap_or_default()
        .join(format!(".claw/repair-history/{}/{}", tool_name, timestamp));

    if std::fs::create_dir_all(&history_dir).is_err() { return; }

    for (i, (tier, result)) in state.history.iter().enumerate() {
        let n = i + 1;
        let _ = std::fs::write(history_dir.join(format!("attempt-{}.ts", n)), &result.code);
        match tier {
            RepairTier::Compile | RepairTier::Cold => {
                let _ = std::fs::write(history_dir.join(format!("attempt-{}-tsc.txt", n)), &result.tsc_errors);
            }
            RepairTier::Test => {
                let _ = std::fs::write(history_dir.join(format!("attempt-{}-vitest.txt", n)), &result.vitest_output);
            }
        }
    }

    let summary = json!({
        "tool":         tool_name,
        "timestamp":    timestamp,
        "total_spend":  state.total_spend_usd,
        "attempts":     state.history.len(),
    });
    let _ = std::fs::write(
        history_dir.join("repair-summary.json"),
        serde_json::to_string_pretty(&summary).unwrap_or_default(),
    );
}
```

**6h. `.gitignore` entry for `generated/__repair__/`:**

In the `claw build` setup code (wherever `generated/` directory is created), add:

```rust
fn ensure_repair_gitignore(project_root: &Path) {
    let gitignore = project_root.join(".gitignore");
    let entry = "generated/__repair__/\n";
    let contents = std::fs::read_to_string(&gitignore).unwrap_or_default();
    if !contents.contains("generated/__repair__/") {
        let _ = std::fs::OpenOptions::new()
            .append(true).create(true)
            .open(&gitignore)
            .and_then(|mut f| std::io::Write::write_all(&mut f, entry.as_bytes()));
    }
}
```

**6i. Clear repair dir at build start:**

```rust
fn clear_repair_dir(project_root: &Path, tool_name: &str) {
    let dir = project_root.join(format!("generated/__repair__/{}", tool_name));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::create_dir_all(&dir);
}
```

**6j. Wire `synthesize_with_repair` into `run_compile_once`:**

Find where the existing single-shot synthesis call happens (for tools with `using:`) and replace it with `synthesize_with_repair`. Handle `SynthesisError` variants and emit the correct compiler errors:

```rust
match synthesize_with_repair(tool, &document, project_root, verbose) {
    Ok(ts_code) => {
        write_tool_ts(project_root, &tool.name, &ts_code)?;
    }
    Err(SynthesisError::Exhausted { tool, attempts, last_failure }) => {
        errors.push(CompilerError::E_R01_SynthesisExhausted {
            tool, attempts, last_failure,
            span: tool_decl.span.clone(),
        });
    }
    Err(SynthesisError::CompileLimitNoRewrite { tool, limit }) => {
        errors.push(CompilerError::E_R01_SynthesisExhausted { /* ... */ });
        // E-R02 is a note — printed alongside E-R01 in error formatting
        notes.push(CompilerNote::E_R02_CompileLimitNoRewrite { tool, limit });
    }
    Err(SynthesisError::BudgetExceeded { tool, spend }) => {
        warnings.push(CompilerWarning::RepairBudgetExceeded { tool, spend });
    }
    Err(SynthesisError::NoSynthesizer) => { /* tool has no synthesizer — skip */ }
}
```

---

### Task 7: `write_document` hash update (`src/codegen/mod.rs`)

Add retry config to the canonical hash so cache invalidation works:

```rust
fn write_synthesizer_decl(output: &mut String, s: &SynthesizerDecl) {
    // ... existing fields ...
    if let Some(retry) = &s.retry {
        write_tag(output, "retry_max",      &retry.max_attempts.unwrap_or(1).to_string());
        write_tag(output, "retry_strategy", &format!("{:?}", retry.strategy));
        write_tag(output, "retry_budget",   &retry.budget_usd.unwrap_or(0.50).to_string());
    }
}
```

---

### Task 8: Test coverage

**8a. Parser tests:**

```rust
#[test]
fn parses_retry_block() {
    let source = r#"
client MyClaude { provider = "anthropic" model = "claude-haiku-4-5-20251001" }
synthesizer DefaultSynth {
    client = MyClaude
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
    let retry = doc.synthesizers[0].retry.as_ref().expect("retry config");
    assert_eq!(retry.max_attempts, Some(4));
    assert_eq!(retry.strategy, Some(RetryStrategy::RepairThenRewrite));
    assert_eq!(retry.compile_repair_limit, Some(2));
    assert_eq!(retry.on_stuck, Some(RetryStrategy::Rewrite));
    assert!((retry.budget_usd.unwrap() - 0.75).abs() < 0.001);
}

#[test]
fn parses_repair_strategy_variants() {
    for (input, expected) in [
        ("repair",               RetryStrategy::Repair),
        ("rewrite",              RetryStrategy::Rewrite),
        ("repair_then_rewrite",  RetryStrategy::RepairThenRewrite),
    ] {
        let source = format!(r#"
client C {{ provider = "anthropic" model = "x" }}
synthesizer S {{ client = C retry {{ max_attempts: 2 strategy: {} }} }}
"#, input);
        let doc = parse(&source).expect("parse");
        assert_eq!(doc.synthesizers[0].retry.as_ref().unwrap().strategy, Some(expected));
    }
}

#[test]
fn parses_minimal_retry() {
    let source = r#"
client C { provider = "anthropic" model = "x" }
synthesizer S { client = C retry { max_attempts: 3 } }
"#;
    let doc = parse(source).expect("parse");
    let retry = doc.synthesizers[0].retry.as_ref().unwrap();
    assert_eq!(retry.max_attempts, Some(3));
    assert!(retry.strategy.is_none());
    assert!(retry.compile_repair_limit.is_none());
}
```

**8b. Semantic tests:**

```rust
#[test]
fn warns_compile_limit_exceeds_max() {
    // compile_repair_limit >= max_attempts → W-R02
}

#[test]
fn warns_budget_too_low() {
    // budget_usd = 0.05 → W-R03
}

#[test]
fn warns_max_attempts_one_with_strategy() {
    // max_attempts: 1, strategy: repair → W-R02
}
```

**8c. Logic unit tests (`src/bin/claw.rs` test module):**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_compile_limit_defaults_to_max_minus_one() {
        let retry = RetryConfig { max_attempts: Some(4), compile_repair_limit: None, .. };
        assert_eq!(resolve_compile_limit(&retry), 3);
    }

    #[test]
    fn resolve_compile_limit_clamps_to_max_minus_one() {
        let retry = RetryConfig { max_attempts: Some(4), compile_repair_limit: Some(10), .. };
        assert_eq!(resolve_compile_limit(&retry), 3);
    }

    #[test]
    fn effective_strategy_repair_then_rewrite_last_attempt() {
        // With max=4, attempt 3 should return Rewrite (the last repair slot before final)
        assert_eq!(
            effective_strategy(&RetryStrategy::RepairThenRewrite, 3, 4),
            RetryStrategy::Rewrite
        );
        assert_eq!(
            effective_strategy(&RetryStrategy::RepairThenRewrite, 2, 4),
            RetryStrategy::Repair
        );
    }

    #[test]
    fn stuck_detection_compile_no_progress() {
        // error count stays same → stuck
        assert!(is_compile_stuck(5, Some(5)));
        assert!(is_compile_stuck(6, Some(5)));  // more errors = stuck
        assert!(!is_compile_stuck(4, Some(5))); // fewer errors = progress
        assert!(!is_compile_stuck(4, None));    // no previous = not stuck
    }

    #[test]
    fn stuck_detection_test_no_progress() {
        assert!(is_test_stuck(3, Some(3)));     // same passes = stuck
        assert!(is_test_stuck(2, Some(3)));     // fewer passes = stuck
        assert!(!is_test_stuck(4, Some(3)));    // more passes = progress
    }

    #[test]
    fn price_table_covers_known_models() {
        let (i, o) = price_per_million("claude-haiku-4-5-20251001");
        assert!(i > 0.0 && o > 0.0);
        let (i2, o2) = price_per_million("unknown-model-xyz");
        assert_eq!((i2, o2), (5.0, 15.0));  // fallback
    }
}
```

---

### Task 9: Final verification

```bash
# All tests pass
INSTA_UPDATE=always ~/.cargo/bin/cargo test

# Binary builds
~/.cargo/bin/cargo build --bin claw

# End-to-end: compile with retry config
cat > /tmp/test36.claw << 'EOF'
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
    retry {
        max_attempts:         4
        strategy:             repair_then_rewrite
        compile_repair_limit: 2
        on_stuck:             rewrite
        budget_usd:           0.50
    }
}

tool WebSearch(query: string) -> SearchResult {
    using:       fetch
    synthesizer: DefaultSynth
    test {
        input:  { query: "rust language" }
        expect: { url: !empty, snippet: !empty }
    }
}
EOF

cd /tmp && ~/.cargo/bin/claw build test36.claw

# Verify artifact has retry config
node -e "
  const a = JSON.parse(require('fs').readFileSync('/tmp/generated/artifact.clawa.json', 'utf8'));
  const s = a.synthesizers[0];
  console.log('retry.max_attempts:', s.retry?.max_attempts);
  console.log('retry.strategy:',     s.retry?.strategy);
  console.log('retry.compile_limit:', s.retry?.compile_repair_limit);
"
# Expected:
# retry.max_attempts: 4
# retry.strategy: repair_then_rewrite
# retry.compile_limit: 2
```

---

## Invariants — never violate these

1. **All tests pass after every task group.** Run `INSTA_UPDATE=always ~/.cargo/bin/cargo test` after each task.
2. **tsc ALWAYS runs before vitest.** Never run vitest on non-compiling code. If tsc fails, vitest result is `(false, "")`.
3. **`repair_then_rewrite` rewrite is the LAST attempt, not `max_attempts - 1`.** With `max_attempts: 3`, attempts are: cold synthesis, repair, rewrite. The rewrite is attempt 3.
4. **`compile_repair_limit` is always clamped to `max_attempts - 1`.** Never let compile repair consume all attempts with no room for test repair or rewrite. `min(user_value, max_attempts - 1)`.
5. **Rewrite uses the DEFAULT synthesis template, never the Spec 35 `tune_prompt` override.** The purpose of rewrite is to break anchoring.
6. **Error truncation algorithm follows §13.12 exactly** — always preserve line 1, up to 15 unique `tsXXXX` codes, and the summary line. Never just truncate at a char limit.
7. **Budget is checked BEFORE each attempt, not after.** If spending the next attempt would exceed the budget, skip it — do not overspend then check.
8. **Repair history is only written to `~/.claw/repair-history/` when E-R01 fires.** Successful repairs do not pollute history.
9. **`generated/__repair__/` is cleared at build start, every build.** Never accumulate stale files across builds.
10. **`claw tune` (Spec 35) ignores `retry {}` completely.** Tune always uses `max_attempts = 1` for accurate pass rate scoring.
11. **`verify_map` is always used in parser validation.** Never `try_map` returning `Err(ContextError)`.
12. **All `Statement` match arms include `Statement::Reason { .. }`.** All `AgentTools` match arms cover both variants. Add these to every new match you write.
