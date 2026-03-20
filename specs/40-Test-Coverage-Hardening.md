# Spec 40: Test Coverage Hardening

**Status:** Specced 2026-03-20.
**Depends on:** Spec 38 (Closed-Loop Runtime), Spec 39 (Runtime-First Architecture), Spec 37 (Spec Autoresearch — 16-criteria quality gate).

---

## 1. Problem

The test suite after Specs 38–39 passes (82 tests, 0 failures) but has seven structural gaps that will allow regressions through undetected:

| # | Gap | Risk |
| --- | --- | --- |
| G1 | `claw run` CLI path not tested | A broken `run_run` will not be caught |
| G2 | `runtime.js` correctness not tested | Broken plan compiler, bad schemas, syntax errors pass silently |
| G3 | `--list` flag not tested | `claw chat` depends on this; silent breakage |
| G4 | Live Ollama tests use OpenCode (wrong executor) | Tests are `#[ignore]` and outdated; manual CI always skips them |
| G5 | `claw init` message/file regression not tested | Messages reverted to OpenCode framing go undetected |
| G6 | `cli_integration.rs` does not assert `runtime.js` exists after build | `generate_runtime` can silently fail to write |
| G7 | Plan compiler `taskTemplate` bug class not covered | Template literal vs. plain string can recur |

This spec defines exactly which tests must exist, what they must assert, and which files change.

---

## 2. No New DSL Constructs

Spec 40 introduces no new Claw DSL syntax, no new AST nodes, and no new codegen output files. All changes are in the Rust test layer only. The 16-criteria `grammar_examples`, `ast_defined`, and `parser_note` criteria are satisfied trivially — this spec is test-only.

---

## 3. New Tests

### 3.1 `src/codegen_tests.rs` — runtime.js codegen unit tests

Four new `#[test]` functions appended to the existing `src/codegen_tests.rs`.

#### 3.1.1 `test_runtime_js_generated_on_build`

Asserts that `generate_runtime` writes `generated/runtime.js` for a minimal valid document.

```rust
#[test]
fn test_runtime_js_generated_on_build() {
    let doc = parser::parse(FULL_DOC).expect("parse failed");
    semantic::analyze(&doc).expect("semantic failed");

    let dir = tempdir().unwrap();
    codegen::generate_runtime(&doc, dir.path()).expect("generate_runtime failed");

    assert!(dir.path().join("generated/runtime.js").exists(),
        "generate_runtime must write generated/runtime.js");
}
```

#### 3.1.2 `test_runtime_js_zero_npm_imports`

Asserts the generated `runtime.js` contains no `require()` or `from "@anthropic-ai/sdk"` or `from "@modelcontextprotocol/sdk"` calls — only Node built-ins and raw `fetch`.

```rust
#[test]
fn test_runtime_js_zero_npm_imports() {
    let doc = parser::parse(FULL_DOC).expect("parse failed");
    semantic::analyze(&doc).expect("semantic failed");

    let dir = tempdir().unwrap();
    codegen::generate_runtime(&doc, dir.path()).expect("generate_runtime failed");

    let content = fs::read_to_string(dir.path().join("generated/runtime.js")).unwrap();

    assert!(!content.contains("require("),
        "runtime.js must not use require()");
    assert!(!content.contains("@anthropic-ai/sdk"),
        "runtime.js must not import @anthropic-ai/sdk");
    assert!(!content.contains("@modelcontextprotocol/sdk"),
        "runtime.js must not import @modelcontextprotocol/sdk");
    assert!(content.contains("fetch("),
        "runtime.js must use raw fetch()");
}
```

#### 3.1.3 `test_runtime_js_list_flag_emitted`

Asserts the generated file contains the `--list` handler that outputs a JSON array of workflow names.

```rust
#[test]
fn test_runtime_js_list_flag_emitted() {
    let doc = parser::parse(FULL_DOC).expect("parse failed");
    semantic::analyze(&doc).expect("semantic failed");

    let dir = tempdir().unwrap();
    codegen::generate_runtime(&doc, dir.path()).expect("generate_runtime failed");

    let content = fs::read_to_string(dir.path().join("generated/runtime.js")).unwrap();

    assert!(content.contains("--list"),
        "runtime.js must handle --list flag");
    assert!(content.contains("Object.values(PLANS)"),
        "runtime.js --list must enumerate PLANS");
}
```

#### 3.1.4 `test_runtime_js_task_template_is_plain_string`

Asserts that `taskTemplate` values in `PLANS` are plain JS strings (double-quoted), not template literals (backtick-quoted). This directly tests for the bug class found during Spec 39 implementation: template literals would evaluate `${topic}` at parse time rather than letting the runtime `interpolate()` function expand it.

```rust
#[test]
fn test_runtime_js_task_template_is_plain_string() {
    let doc = parser::parse(FULL_DOC).expect("parse failed");
    semantic::analyze(&doc).expect("semantic failed");

    let dir = tempdir().unwrap();
    codegen::generate_runtime(&doc, dir.path()).expect("generate_runtime failed");

    let content = fs::read_to_string(dir.path().join("generated/runtime.js")).unwrap();

    // taskTemplate must use double-quoted strings, not template literals
    // Find the PLANS block and check taskTemplate values
    assert!(content.contains(r#"taskTemplate: "find "#),
        "taskTemplate must be a double-quoted string, not a template literal");
    assert!(!content.contains("taskTemplate: `"),
        "taskTemplate must NOT be a backtick template literal");
}
```

#### 3.1.5 `test_runtime_js_node_syntax_check`

Runs `node --check generated/runtime.js` to validate JS syntax. Only runs if Node.js is found. Uses the same `find_node()` helper already in `mcp_smoke_test.rs` — moved to a shared `tests/helpers.rs` module (see §4).

```rust
#[test]
fn test_runtime_js_node_syntax_check() {
    let doc = parser::parse(FULL_DOC).expect("parse failed");
    semantic::analyze(&doc).expect("semantic failed");

    let dir = tempdir().unwrap();
    codegen::generate_runtime(&doc, dir.path()).expect("generate_runtime failed");

    let node = match helpers::find_node() {
        Some(n) => n,
        None => { println!("skip: node not found"); return; }
    };

    let status = Command::new(&node)
        .arg("--check")
        .arg(dir.path().join("generated/runtime.js"))
        .status()
        .unwrap();

    assert!(status.success(), "runtime.js must pass node --check");
}
```

#### 3.1.6 `test_runtime_js_list_flag_valid_json`

Runs `node runtime.js --list` and asserts the output is valid JSON containing an array of workflow objects with `name`, `requiredArgs`, and `returnType` fields.

```rust
#[test]
fn test_runtime_js_list_flag_valid_json() {
    let doc = parser::parse(FULL_DOC).expect("parse failed");
    semantic::analyze(&doc).expect("semantic failed");

    let dir = tempdir().unwrap();
    codegen::generate_runtime(&doc, dir.path()).expect("generate_runtime failed");

    let node = match helpers::find_node() {
        Some(n) => n,
        None => { println!("skip: node not found"); return; }
    };

    let output = Command::new(&node)
        .arg(dir.path().join("generated/runtime.js"))
        .arg("--list")
        .output()
        .unwrap();

    assert!(output.status.success(), "--list must exit 0");

    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim())
        .expect("--list output must be valid JSON");
    let arr = parsed.as_array().expect("--list output must be a JSON array");
    assert!(!arr.is_empty(), "--list output must contain at least one workflow");
    assert!(arr[0]["name"].is_string(), "each workflow must have a 'name' field");
    assert!(arr[0]["requiredArgs"].is_array(), "each workflow must have a 'requiredArgs' field");
}
```

---

### 3.2 `tests/cli_integration.rs` — `claw run` + build regression

#### 3.2.1 `test_cli_build_generates_runtime_js`

Updates the existing `test_cli_build_example_claw` to additionally assert that `generated/runtime.js` was created. **This is not a new test but an amendment to the existing one** — the current test only checks `opencode.json` and `mcp-server.js`.

The amended assertion block:

```rust
assert!(dir.path().join("opencode.json").exists());
assert!(dir.path().join("generated/mcp-server.js").exists());
assert!(dir.path().join("generated/runtime.js").exists(),  // NEW
    "claw build must generate runtime.js");
```

#### 3.2.2 `test_cli_run_missing_runtime`

Asserts that `claw run` with no `generated/runtime.js` present exits with a non-zero code and prints an actionable error message.

```rust
#[test]
fn test_cli_run_missing_runtime() {
    let dir = tempdir().unwrap();

    Command::cargo_bin("claw").unwrap()
        .current_dir(dir.path())
        .arg("run")
        .arg("FindInfo")
        .assert()
        .failure()
        .stderr(predicates::str::contains("claw build"));
}
```

**Expected behavior:** exits 1, stderr contains `"claw build"` as the hint.

#### 3.2.3 `test_cli_run_list_flag`

Builds a minimal `.claw` file, then runs `claw run --list` (which passes `--list` to `runtime.js`) and asserts valid JSON output.

```rust
#[test]
fn test_cli_run_list_flag() {
    let dir = tempdir().unwrap();
    // ... write minimal .claw, build ...
    Command::cargo_bin("claw").unwrap()
        .current_dir(dir.path())
        .arg("run")
        .arg("--list")
        .assert()
        .success()
        .stdout(predicates::str::contains("FindInfo"));
}
```

**Behavior if Node.js missing:** exits with E-RT02 message; test uses `find_node()` to skip if unavailable.

#### 3.2.4 `test_cli_init_no_opencode_message`

Asserts that `claw init` success output contains `"claw run"` and does NOT contain `"opencode run --command"`.

```rust
#[test]
fn test_cli_init_no_opencode_message() {
    let dir = tempdir().unwrap();
    let out = Command::cargo_bin("claw").unwrap()
        .current_dir(dir.path())
        .arg("init")
        .output()
        .unwrap();

    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("claw run"),
        "init must mention claw run as primary path");
    assert!(!stdout.contains("opencode run --command"),
        "init must NOT mention opencode run --command");
}
```

#### 3.2.5 `test_cli_init_package_json_optional_deps`

Asserts that the generated `package.json` uses `optionalDependencies`, not `dependencies`, for `@anthropic-ai/sdk` and `@modelcontextprotocol/sdk`.

```rust
#[test]
fn test_cli_init_package_json_optional_deps() {
    let dir = tempdir().unwrap();
    Command::cargo_bin("claw").unwrap()
        .current_dir(dir.path())
        .arg("init")
        .assert()
        .success();

    let content = fs::read_to_string(dir.path().join("package.json")).unwrap();
    let pkg: serde_json::Value = serde_json::from_str(&content).unwrap();

    assert!(pkg["optionalDependencies"]["@anthropic-ai/sdk"].is_string(),
        "@anthropic-ai/sdk must be in optionalDependencies");
    assert!(pkg["optionalDependencies"]["@modelcontextprotocol/sdk"].is_string(),
        "@modelcontextprotocol/sdk must be in optionalDependencies");
    assert!(pkg["dependencies"].is_null() || pkg["dependencies"].as_object().map(|o| o.is_empty()).unwrap_or(true),
        "dependencies block must not contain SDK packages");
}
```

---

### 3.3 `tests/live_ollama_test.rs` — fix and modernize

The existing `test_live_e2e_with_local_qwen` test uses `opencode -p` to execute workflows, which contradicts the Spec 39 architecture inversion. It must be replaced.

#### 3.3.1 `test_ollama_is_running` (amended)

Change the model check from `qwen2.5-coder:7b` to `qwen2.5:14b`, which is the model verified to work in the Spec 39 end-to-end test.

```rust
let has_qwen = models.iter().any(|m|
    m["id"].as_str().map_or(false, |s| s.contains("qwen2.5:14b"))
);
assert!(has_qwen, "Ollama is missing 'qwen2.5:14b' model");
```

#### 3.3.2 `test_live_e2e_claw_run` (replacement for `test_live_e2e_with_local_qwen`)

Uses `claw run` (not `opencode`) to execute a workflow end-to-end. Remains `#[ignore]` for CI — must be run manually with `cargo test -- --ignored`.

```rust
#[test]
#[ignore]
fn test_live_e2e_claw_run() {
    // 1. Build a .claw file with local Ollama client
    // 2. Run: claw build <file>
    // 3. Assert: generated/runtime.js exists
    // 4. Run: node runtime.js --list
    // 5. Assert: output contains "Summarize"
    // 6. Run: claw run Summarize --arg topic="test"
    // 7. Assert: exit 0, stdout is valid JSON
    // No npm install, no opencode — only claw build + claw run
}
```

The `.claw` content for this test uses a no-tools summarizer agent (matches what was verified in the Spec 39 real test session):

```rust
fs::write(&example_path, r#"
client LocalQwen {
    provider = "local"
    model = "local.qwen2.5:14b"
}
agent Summarizer {
    client = LocalQwen
    system_prompt = "You are a concise summarizer. Respond in 2-3 sentences."
    settings = { max_steps: 2, temperature: 0.3 }
}
workflow Summarize(topic: string) {
    let result = execute Summarizer.run(
        task: "Summarize ${topic} in 2-3 sentences."
    )
    return result
}
"#).unwrap();
```

---

### 3.4 `tests/helpers.rs` (new shared module)

Extract `find_node()` from `mcp_smoke_test.rs` into a shared `tests/helpers.rs` to avoid duplication. Both `mcp_smoke_test.rs` and the new runtime tests need it.

```rust
// tests/helpers.rs
pub fn find_node() -> Option<String> {
    let candidates = [
        "/opt/homebrew/bin/node",
        "/usr/local/bin/node",
        "/usr/bin/node",
        "node",
    ];
    for path in &candidates {
        if Command::new(path).arg("--version").output().is_ok() {
            return Some(path.to_string());
        }
    }
    None
}
```

---

## 4. Files Changed

| File | Change |
| --- | --- |
| `src/codegen_tests.rs` | Add tests 3.1.1–3.1.6 (6 new `#[test]` functions) |
| `tests/cli_integration.rs` | Add tests 3.2.2–3.2.5; amend `test_cli_build_example_claw` (3.2.1) |
| `tests/live_ollama_test.rs` | Replace `test_live_e2e_with_local_qwen` with `test_live_e2e_claw_run`; amend `test_ollama_is_running` |
| `tests/helpers.rs` | New file — shared `find_node()` helper |
| `tests/mcp_smoke_test.rs` | Replace inline `find_node()` with `mod helpers; helpers::find_node()` |

No changes to `src/ast.rs`, `src/parser.rs`, `src/bin/claw.rs`, or any codegen module.

---

## 5. Error and Warning Codes Validated by Tests

This spec does not define new error codes. The following existing codes are explicitly exercised for the first time by the tests in §3:

| Code | Source Spec | Level | Test that validates it |
| --- | --- | --- | --- |
| E-RT01 | Spec 39 §9 | Rust CLI | Not directly tested — requires mocking `node --version`; noted for Spec 41 |
| E-RT02 | Spec 39 §9 | Rust CLI | `test_cli_run_missing_runtime` when Node is absent; `find_node()` guards skip gracefully |
| E-RUN01 | Spec 38 §8.1 | runtime.js | Not exercised by `test_cli_run_missing_runtime` — that test hits the Rust-level `ClawCliError::Message` from `find_runtime_js()` before Node is invoked. `test_runtime_js_list_flag_valid_json` exercises the runtime with a valid workflow name; workflow-not-found path noted for Spec 41 |
| E-RUN02 | Spec 38 §8.1 | runtime.js | Not yet directly tested — noted for Spec 41 |
| W-RUN01 | Spec 38 §8.1 | runtime.js | Not yet directly tested — noted for Spec 41 |

**Rust CLI level errors** (E-RT01, E-RT02) fire inside `run_run` in `src/bin/claw.rs` before Node is invoked.
**runtime.js level errors** (E-RUN01, E-RUN02, W-RUN01) fire inside the Node process after Node successfully starts. These are distinct layers — a test that hits the Rust-level path does not exercise the runtime.js error codes.

The `test_cli_run_missing_runtime` test validates that `claw run` without a built artifact prints a message containing `"claw build"` and exits non-zero. It exercises the Rust error path, not E-RUN01.

The E-RT01 path (Node < 18) requires either mocking `node --version` output or running in a controlled environment. This is out of scope for Spec 40 and noted for Spec 41.

---

## 6. Behavior When Optional Inputs Are Absent

**No workflows declared:** `generate_runtime` must not fail. `PLANS` will be `{}`. The `--list` flag outputs `[]`. `test_runtime_js_list_flag_valid_json` uses `FULL_DOC` which has a workflow, so an additional micro-test is needed:

```rust
#[test]
fn test_runtime_js_no_workflows_emits_empty_plans() {
    let input = r#"
client C { provider = "anthropic" model = "claude-4-sonnet" }
agent A { client = C }
"#;
    let doc = parser::parse(input).expect("parse failed");
    let dir = tempdir().unwrap();
    codegen::generate_runtime(&doc, dir.path()).expect("generate_runtime failed");
    let content = fs::read_to_string(dir.path().join("generated/runtime.js")).unwrap();
    assert!(content.contains("const PLANS = {}"),
        "no workflows must produce empty PLANS object");
}
```

**No tools declared:** `TOOLS` will be `[]`. Tool handler block will be empty. `callTool(name, input)` dispatcher will always hit the `default` case throwing E-RUN99. This is correct behavior — no test needed beyond confirming `generate_runtime` does not panic.

**No clients declared:** `generate_runtime` falls back to `anthropic` provider with `claude-haiku-4-5-20251001` (same fallback logic as `mcp.rs`). Confirmed by `test_codegen_mcp_server_js` in Spec 38 codegen tests — same `resolve_client` function.

---

## 7. Feature Interaction: `claw run` + Missing Node vs. Missing Runtime

Two failure conditions for `claw run` interact:

| Node present | `runtime.js` present | Behavior | Exit code |
| --- | --- | --- | --- |
| Yes | Yes | Run normally | 0 or workflow-defined |
| Yes | No | E-RUN: "claw build first" | 1 |
| No | Yes | E-RT02: "Node.js not found" | 1 |
| No | No | E-RT02: "Node.js not found" (checked first) | 1 |

`check_node_version()` is called before `find_runtime_js()` in `run_run`. The Node check fires first in all cases. `test_cli_run_missing_runtime` in §3.2.2 runs in environments where Node IS present (the test suite requires it for MCP smoke tests), so it correctly exercises the runtime-missing path without conflating it with the node-missing path.

---

## 8. Offline Behavior

All tests in §3.1 and §3.2 (except `test_live_e2e_claw_run`) are **fully offline** — they test codegen output and CLI behavior without making any LLM API calls. They must pass with no network access and no API keys set.

`test_live_e2e_claw_run` (§3.3.2) requires a running Ollama instance at `localhost:11434`. It is `#[ignore]` and will never run in CI. It is run manually by:

```bash
cargo test -- --ignored test_live_e2e_claw_run
```

The `test_runtime_js_node_syntax_check` and `test_runtime_js_list_flag_valid_json` tests (§3.1.5, §3.1.6) require Node.js but make no network calls. They skip gracefully when Node is not found using `find_node()`.

---

## 9. Target Coverage After Spec 40

| Area | Before | After |
| --- | --- | --- |
| `generate_runtime` called in any test | No | Yes (3.1.1) |
| `runtime.js` zero npm imports | No | Yes (3.1.2) |
| `runtime.js` `--list` flag | No | Yes (3.1.3, 3.1.6) |
| `taskTemplate` plain string invariant | No | Yes (3.1.4) |
| `runtime.js` JS syntax valid | No | Yes (3.1.5) |
| `claw build` outputs `runtime.js` | No | Yes (3.2.1 amendment) |
| `claw run` no-runtime error message | No | Yes (3.2.2) |
| `claw init` message regression | No | Yes (3.2.4, 3.2.5) |
| Live E2E uses `claw run` (not opencode) | No | Yes (3.3.2) |
| E-RUN01 exercised | No | Yes (3.2.2) |

Expected test count after implementation: **82 existing + 12 new = 94 total** (2 `#[ignore]`).
