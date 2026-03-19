# Claw Compiler: Testing Specification

This document defines the strict testing methodology for the `.claw` compiler (`clawc`). We utilize a 100% Test-Driven Development (TDD) approach. 

## 1. The TDD Golden Rule — 7-Step Cycle (NON-NEGOTIABLE)

**Before any feature is implemented, a failing test MUST be written first.** This applies to Rust (`#[test]`), TypeScript (`node:test`), and Python (`pytest`).

The exact workflow for every feature:

1. **Read the spec.** If building a parser combinator, read `specs/03-Grammar.md`. If touching the MCP server or OpenCode config emitter, read `specs/25-OpenCode-Integration.md` and `specs/26-MCP-Server-Generation.md`. If touching compiler security, read `specs/12-Security-Model.md §7`.
2. **Write the test.** Create the `#[test]` or `test()` block with explicit assertions on inputs and expected outputs. Include BOTH happy path and error path tests.
3. **Run the test suite — confirm FAILURE (red).** The test must fail because the implementation doesn't exist yet. If it passes, you're testing something that already works or your test is wrong.
4. **Write the MINIMUM code** to make the test pass. No extra features, no premature abstractions.
5. **Run the test suite — confirm PASS (green).** All tests (not just the new one) must pass.
6. **Refactor** for clarity, modularity, and performance. Ensure functions stay under 50 lines.
7. **Run `cargo clippy` / `eslint` AND the full test suite again.** The refactored code must pass all static analysis and tests.

## 2. Test Structure per Phase

### Phase A: Lexer & Parser Combinator Tests
For every single AST node defined in `specs/ast.md`, there must be:
1. A **happy path** test verifying perfectly formatted `.claw` syntax parses into the exact expected Rust Struct.
2. A **syntax error** test providing malformed syntax (e.g., missing curly braces) and asserting that the `winnow` parser throws a precise span error, rather than panicking or silently failing.

*Tooling:* We will use `cargo-insta` for Snapshot Testing here. Instead of manually asserting 50 fields on an AST tree, write a test that serializes the AST to text and asserts against a saved snapshot.

### Phase B: Semantic Analyzer (Type System) Tests
The Type System tests do not test text parsing. They test the AST validation logic.

You must manually construct a dummy `ast::Document` in your test code (bypass the parser) and pass it to the Semantic Analyzer.
1. **Happy Path:** Create an AST where tools are properly declared and used, and assert `Analyzer::validate()` returns `Ok(())`.
2. **Type Error Paths:**
    * Create an AST where `Agent A` requests `WebScraper`, but `WebScraper` is missing from the tree. Assert `Analyzer::validate()` returns `Err(CompilerError::UndefinedTool)`.
    * Create an AST where `Agent A` passes a `string` to a workflow expecting a `int`. Assert `CompilerError::TypeMismatch`.

### Phase C: CodeGen Tests
For code generation, the tests must take a valid, semantic-checked AST and invoke the templating engine.
* You must assert that the string output exactly matches the expected TypeScript or Python boilerplate strings defined in `specs/codegen.md`.
* Use `insta` snapshot tests to verify the emitted code against approved golden files.

## 3. General Testing Rules
* **Rust:** All unit tests live in `#[cfg(test)] mod tests` at the bottom of the module file. Integration tests live in `tests/integration.rs`.
* **TypeScript:** All unit tests live in `*.test.ts` files adjacent to the module. Use `node:test` and `node:assert/strict`.
* **Python:** Use `pytest` with type-hinted test functions.
* Run tests with the highest level of strictness and warning flags enabled (`cargo clippy`, `eslint`).

## 4. Security Testing Requirements

The following security properties MUST have dedicated tests. Note: runtime security (auth, sessions, request limits, WebSocket) is delegated to OpenCode. Claw-owned security is in the **compiler** and the **generated MCP server** only. See `specs/12-Security-Model.md §7` and `specs/26-MCP-Server-Generation.md §5`.

| Property | Owner | Test Description |
|----------|-------|-----------------|
| Parser no-panic | `clawc` compiler | Feed deeply nested `.claw` (256+ levels), assert no panic — returns `CompilerError::ParseError` |
| Numeric overflow | `clawc` compiler | Parse integer exceeding `i64::MAX`, assert `CompilerError::ParseError` with span |
| Path traversal rejection (compile-time) | `clawc` compiler | `invoke: module("../../etc/passwd")`, assert `CompilerError::InvalidToolPath` |
| Path traversal rejection (runtime) | `generated/mcp-server.js` | Symlink inside workspace pointing outside, assert handler throws "resolves outside workspace" |
| MCP input schema validation | `generated/mcp-server.js` | Call tool with missing required arg, assert `isError: true` and message includes "required" |
| MCP handler error isolation | `generated/mcp-server.js` | Mock module throws, assert `isError: true` and MCP server process stays running |
| Compiler warning on `retries` | `clawc` compiler | `client` block with `retries = 3`, `--lang opencode`, assert compiler emits warning and does NOT emit `retries` in `opencode.json` |
| OpenCode config validity | `clawc` emitter | `clawc build --lang opencode`, assert `opencode.json` is valid JSON with required fields |

## 5. OpenCode Integration Test Patterns

End-to-end integration tests (replacing the retired gateway integration tests):

1. Run `clawc build --lang opencode` on a reference `.claw` file
2. Assert all expected output files exist: `opencode.json`, `.opencode/agents/*.md`, `.opencode/commands/*.md`, `generated/mcp-server.js`, `generated/claw-context.md`
3. Assert `opencode.json` is valid JSON and contains `mcp.claw-tools` pointing to `generated/mcp-server.js`
4. Start `generated/mcp-server.js` in-process, send `ListToolsRequest`, assert tool count matches `tool` block count
5. Call each tool via `CallToolRequest` with valid inputs (mock the underlying module), assert valid output
6. Assert that re-running `clawc build` on a project with an existing `opencode.json` preserves non-Claw fields (merge strategy test)

## 6. Offline Test Execution Patterns

`claw test` runs entirely offline (no LLM, no OpenCode). Tests should:

- Verify `assert` statements in test blocks compile to `node:assert` calls in `generated/claw-test-runner.js`
- Verify mock registry intercepts agent calls before any LLM routing
- Verify that a test block executing an agent with no mock fails with "No mock defined for agent '...'"
- Verify test timeout (default 30s, configurable via `CLAW_TEST_TIMEOUT_MS`) kills hanging tests
- Verify `--filter` substring matching selects correct subset of tests

See `specs/17-Phase6-Test-Runner-And-Mocks.md §7` for the generated test runner spec.
