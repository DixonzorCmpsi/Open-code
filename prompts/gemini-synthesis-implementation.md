# Gemini Implementation Prompt: Claw Code Synthesis Pipeline (Specs 32 + 33)

**Date:** 2026-03-19
**Specs to implement:** `specs/32-Code-Synthesis-Pipeline.md`, `specs/33-Synthesis-Model-Interface.md`
**Compiler language:** Rust (`src/`)
**Generated output language:** TypeScript

---

## CRITICAL: Read Before Writing Any Code

You are implementing a **code synthesis pipeline** for the Claw DSL compiler. This is a significant architectural addition — do NOT touch existing functionality. The existing `invoke:` tool path, MCP server generation, OpenCode config generation, and AST interpreter ALL continue to work unchanged.

Read these files in full before writing any code:

1. `specs/32-Code-Synthesis-Pipeline.md` — full pipeline architecture, all DSL changes, artifact format, grammar additions, all invariants
2. `specs/33-Synthesis-Model-Interface.md` — synthesis model interface, synth-runner.js protocol, telemetry privacy rules
3. `src/ast.rs` — existing AST structures you must extend
4. `src/parser.rs` (or equivalent) — existing parser you must extend
5. `src/codegen/opencode.rs` — existing codegen to understand patterns
6. `src/codegen/mcp.rs` — existing MCP codegen to understand patterns
7. `src/bin/claw.rs` — existing CLI to understand the build pipeline shape

---

## What You Are Building

A 5-stage pipeline triggered by `claw build` when any tool uses `using:` instead of `invoke:`:

```
Stage 1: compile    → emit generated/artifact.clawa.json  (Rust, no LLM)
Stage 2: synthesize → emit generated/tools/*.ts            (LLM via child process)
Stage 3: test       → run vitest contract tests             (Node, no LLM)
Stage 4: bundle     → emit generated/bin/*.js              (esbuild)
Stage 5: execute    → deterministic runtime                 (Node)
```

`claw build` runs stages 1-4. Stage 5 is manual user execution.

---

## Implementation Tasks (in strict order)

### Task 1: Extend the AST (`src/ast.rs`)

Add these new AST node variants. Do NOT remove or rename any existing variants.

**New variants for `ToolDecl`:**
```rust
pub struct ToolDecl {
    // existing fields unchanged ...
    pub using: Option<UsingExpr>,          // NEW
    pub synthesizer: Option<String>,       // NEW — references synthesizer name
    pub test_block: Option<TestBlock>,     // NEW
}

pub enum UsingExpr {
    Fetch,
    Playwright,
    Bash,
    Mcp(String),   // server name
    Baml(String),  // function name
}

pub struct TestBlock {
    pub input: Vec<(String, SpannedExpr)>,   // field: value pairs
    pub expect: Vec<(String, ExpectOp)>,
}

pub enum ExpectOp {
    NotEmpty,
    Gt(f64),
    Lt(f64),
    Gte(f64),
    Lte(f64),
    Eq(SpannedExpr),
    Matches(String),
}
```

**New top-level declaration: `SynthesizerDecl`:**
```rust
pub struct SynthesizerDecl {
    pub name: String,
    pub client: String,       // references a ClientDecl by name
    pub temperature: Option<f64>,
    pub max_tokens: Option<u64>,
    pub span: Span,
}
```

Add `SynthesizerDecl` to `Document`:
```rust
pub struct Document {
    // existing fields unchanged ...
    pub synthesizers: Vec<SynthesizerDecl>,  // NEW
}
```

**New statement variant: `ReasonStmt`:**
```rust
// Add to Statement enum:
Statement::Reason {
    using_agent: String,
    input: String,           // variable name
    goal: String,
    output_type: DataType,
    bind: String,            // variable name to bind result to
    span: Span,
}
```

**New step types for artifact emission (internal IR, not AST):**
These are used during artifact serialization only — they do NOT need to be AST nodes. Define them in `src/codegen/artifact.rs` (new file):
```rust
pub enum WorkflowStep {
    ToolCall { tool: String, args: Vec<(String, StepValue)>, bind: String, bind_type: String },
    Reason { agent: String, input: String, input_type: String, goal: String, output_type: String, bind: String },
    ForLoop { item: String, iterable: StepValue, item_type: String, body: Vec<WorkflowStep> },
    IfCond { condition: StepCondition, then_body: Vec<WorkflowStep>, else_body: Vec<WorkflowStep> },
    TryCatch { body: Vec<WorkflowStep>, catch_var: String, catch_type: String, handler: Vec<WorkflowStep> },
    Return { value: StepValue, value_type: String },
}
```

---

### Task 2: Extend the Parser

Add grammar rules from `specs/32-Code-Synthesis-Pipeline.md §10` to the parser.

**New parsing rules:**

1. **`synthesizer_decl`** — top-level declaration:
   ```
   synthesizer DefaultSynth {
       client = MyClaude
       temperature = 0.1
       max_tokens = 8192
   }
   ```
   Parse into `SynthesizerDecl`. Add to document parser alongside `client_decl`.

2. **`using:` in tool blocks:**
   Inside a tool's `{}` block, parse:
   - `using: fetch`
   - `using: playwright`
   - `using: bash`
   - `using: mcp("server-name")`
   - `using: baml("FunctionName")`

3. **`synthesizer:` in tool blocks:**
   - `synthesizer: DefaultSynth` — references a synthesizer by name

4. **`test {}` in tool blocks:**
   ```
   test {
       input:  { query: "rust language" }
       expect: { url: !empty, snippet: !empty, confidence: >= 0.0 }
   }
   ```
   Valid `expect` operators: `!empty`, `>`, `<`, `>=`, `<=`, `== value`, `matches "regex"`

5. **`reason {}` in workflow body:**
   ```
   reason {
       using:       Writer
       input:       raw
       goal:        "Analyze the results and decide the best course of action"
       output_type: Decision
       bind:        decision
   }
   ```
   Parse into `Statement::Reason { ... }`. This is a statement, not an expression.

**Parser invariant:** If a `tool` block contains BOTH `invoke:` AND `using:`, emit `CompilerError::ParseError` with message: `"tool 'X' cannot declare both invoke: and using:"`.

**Parser invariant:** If a `tool` block contains `using:` but the document has no `synthesizer` declaration AND the tool has no `synthesizer:` field, this is NOT a parse error — it is a semantic error (check in semantic analyzer).

---

### Task 3: Extend the Semantic Analyzer (`src/semantic.rs`)

Add these semantic checks. Existing checks are unchanged.

1. **Tool path validation:**
   - Tool with neither `invoke:` nor `using:` → `CompilerError::ParseError { message: "tool 'X' must declare either invoke: or using:" }`
   - Tool with both → caught by parser (above)

2. **Missing synthesizer:**
   - Tool has `using:` AND `synthesizer: SomeName` → verify `SomeName` exists in `document.synthesizers`
   - Tool has `using:` AND no `synthesizer:` field → verify `document.synthesizers` is non-empty (first one is used as default)
   - If `using:` tool exists but no synthesizer declared anywhere → `CompilerError::ParseError { message: "tool 'X' uses using: but no synthesizer is declared. Add a synthesizer {} block." }`

3. **`reason {}` validation:**
   - `using:` agent must exist in `document.agents`
   - `input:` variable must be bound in scope at the point of the `reason {}` block
   - `output_type:` must be a declared type in `document.types`

4. **Synthesizer client validation:**
   - `synthesizer.client` must reference a declared `ClientDecl`

5. **`dynamic_reasoning` derivation:**
   - During semantic analysis, for each `reason {}` block, mark the referenced agent as `dynamic_reasoning = true`
   - This is stored on the agent in the resolved document, NOT declared in the DSL

---

### Task 4: Artifact Emitter (`src/codegen/artifact.rs` — new file)

Create a new codegen module that emits the `.clawa` JSON artifact. This is Stage 1 of the pipeline.

**Function signature:**
```rust
pub fn emit_artifact(document: &Document, project_root: &Path) -> CompilerResult<PathBuf>
```

Returns the path to the written artifact file (`generated/artifact.clawa.json`).

**Artifact structure:** Exactly as defined in `specs/32-Code-Synthesis-Pipeline.md §3`. Key points:

- `manifest.source_hash`: SHA-256 of the raw `.claw` source bytes. Use the `sha2` crate.
- `manifest.claw_version`: hardcoded to current package version from `Cargo.toml`
- `types[]`: ALL type declarations, with constraints
- `tools[]`: ONLY tools with `using:` (not `invoke:` tools — those go to MCP path)
- `agents[]`: ALL agent declarations. `runtime_client` = `agent.client` field. `dynamic_reasoning` = derived flag from semantic analysis. Do NOT emit `synthesis_client` on agents.
- `workflows[]`: ALL workflows. Steps are serialized per the step serialization rules in `specs/32-Code-Synthesis-Pipeline.md §11`. Each step includes `bind_type` / `value_type` populated from the semantic analyzer's type inference.
- `synthesizers[]`: ALL synthesizer declarations
- `capability_registry`: hardcoded static map (fetch, playwright, bash, mcp, baml) as defined in the spec

**Step serialization:** Walk the workflow AST. For each statement:
- `LetDecl` where value is `ExecuteRun` → `WorkflowStep::ToolCall`
- `LetDecl` where value is `Reason` → `WorkflowStep::Reason`
- `Statement::Reason` → `WorkflowStep::Reason`
- `ForLoop` → `WorkflowStep::ForLoop` with recursive body serialization
- `IfCond` → `WorkflowStep::IfCond`
- `TryCatch` → `WorkflowStep::TryCatch`
- `Return` → `WorkflowStep::Return`

The `bind_type` string is the declared type name from the `let` binding (e.g. `"SearchResult"`). If no explicit type annotation, use the tool's declared `output_type`.

---

### Task 5: Synth-Runner Generator (`src/codegen/synth_runner.rs` — new file)

Stage 1 also generates `generated/synth-runner.js`. This is a Node.js script that the Rust compiler spawns as a child process during Stage 2.

**Function signature:**
```rust
pub fn emit_synth_runner(document: &Document, project_root: &Path) -> CompilerResult<()>
```

The generated script must:
1. Read the synthesizer config from the artifact (passed via `--artifact` CLI arg to the script)
2. Import the correct adapter from `@claw/synth-adapters` based on the provider
3. Read newline-delimited `SynthesisRequest` JSON from stdin
4. Process requests concurrently (max 4 at a time using `Promise.all` with a semaphore)
5. Write newline-delimited `SynthesisResponse` JSON to stdout
6. Write progress to stderr (e.g., `synthesizing WebSearch... done (1.2s)`)

**Provider → adapter mapping:**
- `anthropic` → `@claw/synth-adapters/anthropic.js`
- `openai` → `@claw/synth-adapters/openai.js`
- `local` (Ollama) → `@claw/synth-adapters/ollama.js`
- `openrouter` → `@claw/synth-adapters/openrouter.js`

The Rust code builds the `SynthesisRequest` objects from the artifact and the reference implementations library (loaded from `src/synthesis/references/` at compile time via `include_str!`).

---

### Task 6: TypeScript Types Generator (`src/codegen/ts_types.rs` — new file)

Stage 2 (deterministic, no LLM) generates `generated/types.ts` from all `TypeDecl` nodes.

**Function signature:**
```rust
pub fn emit_types_ts(document: &Document, project_root: &Path) -> CompilerResult<()>
```

**Output format:**
```typescript
// generated/types.ts
// AUTO-GENERATED by claw build — do not edit

export interface Summary {
  title: string;
  body: string;
  confidence: number;  // constraints: min=0.0, max=1.0
}

export interface SearchResult {
  url: string;
  snippet: string;
  confidence: number;
}
```

Type mapping:
- `string` → `string`
- `int` → `number`
- `float` → `number`
- `boolean` → `boolean`
- `list<X>` → `X[]`
- Custom type → reference to the interface by name

Constraints become inline comments only — they are enforced by the contract tests, not by TypeScript types.

---

### Task 7: Workflow TypeScript Generator (`src/codegen/ts_workflow.rs` — new file)

Generates `generated/workflows/{WorkflowName}.ts` deterministically from the step AST. No LLM involved.

**Function signature:**
```rust
pub fn emit_workflow_ts(workflow: &WorkflowDecl, document: &Document, project_root: &Path) -> CompilerResult<()>
```

Walk the `WorkflowStep` array and emit TypeScript. Rules:

- **`ToolCall` step:** `const {bind}: {bind_type} = await {tool}({args});`
- **`Reason` step:** `const {bind}: {output_type} = await reason<{input_type}, {output_type}>({ agent: '{agent}', input: {input}, goal: '{goal}', outputType: '{output_type}' });`
- **`ForLoop` step:** `for (const {item} of {iterable}) { ... }`
- **`IfCond` step:** `if ({condition}) { ... } else { ... }`
- **`TryCatch` step:** `try { ... } catch ({catch_var}: unknown) { ... }`
- **`Return` step:** `return {value};`

String interpolation args: `${topic}` in a string arg becomes a TypeScript template literal: `` `Write a concise summary about: ${inputs.topic}` ``

Imports: collect all tool names from `ToolCall` steps, import from `'../tools/index.js'`. If any `Reason` step exists, import `reason` from `'../runtime/reason.js'`. Always import needed types from `'../types.js'`.

---

### Task 8: Contract Test Generator (`src/codegen/ts_tests.rs` — new file)

Generates `generated/__tests__/{ToolName}.contract.test.ts` for every synthesis-path tool.

**Function signature:**
```rust
pub fn emit_contract_tests(tool: &ToolDecl, document: &Document, project_root: &Path) -> CompilerResult<()>
```

**Tier 1 — Contract tests (always generated):**
One test per output field asserting the correct TypeScript type. For constrained `float` fields, also assert range. Use vitest (`describe`, `it`, `expect`, `vi`).

The mock strategy per `using:` value:
- `fetch` → `vi.mock('node-fetch', ...)` returning a minimal valid JSON response
- `playwright` → `vi.mock('playwright', ...)` — stub `chromium.launch()`, mark test with `// TIER3_ONLY`
- `bash` → `vi.mock('node:child_process', ...)` returning exit code 0
- `mcp(...)` → `vi.mock('@modelcontextprotocol/sdk/client/index.js', ...)` returning empty tool list
- `baml(...)` → `vi.mock('../baml_client/index.js', ...)` returning a minimal valid object

**Tier 2 — Behavior tests (generated only when `test {}` block declared):**
One test per `test {}` block. Map `expect` operators to vitest assertions:
- `!empty` → `expect(result.field).not.toBe('')` and `expect(result.field).not.toBeNull()`
- `> n` → `expect(result.field).toBeGreaterThan(n)`
- `< n` → `expect(result.field).toBeLessThan(n)`
- `>= n` → `expect(result.field).toBeGreaterThanOrEqual(n)`
- `<= n` → `expect(result.field).toBeLessThanOrEqual(n)`
- `== value` → `expect(result.field).toBe(value)`
- `matches "regex"` → `expect(result.field).toMatch(/regex/)`

---

### Task 9: Reason Runtime Generator (`src/codegen/ts_reason.rs` — new file)

Generates `generated/runtime/reason.ts` when any workflow contains a `reason {}` block. This is deterministic — no LLM.

The generated file implements the `reason<I, O>()` function used by workflow TypeScript. It must:
1. Look up the agent's `runtime_client` from a config object (passed at module init, loaded from artifact)
2. Build an LLM prompt using the `system_prompt`, serialized input (JSON), and goal string
3. Call the LLM using the provider's API (provider determined at runtime from config)
4. Parse the response as JSON
5. Validate against the declared `output_type` using Zod schemas (generated from `types.ts`)
6. Retry up to 3x on Zod validation failure, appending the error to the prompt

Generate a Zod schema for each declared type alongside `types.ts` in `generated/schemas.ts`.

---

### Task 10: Extend the CLI (`src/bin/claw.rs`)

Add synthesis stages to `run_compile_once`. The synthesis path is triggered when `document.synthesizers` is non-empty OR any tool has `using:`.

**Modified `run_compile_once` for `BuildLanguage::Opencode`:**

```
1. Parse + semantic analysis (unchanged)
2. If any tool has using: OR synthesizers declared:
   a. emit_artifact(document, project_root)  → generates artifact.clawa.json
   b. emit_synth_runner(document, project_root)  → generates synth-runner.js
   c. emit_types_ts(document, project_root)  → generates types.ts
   d. emit_contract_tests(per tool, document, project_root)  → generates __tests__/
   e. spawn synth-runner.js, send SynthesisRequests, collect SynthesisResponses
   f. write received TypeScript to generated/tools/*.ts
   g. emit_workflow_ts(per workflow, document, project_root)  → generates workflows/
   h. emit_reason_runtime if any reason{} blocks
   i. run vitest (npx vitest run generated/__tests__)
      → if tests fail: retry synthesis (max 3x), then hard error
      → if tests pass: continue
   j. run esbuild (per workflow, bundle to generated/bin/)
3. Also run existing OpenCode path (opencode.json, MCP server) — unchanged
```

**Synthesis invocation timeout:** Default 120s per tool. Read from `claw.json` `synthesis.timeout_ms`.

**New `claw verify` subcommand:**
```rust
Commands::Verify(VerifyArgs)
```
Runs only Tier 3 tests (real network). Fails if no `generated/` directory exists (must run `claw build` first).

---

### Task 11: Synthesis Cache (`src/codegen/cache.rs` — new file)

Cache synthesized TypeScript to avoid redundant LLM calls.

**Cache key:** `sha256(source_hash + synthesizer_model_id + reference_lib_version)`

`reference_lib_version` is a constant string embedded at compile time from `src/synthesis/references/VERSION`.

**Cache location:** `.claw-cache/synthesis/{cache_key}/{tool_name}.ts`

**Behavior:**
- Before spawning `synth-runner.js` for a tool, check cache
- Cache hit: load TypeScript from cache, still run contract tests (Stage 3)
- Cache miss: synthesize, write to cache after tests pass

**Offline handling:**
If cache miss and synthesis fails due to network error → emit: `"Synthesis cache miss for '{tool}'. Run claw build with network access, or commit generated/tools/ to version control."`

---

### Task 12: Reference Library (`src/synthesis/references/`)

Create the reference implementations directory with one file per capability primitive. These are included at compile time via `include_str!()` in the artifact emitter and injected into `SynthesisRequest.references`.

**Files to create:**

`src/synthesis/references/fetch.ts` — canonical fetch-based tool:
```typescript
// REFERENCE: canonical fetch tool implementation
// The synthesized tool SHOULD follow this pattern
import fetch from 'node-fetch';

export async function ExampleFetchTool(inputs: { query: string }): Promise<{ result: string }> {
  const url = new URL('https://api.example.com/search');
  url.searchParams.set('q', inputs.query);
  const response = await fetch(url.toString());
  if (!response.ok) throw new Error(`HTTP ${response.status}: ${response.statusText}`);
  const data = await response.json() as { result: string };
  return data;
}
```

`src/synthesis/references/playwright.ts` — canonical Playwright navigation:
```typescript
import { chromium } from 'playwright';

export async function ExamplePlaywrightTool(inputs: { url: string }): Promise<{ title: string, text: string }> {
  const browser = await chromium.launch({ headless: true });
  const page = await browser.newPage();
  await page.goto(inputs.url, { waitUntil: 'networkidle' });
  const title = await page.title();
  const text = await page.innerText('body');
  await browser.close();
  return { title, text };
}
```

`src/synthesis/references/mcp.ts` — canonical MCP client call:
```typescript
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StdioClientTransport } from '@modelcontextprotocol/sdk/client/stdio.js';

export async function ExampleMcpTool(inputs: { query: string }): Promise<{ result: string }> {
  const transport = new StdioClientTransport({ command: 'node', args: ['mcp-server.js'] });
  const client = new Client({ name: 'claw-tool', version: '1.0.0' }, { capabilities: {} });
  await client.connect(transport);
  const result = await client.callTool({ name: 'search', arguments: inputs });
  await client.close();
  return { result: JSON.stringify(result.content) };
}
```

`src/synthesis/references/bash.ts` — canonical bash execution:
```typescript
import { exec } from 'node:child_process';
import { promisify } from 'node:util';
const execAsync = promisify(exec);

export async function ExampleBashTool(inputs: { cmd: string }): Promise<{ stdout: string, exitCode: number }> {
  const { stdout, stderr } = await execAsync(inputs.cmd, { timeout: 30000 });
  return { stdout: stdout.trim(), exitCode: 0 };
}
```

`src/synthesis/references/VERSION` — version string (plain text, no newline):
```
2026-03-19-v1
```

---

## Critical Invariants — Do NOT Violate

Read these carefully. Previous Gemini implementations have violated all of them.

1. **`invoke:` tools NEVER go through the synthesis path.** They use the existing MCP path (Spec 26) unchanged. The synthesis path is triggered ONLY by `using:`.

2. **Workflow TypeScript is generated from the step AST deterministically — NO LLM call for workflows.** Only tool implementations are synthesized by the LLM.

3. **`generated/types.ts` is always emitted BEFORE tool synthesis requests are sent.** Type definitions cannot depend on synthesized tools.

4. **The `SynthesisModelAdapter` interface in `generated/synth-runner.js` must exactly match the `SynthesisRequest` / `SynthesisResponse` interfaces in Spec 33 §2.1 / §2.2.** Do not add or remove fields.

5. **Synthesis cache is keyed on `(source_hash, synthesizer_model_id, reference_lib_version)` — NOT on source_hash alone.** Changing the reference library version must invalidate the cache.

6. **`dynamic_reasoning` on agents in the artifact is DERIVED by the compiler, not declared by the user.** Never ask the user to add `dynamic_reasoning:` to their agent declaration.

7. **`reason {}` uses the agent's `client` field at RUNTIME.** The synthesizer's client is only used at build time. These are different client configs.

8. **On synthesis failure after 3 retries, write the failed artifact to `generated/tools/.failed/{ToolName}.attempt3.ts`.** Do not silently discard it.

9. **`claw test` (DSL test blocks, Rust AST interpreter) is UNCHANGED.** The new vitest tests run as part of `claw build`, not `claw test`.

10. **Tool path decision is a hard invariant:**
    - `invoke:` only → MCP path
    - `using:` only → synthesis path
    - Both → compiler error
    - Neither → compiler error

11. **Workflow generation depends on tool synthesis completing successfully.** Never emit `workflows/*.ts` with placeholder or missing tool imports. Fail the build if any tool synthesis fails.

12. **`playwright` tools are SKIPPED in Tier 1/2 vitest tests** (their tests are marked with a skip comment). They only run under `claw verify`. Do not make `claw build` launch a real browser.

---

## What NOT to Do

These are mistakes previous implementations have made:

- **Do NOT change any existing OpenCode codegen logic** (`src/codegen/opencode.rs`). The MCP server, opencode.json, and command file generation are unchanged.
- **Do NOT change the existing parser for `invoke:` tool blocks.** Only ADD new grammar for `using:`.
- **Do NOT add `synthesis_client` to the agent artifact.** Agents have `runtime_client`. Synthesizers are separate declarations.
- **Do NOT make the `synth-runner.js` call the LLM synchronously in a `for` loop.** Use `Promise.all` with concurrency limit 4.
- **Do NOT skip writing `generated/types.ts` before synthesis.** The Synthesis Pass needs the type context in the artifact — but also, the synthesized tools import from `types.ts` which must exist before tests run.
- **Do NOT hardcode `"DefaultClient"` anywhere in BAML generation** (a known prior bug — see Spec 31). The BAML default client must come from `document.clients.first().name`.
- **Do NOT use commas between fields in `.claw` test fixtures.** The parser rejects commas between block fields. All test `.claw` strings must use newline-separated fields.
- **Do NOT use single quotes in `.claw` test fixtures.** Parser only accepts double quotes.
- **Do NOT create the `synthesizer_decl` grammar rule in isolation** — it must also be added to the `document` top-level rule.

---

## Tests to Write (TDD — write these BEFORE implementing)

Follow the TDD golden rule from `specs/08-Testing-Spec.md`: write failing tests first, then implement.

### Parser tests (`src/parser_tests.rs`)

```rust
#[test]
fn test_parse_tool_with_using_fetch() {
    let src = r#"
tool WebSearch(query: string) -> SearchResult {
    using: fetch
}
"#;
    let doc = parser::parse(src).unwrap();
    assert_eq!(doc.tools[0].using, Some(UsingExpr::Fetch));
    assert!(doc.tools[0].invoke_path.is_none());
}

#[test]
fn test_parse_tool_with_using_mcp() {
    let src = r#"
tool BraveSearch(query: string) -> SearchResult {
    using: mcp("brave-search")
}
"#;
    let doc = parser::parse(src).unwrap();
    assert_eq!(doc.tools[0].using, Some(UsingExpr::Mcp("brave-search".to_string())));
}

#[test]
fn test_parse_tool_with_test_block() {
    let src = r#"
tool WebSearch(query: string) -> SearchResult {
    using: fetch
    test {
        input:  { query: "rust language" }
        expect: { url: !empty }
    }
}
"#;
    let doc = parser::parse(src).unwrap();
    assert!(doc.tools[0].test_block.is_some());
    let tb = doc.tools[0].test_block.as_ref().unwrap();
    assert_eq!(tb.expect[0].1, ExpectOp::NotEmpty);
}

#[test]
fn test_parse_synthesizer_decl() {
    let src = r#"
client MyClaude {
    provider = "anthropic"
    model = "claude-sonnet-4-6"
}
synthesizer DefaultSynth {
    client = MyClaude
    temperature = 0.1
}
"#;
    let doc = parser::parse(src).unwrap();
    assert_eq!(doc.synthesizers[0].name, "DefaultSynth");
    assert_eq!(doc.synthesizers[0].client, "MyClaude");
}

#[test]
fn test_parse_reason_stmt() {
    let src = r#"
workflow ResearchAndDecide(query: string) -> Decision {
    let raw: SearchResult = execute Searcher.run(query: query)
    reason {
        using:       Writer
        input:       raw
        goal:        "Analyze the results"
        output_type: Decision
        bind:        decision
    }
    return decision
}
"#;
    let doc = parser::parse(src).unwrap();
    let stmts = &doc.workflows[0].body.statements;
    assert!(stmts.iter().any(|s| matches!(s, Statement::Reason { .. })));
}

#[test]
fn test_parse_error_tool_with_both_invoke_and_using() {
    let src = r#"
tool WebSearch(query: string) -> SearchResult {
    invoke: module("scripts/search").function("run")
    using: fetch
}
"#;
    assert!(parser::parse(src).is_err());
}
```

### Semantic tests (`src/semantic_tests.rs`)

```rust
#[test]
fn test_semantic_error_using_tool_no_synthesizer() {
    // Build a document with using: fetch tool but no synthesizer declared
    // Assert CompilerError about missing synthesizer
}

#[test]
fn test_semantic_dynamic_reasoning_flag_derived() {
    // Build a document where Writer agent is used in a reason{} block
    // After semantic analysis, assert Writer has dynamic_reasoning = true
    // WITHOUT the user declaring it
}

#[test]
fn test_semantic_reason_undefined_agent() {
    // reason { using: GhostAgent ... } where GhostAgent not declared
    // Assert CompilerError::UndefinedAgent
}
```

### Artifact emitter tests (`src/codegen_tests.rs`)

```rust
#[test]
fn test_artifact_emitter_includes_only_using_tools() {
    // Document with one invoke: tool and one using: tool
    // Artifact tools[] must contain only the using: tool
}

#[test]
fn test_artifact_step_serialization_includes_bind_type() {
    // Workflow with let result: Summary = execute Writer.run(...)
    // Artifact step must have bind_type: "Summary"
}

#[test]
fn test_artifact_dynamic_reasoning_derived_not_declared() {
    // Agent appears in reason{} block
    // Artifact agents[].dynamic_reasoning must be true
    // without user declaring it
}
```

---

## Package Dependencies to Add

Add to `Cargo.toml`:
```toml
sha2 = "0.10"
serde_json = "1"  # already present, ensure it is
```

Add to generated project's `package.json` (via `claw init` template update):
```json
{
  "devDependencies": {
    "vitest": "^3.0.0",
    "esbuild": "^0.24.0"
  },
  "dependencies": {
    "@claw/synth-adapters": "^0.1.0",
    "node-fetch": "^3.0.0",
    "zod": "^3.0.0"
  }
}
```

---

## Verification Checklist

After implementation, verify all of these:

- [ ] `cargo test` passes with 0 failures
- [ ] `cargo clippy` passes with 0 errors (warnings OK)
- [ ] Parser test: `test_parse_tool_with_using_fetch` passes
- [ ] Parser test: `test_parse_error_tool_with_both_invoke_and_using` returns error
- [ ] Semantic test: missing synthesizer returns correct error message
- [ ] Artifact test: `bind_type` populated in tool_call steps
- [ ] Artifact test: `dynamic_reasoning` derived from usage, not declared
- [ ] Manual test: `claw build demo.claw` on a file with `using: fetch` produces `generated/artifact.clawa.json`
- [ ] Manual test: `generated/synth-runner.js` exists and is valid JS (`node --check`)
- [ ] Manual test: `generated/types.ts` exists and contains all type interfaces
- [ ] Manual test: `generated/workflows/Summarize.ts` exists, imports from `../tools/index.js`
- [ ] Manual test: `generated/__tests__/WebSearch.contract.test.ts` exists with vi.mock
- [ ] Manual test: existing `invoke:` tool project still builds without synthesis path triggering
- [ ] Manual test: `opencode.json`, `generated/mcp-server.js` still generated (MCP path unchanged)
