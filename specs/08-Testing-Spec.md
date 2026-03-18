# OpenClaw Compiler: Testing Specification

This document defines the strict testing methodology for the `.claw` compiler (`clawc`). We utilize a 100% Test-Driven Development (TDD) approach. 

## 1. The TDD Golden Rule — 7-Step Cycle (NON-NEGOTIABLE)

**Before any feature is implemented, a failing test MUST be written first.** This applies to Rust (`#[test]`), TypeScript (`node:test`), and Python (`pytest`).

The exact workflow for every feature:

1. **Read the spec.** If building a parser combinator, read `specs/03-Grammar.md`. If touching the gateway, read `specs/07-OpenClaw-OS.md`. If touching security, read `specs/12-Security-Model.md`.
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

The following security properties MUST have dedicated tests (see `specs/12-Security-Model.md`):

| Property | Test Description |
|----------|-----------------|
| Timing-safe API key | Verify `crypto.timingSafeEqual` is used, not `===` |
| Request body limit | Send >1MB body, assert HTTP 413 or connection reset |
| Symlink rejection | Create symlink to outside workspace, assert tool resolution fails |
| Malformed WebSocket | Send incomplete frame buffer, assert no crash |
| Predictable session ID | Assert session IDs contain UUID format, not timestamps |
| Exit code mapping | Run sandbox with OOM, assert exit code 137 maps to `SandboxOOMError` |

## 5. Gateway Integration Test Patterns

End-to-end gateway tests should:
1. Load a real compiled `document.json` (produced by `clawc build`)
2. Execute a workflow through the traversal engine with a mock or in-memory checkpoint store
3. Validate the result against the expected schema
4. Verify checkpoint persistence and idempotent replay (same session_id returns cached result)

## 6. LLM Mock Patterns

Tests MUST NOT call real LLM APIs. The gateway's LLM bridge falls back to a mock response generator when no API keys are configured. Tests should:
- Verify mock responses conform to the TypeBox schema
- Verify schema degradation detection on deliberately empty mock responses
- Verify error handling when the LLM bridge returns non-JSON
