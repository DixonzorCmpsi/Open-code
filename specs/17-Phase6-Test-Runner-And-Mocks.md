# Phase 6C: Test Runner & Mock Execution

> **ARCHITECTURE UPDATE (Phase 2 OpenCode Migration):**
> The gateway-based test runner (`openclaw-gateway/src/test-runner.ts`) is **retired** along with the gateway.
> The execution model in §2 (traversal engine), §3.3 (mock registry in `executeAgentRun()`), and §4
> (gateway subprocess) are superseded. The new test execution model is defined in **§7** below.
> The DSL constructs (`test` blocks, `mock` blocks, `assert`) and compiler rules (§1, §3 AST) remain
> **fully active and unchanged**. Only the runtime execution backend changes.
> See also: `specs/25-OpenCode-Integration.md §9` for the overall testing strategy.

This spec covers the `claw test` command and the execution of `.claw` test and mock blocks. Developers write tests alongside workflows and run them without hitting real LLM APIs.

**Prerequisite:** Read `specs/01-DSL-Core-Specification.md` (test/mock syntax), `specs/03-Grammar.md`, `specs/04-AST-Structures.md`, `specs/14-CLI-Tooling.md`.

---

## 0. Goals & Non-Goals

### Goals (MUST do)
- Add `assert` as a first-class statement with optional message: `assert expr, "message"`
- `assert` is ONLY valid inside `test` blocks — compile error elsewhere (`InvalidAssertOutsideTest`)
- Replace `MockDecl` AST (BREAKING CHANGE) — old `mock Agent(input) -> output` becomes `mock Agent { key: value }`
- Implement mock registry with name-only matching (last mock for same agent wins)
- Mock interception is HIGHEST priority in `executeAgentRun()` — before tools, BAML, and raw HTTP
- Implement `claw test` CLI command with `--filter` substring matching
- Implement `test-runner.ts` as gateway subprocess reading JSON manifest from stdin, outputting JSON lines to stdout
- Enforce 30s per-test timeout (configurable via `CLAW_TEST_TIMEOUT_MS`)
- Tests pass when ALL assertions succeed and no unhandled exception propagates; tests fail on first false assertion or any thrown error

### Non-Goals (MUST NOT do)
- Do NOT implement input-pattern mock matching (`mock Agent(task: "pattern")`) — Phase 7
- Do NOT implement test coverage reporting — Phase 7
- Do NOT implement test suites/grouping (`suite "name" { ... }`) — Phase 8
- Do NOT implement `beforeEach`/`afterEach` setup/teardown hooks — Phase 8
- Do NOT implement parallel test execution — tests run sequentially in definition order
- Do NOT implement watch mode for tests (`claw test --watch`) — Phase 7
- Do NOT change the `test` block syntax — it remains `test "name" { ... }` with no arguments
- Do NOT allow `assert` in workflows or listeners — test blocks only
- Do NOT implement mock verification (asserting a mock was called N times) — Phase 8

---

## 1. The `assert` Statement (New Language Primitive)

### Design Decision

The original "return PASS/FAIL" model is replaced with a first-class `assert` statement. This aligns with BAML's testing model, pytest, Jest, and every modern test framework. Tests use assertions, not string returns.

### Grammar Addition (update `specs/03-Grammar.md`)

```peg
assert_stmt = { "assert" ~ expr ~ ("," ~ string_lit)? }
```

The optional string literal is a custom failure message.

### AST Addition (update `specs/04-AST-Structures.md`)

```rust
Statement::Assert {
    condition: Expr,
    message: Option<String>,
    span: Span,
}
```

### Semantic Rules

- `assert` is ONLY valid inside `test` blocks. Using `assert` in a workflow → `CompilerError::InvalidAssertOutsideTest`.
- The `condition` expression must evaluate to a boolean. Non-boolean → `CompilerError::TypeMismatch`.
- At runtime, if the condition is false, the test fails immediately with the assertion message (or a generated message including the span).

### Examples

```claw
test "Researcher returns valid SearchResult" {
    let result: SearchResult = execute Researcher.run(
        task: "Find announcements for Apple",
        require_type: SearchResult
    )

    assert result.url != "", "url must not be empty"
    assert result.confidence_score > 0.5
    assert result.tags.length() > 0, "must have at least one tag"
}
```

### TDD Tests (Compiler)

1. **`test_parse_assert_with_message`** — Parse `assert x > 0, "must be positive"` → `Statement::Assert { condition: BinaryOp(...), message: Some("must be positive") }`.
2. **`test_parse_assert_without_message`** — Parse `assert x > 0` → `Statement::Assert { message: None }`.
3. **`test_semantic_reject_assert_outside_test`** — Assert in a workflow → `CompilerError::InvalidAssertOutsideTest`.
4. **`test_semantic_accept_assert_in_test_block`** — Assert inside `test "..." { assert ... }` → `Ok(())`.

---

## 2. Test Block Execution Semantics

### How Tests Run

1. Each `test` block is compiled into the AST as a `TestDecl`.
2. During `claw test`, the gateway traversal engine executes each `TestDecl.body` like a workflow body.
3. **Pass condition:** All `assert` statements in the body evaluate to `true` AND no unhandled exception is thrown.
4. **Fail condition:** Any `assert` evaluates to `false` OR an exception propagates out of the test body.

### Runtime Assert Behavior (Gateway)

When the traversal engine encounters `Statement::Assert`:

```typescript
case "Assert": {
  const condition = await evaluateExpr(compiled, state, payload.condition, ...);
  if (!condition) {
    const message = payload.message ?? `Assertion failed at ${statementPath}`;
    throw new AssertionError(message, statementPath);
  }
  await checkpoints.checkpoint(state, statementPath, "assert_pass");
  frame.nextIndex += 1;
  return;
}
```

`AssertionError` is a new error type in `gateway/src/engine/errors.ts`:
```typescript
export class AssertionError extends Error {
  nodePath: string;
  constructor(message: string, nodePath: string) {
    super(message);
    this.name = "AssertionError";
    this.nodePath = nodePath;
  }
}
```

---

## 3. Mock Blocks

### Syntax

```claw
mock Researcher {
    url: "https://apple.com/news",
    confidence_score: 0.95,
    snippet: "Apple releases new XR headset.",
    tags: ["hardware", "xr"]
}
```

**Design decision:** Mocks match by **agent name only** for MVP. The mock body is a literal expression that replaces ALL calls to that agent during test execution.

**Rationale for name-only matching:** Input pattern matching (e.g., `mock Researcher(task: "Find Apple") -> ...`) requires runtime string comparison against dynamic expressions. This adds significant complexity for marginal value in the MVP. Tests that need different responses for different inputs should use separate agents:

```claw
mock AppleResearcher { url: "apple.com", ... }
mock MicrosoftResearcher { url: "microsoft.com", ... }
```

Input-pattern matching is deferred to Phase 7.

### BREAKING CHANGE: MockDecl AST Migration

The existing `MockDecl` in `src/ast.rs` has:
```rust
pub struct MockDecl {
    pub target_agent: String,
    pub mock_input: Expr,
    pub mock_output: Expr,
    pub span: Span,
}
```

This spec replaces it with:
```rust
pub struct MockDecl {
    pub target_agent: String,
    pub output: Vec<(String, Expr)>,
    pub span: Span,
}
```

**Migration required:**
- Update `src/ast.rs` — replace MockDecl fields
- Update `src/parser.rs` — replace `mock_decl` parser combinator to match new grammar
- Update `specs/04-AST-Structures.md` — reflect new MockDecl shape
- Update `specs/03-Grammar.md` — replace `mock_decl` grammar rule
- Update `generated/claw/document.json` schema — the serialized AST format changes
- Update gateway `types.ts` — reflect new MockDecl shape in TypeScript types

The old `mock_input` and `mock_output` fields are REMOVED. The new `output` field is a key-value object literal.

**Complete blast radius (ALL files that reference MockDecl):**
1. `src/ast.rs` — struct definition
2. `src/parser.rs` — `mock_decl` combinator
3. `src/codegen/mod.rs` — snapshot tests referencing mock AST
4. `src/codegen/typescript.rs` — if mocks appear in generated SDK
5. `specs/04-AST-Structures.md` — spec already updated
6. `specs/03-Grammar.md` — grammar already updated
7. `generated/claw/document.json` — serialized AST format changes
8. `openclaw-gateway/src/types.ts` — TypeScript MockDecl interface
9. `openclaw-gateway/src/engine/traversal.test.ts` — test fixtures with mock blocks
10. `cargo insta` snapshot files — any snapshot containing MockDecl output

**All 10 files MUST be updated in a single coordinated commit.** Missing any one produces compile errors or test failures.

### Grammar

```peg
mock_decl = { "mock" ~ identifier ~ block_object }
block_object = { "{" ~ (identifier ~ ":" ~ expr ~ ","?)+ ~ "}" }
```

**Note:** This simplifies the original `mock Agent(input) -> output` syntax to `mock Agent { key: value, ... }`. The mock body is an object literal, not an input→output mapping.

### AST

```rust
pub struct MockDecl {
    pub target_agent: String,
    pub output: Vec<(String, Expr)>,  // Key-value pairs
    pub span: Span,
}
```

### Mock Registry (Gateway)

```typescript
interface MockRegistry {
  lookup(agentName: string): Record<string, unknown> | null;
}

function buildMockRegistry(mocks: MockDecl[]): MockRegistry {
  const map = new Map<string, Record<string, unknown>>();

  for (const mock of mocks) {
    // Last mock for the same agent wins
    const output: Record<string, unknown> = {};
    for (const [key, value] of mock.output) {
      output[key] = evaluateConstExpr(value);
    }
    map.set(mock.target_agent, output);
  }

  return {
    lookup(agentName: string) {
      return map.get(agentName) ?? null;
    }
  };
}
```

### Traversal Integration

In `executeAgentRun()`, check mock registry BEFORE any LLM/tool routing:

```typescript
async function executeAgentRun(
  compiled, state, executeRun, statementPath, workspaceRoot, checkpoints,
  mockRegistry?: MockRegistry  // NEW parameter
): Promise<unknown> {
  // Mock interception (test mode only)
  if (mockRegistry) {
    const mockResult = mockRegistry.lookup(executeRun.agent_name);
    if (mockResult !== null) {
      return validateToolResult(mockResult, returnSchema);
    }
  }

  // ... existing LLM/tool routing ...
}
```

Thread `mockRegistry` through `TraversalOptions`:

```typescript
interface TraversalOptions {
  compiled: CompiledDocumentFile;
  request: ExecutionRequest;
  checkpoints: CheckpointStore;
  workspaceRoot?: string;
  mockRegistry?: MockRegistry;  // Active during test execution only
}
```

### TDD Tests (Gateway)

1. **`test_mock_registry_intercepts_agent_execution`** — Mock "Researcher" returning `{ url: "mock://test" }`. Execute workflow. Assert mock result is returned, no LLM called.
2. **`test_mock_registry_passes_through_unmocked_agents`** — Mock "Researcher" only. Execute "Navigator" agent. Assert normal execution (mock doesn't interfere).
3. **`test_mock_registry_last_mock_wins`** — Define two mocks for "Researcher". Assert the second one's output is used.

---

## 4. `claw test` CLI Command

### Usage

```
claw test [source.claw] [--config claw.json] [--filter "pattern"]
```

### Rust CLI Implementation

Add `Test(TestArgs)` to the `Commands` enum in `claw.rs`:

```rust
#[derive(Debug, clap::Args)]
struct TestArgs {
    source: Option<PathBuf>,
    #[arg(long)]
    filter: Option<String>,
    #[arg(long, default_value = "claw.json")]
    config: PathBuf,
}
```

### Execution Flow

1. **Compile** the `.claw` source (reuse `compile_document()`)
2. **Extract** `TestDecl` and `MockDecl` from the compiled document
3. **Filter** tests by `--filter` pattern (case-insensitive substring match)
4. **Build** a test manifest containing:
   - The compiled `document.json`
   - List of test names to execute
   - Mock declarations
5. **Execute** tests via the gateway test runner
6. **Print** results
7. **Exit** with code 0 (all pass) or 1 (any fail)

### Test Runner (Gateway Side)

Create `openclaw-gateway/src/test-runner.ts`:

```typescript
// Entry point: node --experimental-strip-types openclaw-gateway/src/test-runner.ts
// Reads test manifest from stdin as JSON
// For each test:
//   1. Build MockRegistry from manifest.mocks
//   2. Create fresh in-memory checkpoint store
//   3. Execute test body through traversal engine
//   4. Catch AssertionError → FAIL
//   5. Catch other errors → FAIL (with error message)
//   6. No errors → PASS
// Output: JSON results to stdout (one line per test, then summary)
```

### Test Result JSON Schema

Each test emits one JSON line to stdout:

```json
{ "name": "Researcher returns valid results", "status": "pass", "duration_ms": 12 }
{ "name": "Handles empty results", "status": "fail", "duration_ms": 8, "error": "Assertion failed: url must not be empty", "node_path": "test:Handles empty results/body/statements/2" }
```

Final summary line:

```json
{ "summary": true, "passed": 2, "failed": 1, "total_ms": 35 }
```

### Human-Readable Output (Default)

The Rust CLI parses the JSON lines and formats them:

```
Running 3 tests from example.claw...

  PASS  Researcher returns valid results (12ms)
  FAIL  Handles empty results (8ms)
        Assertion failed: url must not be empty
        at test:Handles empty results/body/statements/2
  PASS  SeniorResearcher summarizes correctly (15ms)

Results: 2 passed, 1 failed (35ms total)
```

### TDD Tests (CLI)

1. **`test_parses_test_command`** — `Cli::parse_from(["claw", "test", "example.claw"])` → `Commands::Test(...)`.
2. **`test_parses_test_filter`** — `Cli::parse_from(["claw", "test", "--filter", "Researcher"])` → `filter: Some("Researcher")`.
3. **`test_no_tests_match_filter_exits_zero`** — Filter "nonexistent" → exit 0 with "No tests matched filter" message.

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | All tests passed (or no tests matched filter) |
| 1 | One or more tests failed |
| 2 | Compilation error |
| 4 | I/O error |

### Per-Test Timeout

Each test execution has a default timeout of **30 seconds**. If a test body does not complete within this time (e.g., infinite loop, hanging tool execution), the test runner:
1. Kills the traversal execution for that test
2. Reports the test as `FAIL` with message `"Test timed out after 30000ms"`
3. Proceeds to the next test

Configurable via `CLAW_TEST_TIMEOUT_MS` environment variable.

```typescript
const TEST_TIMEOUT_MS = Number(process.env.CLAW_TEST_TIMEOUT_MS ?? 30_000);

async function executeTestWithTimeout(testBody: Block, ...): Promise<TestResult> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), TEST_TIMEOUT_MS);
  try {
    await executeWorkflow({ ..., signal: controller.signal });
    return { status: "pass" };
  } catch (error) {
    if (controller.signal.aborted) {
      return { status: "fail", error: `Test timed out after ${TEST_TIMEOUT_MS}ms` };
    }
    return { status: "fail", error: error.message };
  } finally {
    clearTimeout(timer);
  }
}
```

---

## 5. `--filter` Flag

**Behavior:**
- Substring match (case-insensitive): `--filter "researcher"` matches `"Researcher returns valid results"`
- Multiple words match independently: `--filter "researcher valid"` is NOT supported (single pattern only)
- If no tests match → exit 0 with message "No tests matched filter '{pattern}'"

---

## 6. Future Extensions (Not in Phase 6)

These are documented for architectural awareness but deferred:

### 6.1 Input-Pattern Mock Matching (Phase 7)

```claw
mock Researcher(task: "Find Apple") {
    url: "apple.com", ...
}
mock Researcher(task: "Find Microsoft") {
    url: "microsoft.com", ...
}
mock Researcher(task: *) {
    url: "default.com", ...
}
```

Requires runtime string matching in the mock registry. Deferred because it adds complexity without critical MVP value.

### 6.2 Coverage Reporting (Phase 7)

```
Coverage: 3/4 workflows (75%), 2/3 agents (67%), 1/2 tools (50%)
```

The test runner tracks which entities were exercised. Requires the traversal engine to emit coverage events.

### 6.3 Test Grouping / Suites (Phase 8)

```claw
suite "Researcher Tests" {
    test "returns valid results" { ... }
    test "handles empty input" { ... }
}
```

Adds organizational structure. Deferred because flat test lists are sufficient for MVP.

---

## 7. OpenCode-Era Test Execution Model (ACTIVE — replaces §2–§4 implementation)

The gateway traversal engine is retired. `claw test` in the OpenCode architecture uses a **generated Node.js test runner** that is co-emitted by `clawc build --lang opencode`. This is the authoritative execution model.

### 7.1 Architecture

```
claw test example.claw
    │
    ├── clawc compiles example.claw → generated/claw-test-runner.js
    │
    └── node generated/claw-test-runner.js [--filter "pattern"]
            │
            ├── Builds MockRegistry from mock blocks (same name-only matching, §3)
            ├── Executes each test block in definition order
            │   ├── Mock interception: agent calls → MockRegistry.lookup()
            │   ├── assert statements → throw AssertionError on failure
            │   └── No LLM calls, no OpenCode required, no gateway required
            └── Outputs JSON result lines + summary to stdout
```

**Key constraint:** `claw test` is **fully offline** — it requires no LLM API keys, no OpenCode installation, and no internet connection. All agent calls are intercepted by the mock registry. A test that executes an agent without a mock block fails with: `"No mock defined for agent 'Researcher'. Add a mock block to run this test offline."`.

### 7.2 Generated Test Runner (`generated/claw-test-runner.js`)

The compiler emits a self-contained ESM test runner alongside the MCP server. It is generated from `test` and `mock` blocks only and has no dependency on `opencode` or `@modelcontextprotocol/sdk`.

```javascript
// generated/claw-test-runner.js
// AUTO-GENERATED by clawc build --lang opencode
// DO NOT EDIT — re-run clawc to regenerate

import assert from "node:assert/strict";

// ── Mock Registry ──────────────────────────────────────────────────────────────
const MOCKS = {
  Researcher: { url: "https://apple.com/news", confidence_score: 0.95, snippet: "...", tags: ["hardware"] },
  // ... one entry per mock block (last definition wins)
};

function lookupMock(agentName) {
  return MOCKS[agentName] ?? null;
}

// ── Test Execution ─────────────────────────────────────────────────────────────
async function executeAgent(agentName, kwargs) {
  const mock = lookupMock(agentName);
  if (mock === null) {
    throw new Error(`No mock defined for agent '${agentName}'. Add a mock block to run this test offline.`);
  }
  return mock;
}

// ── Test Definitions ───────────────────────────────────────────────────────────
const TESTS = [
  {
    name: "Researcher returns valid SearchResult",
    async run() {
      const result = await executeAgent("Researcher", { task: "Find announcements for Apple" });
      // assert statements compiled into direct node:assert calls:
      assert.notEqual(result.url, "", "url must not be empty");
      assert.ok(result.confidence_score > 0.5);
      assert.ok(result.tags.length > 0, "must have at least one tag");
    }
  },
  // ... one entry per test block
];

// ── Runner ────────────────────────────────────────────────────────────────────
const filter = process.argv.find((a, i) => process.argv[i-1] === "--filter");
const tests = filter ? TESTS.filter(t => t.name.toLowerCase().includes(filter.toLowerCase())) : TESTS;
const TEST_TIMEOUT_MS = Number(process.env.CLAW_TEST_TIMEOUT_MS ?? 30_000);

if (tests.length === 0) {
  console.log(`No tests matched filter '${filter}'`);
  process.exit(0);
}

console.log(`Running ${tests.length} test(s)...`);
let passed = 0, failed = 0;
const start = Date.now();

for (const t of tests) {
  const tStart = Date.now();
  try {
    await Promise.race([
      t.run(),
      new Promise((_, rej) => setTimeout(() => rej(new Error(`Test timed out after ${TEST_TIMEOUT_MS}ms`)), TEST_TIMEOUT_MS))
    ]);
    const ms = Date.now() - tStart;
    console.log(`  PASS  ${t.name} (${ms}ms)`);
    passed++;
  } catch (err) {
    const ms = Date.now() - tStart;
    console.log(`  FAIL  ${t.name} (${ms}ms)\n        ${err.message}`);
    failed++;
  }
}

console.log(`\nResults: ${passed} passed, ${failed} failed (${Date.now() - start}ms total)`);
process.exit(failed > 0 ? 1 : 0);
```

### 7.3 Compiler Emitter (`src/codegen/test_runner.rs`)

The `generate_test_runner(document)` function is called by the OpenCode codegen stage when `test` blocks or `mock` blocks are present. It:

1. Emits the `MOCKS` object from all `mock` blocks in definition order (last wins per agent name)
2. Emits the `TESTS` array — each test block body is compiled to a series of `await executeAgent(...)` calls and `assert.*` calls
3. Emits the runner harness (identical structure per project)
4. Writes to `generated/claw-test-runner.js`

**`assert` statement compilation:**

| `.claw` assert | Compiled to |
|---|---|
| `assert x != ""` | `assert.notEqual(x, "")` |
| `assert x > 0.5` | `assert.ok(x > 0.5)` |
| `assert x > 0.5, "msg"` | `assert.ok(x > 0.5, "msg")` |
| `assert x.length() > 0` | `assert.ok(x.length > 0)` |

### 7.4 CLI Integration

`claw test` in `src/bin/claw.rs`:
1. Compile the `.claw` source (runs full pipeline)
2. Emit `generated/claw-test-runner.js` (if test/mock blocks exist)
3. Execute: `node generated/claw-test-runner.js [--filter <pattern>]`
4. Parse stdout for results (JSON lines format OR human-readable — the runner outputs human-readable directly)
5. Exit with code 0 (all pass), 1 (any fail), 2 (compile error), 4 (I/O error)

No gateway subprocess. No gateway manifest. No stdin JSON protocol. Direct Node.js execution of the generated test runner file.

### 7.5 Blast Radius (OpenCode era)

Files affected by `claw test` implementation (replaces gateway blast radius list in §3):

1. `src/ast.rs` — `MockDecl` and `TestDecl` structs (unchanged shape from §3)
2. `src/parser.rs` — `mock_decl` and `test_decl` combinators (unchanged from §3)
3. `src/codegen/test_runner.rs` — NEW: test runner emitter
4. `src/codegen/opencode.rs` — call `generate_test_runner()` when test/mock blocks exist
5. `specs/03-Grammar.md` — `mock_decl` grammar (unchanged from §3)
6. `specs/04-AST-Structures.md` — `MockDecl` and `TestDecl` (unchanged from §3)
7. `generated/claw-test-runner.js` — generated output (add to `.gitignore`)

**Retired files (archived gateway, no longer relevant):**

- `openclaw-gateway/src/test-runner.ts` — archived
- `openclaw-gateway/src/engine/traversal.ts` — archived
- `openclaw-gateway/src/engine/errors.ts` — archived
- `openclaw-gateway/src/types.ts` — archived
