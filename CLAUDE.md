# OpenClaw DSL (`.claw`) repository rules for AI Agents

Welcome to the `Open-code` repository for the `.claw` compiler and DSL. If you are an AI agent contributing to this repository, you must strictly adhere to the following rules and standards.

## 1. Core Philosophy: Determinism & Clean Code
- **Explicit over Implicit**: We are building a strict, deterministic orchestration compiler. There should be no "magic" in our code. Variable naming, function execution paths, and memory management (Rust) must be 100% explicit.
- **Small, Modular Functions**: Functions should do exactly one thing. If a function in Rust or TypeScript exceeds 50 lines, it should be heavily scrutinized and likely broken down into smaller, testable private functions.
- **Fail Fast, Fail Loudly**: The compiler (`clawc`) should crash and print a human-readable error immediately if it detects a `.claw` syntax violation or type mismatch. Never silently recover or guess the developer's intent.

## 2. Programming Standards (Rust)
- **Error Handling**: Use `Result<T, E>` exhaustively. Never use `.unwrap()` or `.expect()` unless you can mathematically prove the branch is unreachable. Define custom error enums using `thiserror` for the parser and semantic analyzer.
- **Immutability**: variables should be immutable by default. Only use `mut` when absolutely necessary for performance in AST traversal.
- **Lifetimes**: Avoid complex lifetimes (`'a`) unless necessary for zero-copy parsing. It is often preferable to `.clone()` a string during the early AST pipeline to keep the developer experience robust, optimizing later.

## 3. Programming Standards (Generated SDKs: TS & Python)
- **TypeScript**: Use strict mode (`"strict": true`). Absolutely no `any` types. Everything must be typed using Zod or TypeBox depending on the exact Gateway requirements.
- **Python**: Use Python 3.12+ type hinting exhaustively. Use `pydantic` for modeling the generated data shapes.

## 4. STRICT Test-Driven Development (TDD)
- **TESTS BEFORE CODE (NON-NEGOTIABLE)**: Before any feature is built in Rust (lexer, parser, semantic analyzer, etc.), you MUST write the `#[test]` block first. The test will initially fail. Only *after* the failing test is written are you allowed to implement the actual Rust code to make it pass.
- **Unit Testing First**: You must write unit tests for every parser combinator and semantic check, strictly following TDD workflow.
- **Rust Tests**: Place `#[cfg(test)]` modules at the bottom of the exact file you are building.
- **Error Tests are Priority**: Testing that the compiler *successfully* parses a file is only 50% of the job. You MUST write tests ensuring the compiler *fails gracefully* when given malformed syntax or type mismatches.
- **Snapshots**: For AST generation, use snapshot testing (e.g., `cargo-insta` in Rust) to easily verify tree outputs against expected states.
- **Reference Testing Spec**: You must read `specs/08-Testing-Spec.md` to understand exactly how the tests should be structured before writing any code.

## 5. Architectural Flow
1. **Lexing/Parsing**: Convert raw text to tokens.
2. **Semantic Analysis**: Verify tool existence, block loops, and valid type handoffs.
3. **IR Lowering**: Convert `.claw` types to TypeBox schemas.
4. **Emission**: Write strings to SDK files.

## 6. Context Management & Progressive Disclosure
Following industry best practices for agent instructions:
- **Progressive Disclosure**: Do not try to read or embed large swathes of the codebase into your context window upfront. Instead, search for what you need *when you need it*. If you require specific information about the TypeBox schema generation, search `openclaw/src/agents/schema/` instead of dumping files.
- **Reference Over Copying**: When citing existing code within the repo, use `@` style file:line references (e.g. `src/compiler/parser.rs:45`) rather than copy-pasting massive snippets into your memory. This keeps the prompt context lean and prevents performance degradation.
- **Conciseness**: Keep documentation, PR logs, and comments extremely concise. Only provide exactly what is needed for the current task.

## 7. Commit and Documentation Rules
- **Document the "Why"**: Code comments should not explain *what* the code does (the Rust compiler tells us what it does). Comments must explain *why* we chose this specific approach (e.g., "We are avoiding anyOf here because Claude 3.5 rejects it in tool schemas").
- **Update Specifications**: If an implementation detail deviates from `specs/02-Compiler-Architecture.md` or `specs/01-DSL-Core-Specification.md`, you MUST update the spec file first.

By following these rules, we ensure the `.claw` toolchain remains a robust, enterprise-grade execution environment.
