# Claw Compiler: Implementation Flow

This document specifies the exact build order for the `clawc` compiler and the OpenCode integration layer. It defines codebase structure and the strict sequence of operations required to build the system successfully.

**Minimum toolchain versions:**
- **Rust:** `1.75.0` stable or newer. `Cargo.toml` MUST declare `rust-version = "1.75"`.
- **Node.js:** `20.0.0` or newer (for the generated MCP server and TypeScript SDK).
- **OpenCode:** Latest stable (`opencode.ai/install`) — required to run compiled workflows interactively.

---

## 1. Codebase Structure (`src/`)

```text
├── Cargo.toml               # Dependencies (winnow, thiserror, minijinja, clap, notify, tower-lsp, sha2)
├── src/
│   ├── lib.rs               # Library root, exports public modules
│   ├── main.rs              # `clawc` CLI entry point
│   ├── ast.rs               # AST data structures (specs/04-AST-Structures.md)
│   ├── parser.rs            # winnow combinators (specs/03-Grammar.md)
│   ├── semantic/
│   │   ├── mod.rs           # Type System engine (3-pass analysis)
│   │   ├── symbols.rs       # Symbol table resolution (Pass 1)
│   │   └── types.rs         # Reference validation (Pass 2) + type checking (Pass 3)
│   ├── codegen/
│   │   ├── mod.rs           # Template engine (minijinja), TypeBox lowering, AST hashing
│   │   ├── typescript.rs    # TS SDK emitter (Zod schemas)
│   │   ├── python.rs        # Python SDK emitter (Pydantic models)
│   │   ├── opencode.rs      # OpenCode config emitter (opencode.json, agent/command markdown)
│   │   ├── mcp.rs           # MCP server emitter (generated/mcp-server.js)
│   │   └── baml.rs          # BAML integration codegen (specs/18-BAML-Integration-Layer.md)
│   ├── errors.rs            # CompilerError enums with Spans + exit code mapping
│   ├── config.rs            # claw.json configuration (specs/14-CLI-Tooling.md)
│   ├── lsp.rs               # LSP utilities (diagnostics, completion, semantic tokens)
│   └── bin/
│       ├── claw.rs          # `claw` CLI (init, build, dev) (specs/14-CLI-Tooling.md)
│       └── claw-lsp.rs      # LSP server binary (tower-lsp)
├── python-sdk/              # Hand-written Python client library
├── npm-cli/                 # NPM wrapper package (@claw/cli)
├── vscode-extension/        # VSCode language extension
└── tests/
    └── integration.rs       # End-to-end Rust tests
```

**Retired directories (archived, not built):**
- `archived/openclaw-gateway/` — replaced by OpenCode runtime
- `archived/packages/openclaw-sdk/` — replaced by OpenCode integration

---

## 2. Order of Operations (The Builder's Path)

Build the compiler sequentially. Do not jump to Phase 3 (CodeGen) before Phase 1 (Parsing) is fully tested. Follow TDD at every step per `specs/08-Testing-Spec.md`.

### Step 1: Foundation (AST & Errors)

1. Initialize `Cargo.toml` with `winnow`, `thiserror`, `minijinja`, `clap`.
2. Implement `src/ast.rs` per `specs/04-AST-Structures.md`. Write a basic test instantiating an AST node.
3. Implement `src/errors.rs` with all error types used across the pipeline.

### Step 2: The Parser Engine

1. Open `src/parser.rs`.
2. TDD: Write failing tests for primitive types (`string`, `int`), literals, keywords.
3. Build bottom-up `winnow` combinators: identifiers, types, expressions.
4. Add tests and combinators for `tool`, `agent`, `workflow`, `client`, `type` blocks.
5. Finish parser outputting full `ast::Document`. Snapshot-test with `cargo insta`.

### Step 3: Semantic Analysis

1. Open `src/semantic/mod.rs`.
2. Write tests passing dummy AST trees directly to the Analyzer.
3. **Pass 1:** Symbol Table accumulation (fail on duplicate tools/agents/types).
4. **Pass 2:** Reference Validation (fail if agent uses non-existent tool or client).
5. **Pass 3:** Type Checking (variable assignment and tool call type compatibility, exhaustive return analysis).

### Step 4: TypeBox Lowering

1. In `src/codegen/mod.rs`, implement TypeBox IR lowering.
2. Convert all `type` blocks → JSON Schema objects used by MCP server and SDK validation.
3. Write snapshot tests for each primitive and composite type.

### Step 5: TypeScript + Python SDK CodeGen

1. Implement `typescript.rs`: Zod schemas + typed async workflow functions.
2. Implement `python.rs`: Pydantic models + typed async workflow functions.
3. Both import from the generated SDK — no direct LLM or tool calls.
4. Tests: Pass valid AST, assert string output matches golden snapshots.

### Step 6: OpenCode CodeGen (Primary Target)

1. Implement `src/codegen/opencode.rs`:
   - `emit_opencode_json(document)` → `opencode.json`
   - `emit_agent_markdowns(document)` → `.opencode/agents/{Name}.md` per agent
   - `emit_command_markdowns(document)` → `.opencode/commands/{Name}.md` per workflow
   - `emit_context_md(document)` → `generated/claw-context.md`
   - `emit_test_runner(document)` → `generated/claw-test-runner.js` (if test/mock blocks exist)
   - Full mapping rules in `specs/25-OpenCode-Integration.md`
2. Implement `src/codegen/mcp.rs`:
   - `emit_mcp_server(document)` → `generated/mcp-server.js`
   - Full generation rules in `specs/26-MCP-Server-Generation.md`
3. Tests: Snapshot-test all emitted files for a canonical `.claw` example.

### Step 7: CLI Orchestrator

1. Open `src/bin/claw.rs`.
2. Implement `clap` CLI: `claw init`, `claw build`, `claw dev`, `claw test`.
3. `claw build --lang opencode` runs Steps 4-6 emitters in sequence.
4. `claw build --lang ts` runs Steps 4-5 TypeScript emitter.
5. Connect parser → analyzer → IR → codegen pipeline end-to-end.
6. Gorgeous console errors with file:line:col + caret on failure.
7. Map errors to exit codes per `specs/02-Compiler-Architecture.md §5`.

### Step 8: LSP Language Server

1. Implement `claw-lsp` in `src/bin/claw-lsp.rs` using `tower-lsp`.
2. On document open/change: re-parse + run semantic analysis, publish diagnostics.
3. Completion: keywords + document symbols (agents, types, tools, workflows).
4. Reuses the same `parser::parse()` and `semantic::analyze()` — no duplication.

### Step 9: Binary Distribution

1. Implement `npm-cli/postinstall.js` per `specs/19-Binary-Distribution.md`.
2. Set up GitHub Actions matrix (5 targets) per `specs/19 §2`.
3. `claw init` scaffolds `package.json` with `@claw/cli` devDependency + OpenCode install instructions.

---

## 3. Testing Gates (Non-Negotiable)

Each step MUST pass its tests before proceeding to the next:

| Step | Test Command | Must Pass |
|------|-------------|-----------|
| 1 | `cargo test -p clawc` | AST instantiation |
| 2 | `cargo test -p clawc parser` + `cargo insta review` | All parser snapshots |
| 3 | `cargo test -p clawc semantic` | All semantic analyzer tests |
| 4 | `cargo test -p clawc codegen::mod` | TypeBox lowering snapshots |
| 5 | `cargo test -p clawc codegen::typescript codegen::python` | SDK snapshots |
| 6 | `cargo test -p clawc codegen::opencode codegen::mcp` | OpenCode config snapshots |
| 7 | `cargo test --test integration` | End-to-end CLI tests |
| 8 | `cargo test -p claw-lsp` | LSP diagnostics + completion |

After every step: `cargo clippy -- -D warnings` MUST produce zero warnings.

---

## 4. The User Experience Goal

A developer should be able to go from zero to running their first Claw workflow in under 5 minutes:

```bash
# 1. Install Claw compiler (once, global)
npm install -g @claw/cli

# 2. Install OpenCode (once, global)
curl -fsSL https://opencode.ai/install | bash

# 3. Create a new project
mkdir my-agents && cd my-agents
claw init

# 4. Edit your pipeline
# (write your .claw file with types, tools, agents, workflows)

# 5. Compile to OpenCode
claw build --lang opencode

# 6. Run interactively
opencode /MyWorkflow "some input"

# OR use programmatically
npm install
node -e "import('./generated/claw/index.js').then(m => m.MyWorkflow('input').then(console.log))"
```

This 6-step flow is the north star. Every implementation decision should make it simpler, not harder.
