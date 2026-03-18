# OpenClaw Compiler: Implementation Flow

This document serves as the exact code process specification for the Agent building the `clawc` compiler. It defines the codebase structure and the strict order of operations required to build the compiler successfully.

## 1. Codebase Structure (`src/`)

The repository will be structured as a standard Rust CLI application (`cargo new clawc --bin`).

```text
├── Cargo.toml               # Dependencies (winnow, thiserror, minijinja, clap)
├── src/
│   ├── main.rs              # CLI entry point (clap setup), orchestrates the 4 phases
│   ├── ast.rs               # The exact data structures defined in `specs/04-AST-Structures.md`
│   ├── lexer.rs             # Optional: Tokenization (if separating from parser)
│   ├── parser.rs            # `winnow` combinators matching `specs/03-Grammar.md`
│   ├── semantic/
│   │   ├── mod.rs           # The Type System engine
│   │   ├── symbols.rs       # Symbol table resolution (Pass 1)
│   │   └── types.rs         # Type mismatch/boundary checking (Pass 2 & 3)
│   ├── codegen/
│   │   ├── mod.rs           # The template engine (minijinja)
│   │   ├── typescript.rs    # TS Emitter
│   │   └── python.rs        # Python Emitter
│   └── errors.rs            # Custom `CompilerError` enums with Spans for beautiful CLI reporting
└── tests/
    └── integration.rs       # End-to-end tests
```

## 2. Order of Operations (The Builder's Path)

The builder MUST construct the compiler sequentially. Do not jump to Phase 3 (CodeGen) before Phase 1 (Parsing) is fully tested and verified. Refer to `specs/08-Testing-Spec.md` to ensure TDD is followed at every step.

### Step 1: Foundation (AST & Errors)
1. Initialize the `Cargo.toml` with `winnow`, `thiserror`, `minijinja`, and `clap`.
2. Implement `src/ast.rs` exactly as spec'd out in the architectural docs. (Write a basic test instantiating an AST node).
3. Implement `src/errors.rs` to define the error types that will be used across the pipeline.

### Step 2: The Parser engine
1. Open `src/parser.rs`.
2. Follow TDD: Write failing tests for parsing primitive types (`string`, `int`), literals, and keywords.
3. Build the bottom-up `winnow` combinators to parse identifiers and types.
4. Move up the chain: Write tests and combinators for `tool`, `agent`, and `workflow` blocks.
5. Finish the parser by outputting the full `ast::Document` root. Use `cargo insta` to snapshot test the output.

### Step 3: Semantic Analysis
1. Open `src/semantic/mod.rs`.
2. Write tests passing dummy AST trees directly to the Analyzer.
3. Implement Pass 1: Symbol Table accumulation (Fail if duplicate tools/agents exist).
4. Implement Pass 2: Reference Validation (Fail if an agent uses a non-existent tool).
5. Implement Pass 3: Type Checking (Ensure variable assignments and tool calls match the required types).

### Step 4: Code Generation
1. Open `src/codegen/mod.rs`.
2. Write tests passing dummy valid `ast::Document` trees and asserting against string outputs.
3. Implement `typescript.rs`: Traverse the AST and write the strings required for the Gateway Client API contract (detailed in `specs/06-CodeGen-SDK.md`).
4. Implement TypeBox JSON lowering (converting the AST types to strict JSON Schema).

### Step 5: The CLI Orchestrator
1. Open `src/main.rs`.
2. Implement the `clap` CLI arguments (`clawc build source.claw --lang ts`).
3. Connect the parser -> analyzer -> codegen pipeline.
4. Read the target `.claw` file, run the pipeline, print gorgeous console errors if they fail, and output the generated SDK files to disk if it succeeds.
