# OpenClaw Compiler: Implementation Flow

This document serves as the exact code process specification for the Agent building the `clawc` compiler. It defines the codebase structure and the strict order of operations required to build the compiler successfully.

## 1. Codebase Structure (`src/`)

The repository will be structured as a standard Rust CLI application (`cargo new clawc --bin`).

```text
├── Cargo.toml               # Dependencies (winnow, thiserror, minijinja, clap, ctrlc, notify, tower-lsp, sha2)
├── src/
│   ├── lib.rs               # Library root, exports public modules
│   ├── main.rs              # `clawc` CLI entry point
│   ├── ast.rs               # AST data structures (specs/04-AST-Structures.md)
│   ├── parser.rs            # `winnow` combinators (specs/03-Grammar.md)
│   ├── semantic/
│   │   ├── mod.rs           # The Type System engine (3-pass analysis)
│   │   ├── symbols.rs       # Symbol table resolution (Pass 1)
│   │   └── types.rs         # Reference validation (Pass 2) & type checking (Pass 3)
│   ├── codegen/
│   │   ├── mod.rs           # Template engine (minijinja), TypeBox lowering, AST hashing
│   │   ├── typescript.rs    # TS SDK emitter (Zod schemas)
│   │   └── python.rs        # Python SDK emitter (Pydantic models)
│   ├── errors.rs            # CompilerError enums with Spans + exit code mapping
│   ├── config.rs            # openclaw.json configuration (specs/14-CLI-Tooling.md)
│   ├── lsp.rs               # LSP utilities (diagnostics, completion, semantic tokens)
│   └── bin/
│       ├── openclaw.rs      # `openclaw` CLI (init, build, dev) (specs/14-CLI-Tooling.md)
│       └── claw-lsp.rs      # LSP server binary (tower-lsp)
├── openclaw-gateway/        # TypeScript execution OS (specs/07-OpenClaw-OS.md)
│   └── src/
│       ├── server.ts        # HTTP server + WebSocket upgrade
│       ├── auth.ts          # API key authentication (specs/12-Security-Model.md)
│       ├── ws.ts            # WebSocket protocol (specs/11-WebSocket-Protocol.md)
│       ├── engine/
│       │   ├── traversal.ts # AST execution engine with exhaustive checkpointing
│       │   ├── checkpoints.ts # SQLite + Redis checkpoint backends
│       │   ├── llm.ts       # LLM provider bridges (OpenAI, Anthropic)
│       │   ├── schema.ts    # TypeBox schema validation + degradation detection
│       │   ├── runtime.ts   # Docker/local sandbox execution with timeout
│       │   ├── ast.ts       # AST navigation utilities
│       │   └── errors.ts    # Gateway error types
│       ├── tools/
│       │   ├── browser.ts   # Playwright browser automation
│       │   └── vision.ts    # Visual Intelligence bridge (specs/13-Visual-Intelligence.md)
│       └── types.ts         # TypeScript type definitions
├── packages/openclaw-sdk/   # Hand-written TS client library
├── python-sdk/              # Hand-written Python client library
└── tests/
    └── integration.rs       # End-to-end Rust tests
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
5. Map errors to exit codes per `specs/02-Compiler-Architecture.md` Section 5.

### Step 6: Security Hardening (Gateway)
1. Implement timing-safe API key comparison in `auth.ts` (specs/12-Security-Model.md Section 2).
2. Add request body size limits to `server.ts` (specs/12-Security-Model.md Section 3).
3. Add security headers to all HTTP responses (specs/12-Security-Model.md Section 3.2).
4. Replace `Date.now()` session IDs with `crypto.randomUUID()` (specs/12-Security-Model.md Section 4).
5. Add `fs.realpath()` to tool path resolution (specs/12-Security-Model.md Section 5).

### Step 7: WebSocket Protocol (Gateway)
1. Implement WebSocket frame parser with bounds checking (specs/11-WebSocket-Protocol.md Section 3).
2. Implement streaming execution handler at `/workflows/stream` (specs/11-WebSocket-Protocol.md Section 4-5).
3. Implement close frame with write callback (specs/11-WebSocket-Protocol.md Section 3.3).
4. For production: migrate to `ws` library (specs/11-WebSocket-Protocol.md Section 1).

### Step 8: CLI Tooling
1. Implement `openclaw init` (specs/14-CLI-Tooling.md Section 2).
2. Implement `openclaw build --watch` (specs/14-CLI-Tooling.md Section 3).
3. Implement `openclaw dev` with gateway child process (specs/14-CLI-Tooling.md Section 4).
4. Implement `claw-lsp` language server (specs/14-CLI-Tooling.md Section 6).
