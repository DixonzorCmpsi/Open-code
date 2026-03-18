# Phase 6C: Test Runner & Mock Execution

This spec covers the `openclaw test` command and the gateway's ability to execute `.claw` test and mock blocks. Developers write tests alongside workflows and run them without hitting real LLM APIs.

**Prerequisite:** Read `specs/01-DSL-Core-Specification.md` (test/mock syntax), `specs/03-Grammar.md`, `specs/04-AST-Structures.md`, `specs/14-CLI-Tooling.md`.

---

## 0. Goals & Non-Goals

### Goals (MUST do)
- Add `assert` as a first-class statement with optional message: `assert expr, "message"`
- `assert` is ONLY valid inside `test` blocks — compile error elsewhere (`InvalidAssertOutsideTest`)
- Replace `MockDecl` AST (BREAKING CHANGE) — old `mock Agent(input) -> output` becomes `mock Agent { key: value }`
- Implement mock registry with name-only matching (last mock for same agent wins)
- Mock interception is HIGHEST priority in `executeAgentRun()` — before tools, BAML, and raw HTTP
- Implement `openclaw test` CLI command with `--filter` substring matching
- Implement `test-runner.ts` as gateway subprocess reading JSON manifest from stdin, outputting JSON lines to stdout
- Enforce 30s per-test timeout (configurable via `OPENCLAW_TEST_TIMEOUT_MS`)
- Tests pass when ALL assertions succeed and no unhandled exception propagates; tests fail on first false assertion or any thrown error

### Non-Goals (MUST NOT do)
- Do NOT implement input-pattern mock matching (`mock Agent(task: "pattern")`) — Phase 7
- Do NOT implement test coverage reporting — Phase 7
- Do NOT implement test suites/grouping (`suite "name" { ... }`) — Phase 8
- Do NOT implement `beforeEach`/`afterEach` setup/teardown hooks — Phase 8
- Do NOT implement parallel test execution — tests run sequentially in definition order
- Do NOT implement watch mode for tests (`openclaw test --watch`) — Phase 7
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
2. During `openclaw test`, the gateway traversal engine executes each `TestDecl.body` like a workflow body.
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

## 4. `openclaw test` CLI Command

### Usage

```
openclaw test [source.claw] [--config openclaw.json] [--filter "pattern"]
```

### Rust CLI Implementation

Add `Test(TestArgs)` to the `Commands` enum in `openclaw.rs`:

```rust
#[derive(Debug, clap::Args)]
struct TestArgs {
    source: Option<PathBuf>,
    #[arg(long)]
    filter: Option<String>,
    #[arg(long, default_value = "openclaw.json")]
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

1. **`test_parses_test_command`** — `Cli::parse_from(["openclaw", "test", "example.claw"])` → `Commands::Test(...)`.
2. **`test_parses_test_filter`** — `Cli::parse_from(["openclaw", "test", "--filter", "Researcher"])` → `filter: Some("Researcher")`.
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

Configurable via `OPENCLAW_TEST_TIMEOUT_MS` environment variable.

```typescript
const TEST_TIMEOUT_MS = Number(process.env.OPENCLAW_TEST_TIMEOUT_MS ?? 30_000);

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
