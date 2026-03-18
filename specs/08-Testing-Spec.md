# OpenClaw Compiler: Testing Specification

This document defines the strict testing methodology for the `.claw` compiler (`clawc`). We utilize a 100% Test-Driven Development (TDD) approach. 

## 1. The TDD Golden Rule

**Before any feature is implemented in Rust, a failing test MUST be written and committed first.**

If you are tasked with building the "Agent Parser Combinator", your exact flow must be:
1. Open `src/parser.rs` (or create it).
2. Write the `#[cfg(test)]` module at the bottom.
3. Write `#[test] fn test_parse_agent() { ... }` with explicit assertions on what the `winnow` combinator *should* output.
4. Run `cargo test` and watch it fail to compile/run.
5. Only then may you write the actual `parse_agent` Rust function to make the test pass.

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

## 3. General Rust Testing Rules
* All tests for a specific module should live in that module's file (e.g., `src/semantic.rs` contains `mod tests`).
* Integration tests (testing the CLI pipeline end-to-end from `.claw` file to `.ts` file) should live in a separate `tests/ integration_test.rs` directory.
* Run tests with the highest level of strictness and warning flags enabled for `cargo clippy`.
