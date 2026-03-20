# Implementation Prompt: Spec 34 — Advanced Tool Patterns

**Project:** Claw DSL compiler (`clawc`) in Rust — `/Users/dixon.zor/Documents/Open-code`
**Specs to implement:** `specs/34-Advanced-Tool-Patterns.md` (with GAN fixes from `specs/34-GAN-Audit.md`)
**Prerequisite:** All 76 tests pass on `main`. Run `INSTA_UPDATE=always ~/.cargo/bin/cargo test` to verify before starting.

---

## What you are implementing

Three advanced tool-calling patterns added to the Claw DSL:

1. **Tool Registry** — `registry {}` declaration + `tools = RegistryName` on agents. Deferred MCP tool loading via auto-generated `tool_search` / `tool_load` tools. BM25 index pre-built at compile time.
2. **Sandbox Primitive** — `sandbox {}` declaration + `using: sandbox(Name)` on tools. Generates a two-file TypeScript artifact (wrapper + sandbox script) and a Tool Bridge HTTP server with auth token.
3. **Multi-shot Examples** — `examples {}` block + `description:` property on tools. Goes into artifact, synthesis prompt, MCP schema, and generated JSDoc.

---

## Existing codebase orientation

Read these files FIRST before writing any code:

- `src/ast.rs` — all AST node definitions
- `src/parser.rs` — winnow 0.7 parser (use `verify_map` not `try_map`, no single-element `alt()`)
- `src/semantic/mod.rs` — symbol table, duplicate detection
- `src/semantic/types.rs` — statement/expression type checking (add `Statement::Reason` arm to every match — already done)
- `src/codegen/mod.rs` — codegen module registry, `document_ast_hash`
- `src/codegen/opencode.rs` — generates `opencode.json` + `.opencode/command/*.md`
- `src/codegen/mcp.rs` — generates `generated/mcp-server.js`
- `src/codegen/artifact.rs` — generates `generated/artifact.clawa.json`
- `src/bin/claw.rs` — CLI, `run_compile_once` wires all codegen steps
- `specs/34-Advanced-Tool-Patterns.md` — the full spec with all GAN fixes
- `specs/32-Code-Synthesis-Pipeline.md` — artifact format reference
- `specs/33-Synthesis-Model-Interface.md` — SynthesisRequest interface reference

---

## Implementation order

Work in this exact sequence. Run `cargo test` after each task group. Do NOT batch.

---

### Task 1: AST changes (`src/ast.rs`)

**1a. Add `RegistryDecl`:**

```rust
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RegistryDecl {
    pub name: String,
    pub tools: Vec<String>,
    pub search: RegistrySearch,
    pub max_results: Option<u32>,
    pub min_relevance: Option<f64>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum RegistrySearch {
    Semantic,  // deferred loading — generates tool_search
    All,       // eager loading — same as tools = [list]
}
```

**1b. Add `SandboxDecl`:**

```rust
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SandboxDecl {
    pub name: String,
    pub runtime: SandboxRuntime,
    pub network: SandboxNetwork,
    pub bridge_tools: Vec<String>,
    pub timeout_ms: Option<u32>,
    pub node_version: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum SandboxRuntime { Gvisor, Docker, Subprocess }

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum SandboxNetwork { BridgeOnly, None }
```

**1c. Add `UsingExpr::Sandbox(String)` variant** to existing enum.

**1d. Add `ExampleBlock` and `ExampleEntry`:**

```rust
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExampleBlock {
    pub entries: Vec<ExampleEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExampleEntry {
    pub input:  Vec<(String, SpannedExpr)>,
    pub output: Vec<(String, SpannedExpr)>,
}
```

**1e. Update `ToolDecl`** — add two fields:

```rust
pub description: Option<String>,
pub examples: Option<ExampleBlock>,
```

**1f. Update `AgentDecl.tools`** — change type:

```rust
// BEFORE:
pub tools: Vec<String>,

// AFTER:
pub tools: AgentTools,

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum AgentTools {
    List(Vec<String>),
    Registry(String),
}
```

**1g. Update `Document`** — add two fields:

```rust
pub registries: Vec<RegistryDecl>,
pub sandboxes:  Vec<SandboxDecl>,
```

**After 1g:** Run `cargo test`. Fix all struct literal missing-field errors in test code (add `description: None, examples: None` to ToolDecl fixtures; `tools: AgentTools::List(vec![...])` to AgentDecl fixtures; `registries: Vec::new(), sandboxes: Vec::new()` to Document fixtures). Update the insta snapshot with `INSTA_UPDATE=always cargo test`.

---

### Task 2: Parser changes (`src/parser.rs`)

Read the existing parser patterns (especially `tool_property_parser`, `agent_property`, `using_expr`, `synthesizer_decl`) before adding any parser.

**2a. Add `ToolProperty` variants:**

```rust
enum ToolProperty {
    Invoke(String),
    Using(UsingExpr),
    Synthesizer(String),
    Test(TestBlock),
    Description(String),   // NEW
    Examples(ExampleBlock), // NEW
}
```

**2b. Extend `tool_property_parser`** — add two branches:

- `"description"` → `preceded(lexeme(':'), string_literal).map(ToolProperty::Description)`
- `"examples"` → `brace_delimited(repeat(1.., example_entry)).map(ToolProperty::Examples)`

**2c. Add `example_entry` parser:**

An `example_entry` parses one `{ input: { k: v, ... }, output: { k: v, ... } }` inside the examples block. Values must be scalar literals only — reject non-scalar at parse time using `verify_map`.

```rust
fn example_entry(input: &mut Input<'_>) -> PResult<ExampleEntry> {
    // parse brace-delimited block with "input" and "output" keys
    // each value is a brace-delimited k:v map where v is a scalar literal
}

fn scalar_literal(input: &mut Input<'_>) -> PResult<SpannedExpr> {
    // string_literal | int_literal | float_literal | bool_literal
    // rejects identifiers, arrays-of-objects, etc.
}
```

**2d. Extend `using_expr`** — add `sandbox(Name)` variant:

```rust
"sandbox" => preceded(
    paren_delimited(lexeme(simple_identifier_raw))
).map(UsingExpr::Sandbox)
```

**2e. Add `registry_decl` parser:**

```rust
fn registry_decl(input: &mut Input<'_>) -> PResult<RegistryDecl> {
    // "registry" <Name> { tools = [...], search = semantic|all,
    //   max_results = <int>?, min_relevance = <float>? }
}
```

**2f. Add `sandbox_decl` parser:**

```rust
fn sandbox_decl(input: &mut Input<'_>) -> PResult<SandboxDecl> {
    // "sandbox" <Name> { runtime = gvisor|docker|subprocess,
    //   network = bridge_only|none, bridge_tools = [...],
    //   timeout_ms = <int>?, node_version = <string>? }
}
```

**2g. Update `agent_property` parser** for `tools`:

The `tools` property currently parses `= [list]`. Extend to also accept `= IdentifierName` (no brackets) → `AgentTools::Registry(name)`.

```rust
"tools" => alt((
    bracket_delimited(comma_separated0(simple_identifier_raw))
        .map(AgentTools::List),
    lexeme(simple_identifier_raw)
        .map(AgentTools::Registry),
))
```

**2h. Wire new declarations into `document()` parser** — add arms to the `Declaration` enum:

```rust
Declaration::Registry(RegistryDecl)
Declaration::Sandbox(SandboxDecl)
```

And push to `document.registries` / `document.sandboxes` in the folding map.

**After Task 2:** Run `cargo test`. Fix parser test issues. Update snapshot if needed.

---

### Task 3: Semantic validation (`src/semantic/mod.rs` and `src/semantic/types.rs`)

**3a. Symbol table** — add registries and sandboxes to `SymbolTable`:

```rust
pub struct SymbolTable {
    // existing...
    pub registries: HashMap<String, RegistryDecl>,
    pub sandboxes:  HashMap<String, SandboxDecl>,
}
```

**3b. Duplicate detection** — add `DuplicateDeclaration` checks for registry names and sandbox names (same pattern as existing tool/agent/client checks).

**3c. Registry validation** (`validate_registry`):
- `E-R01 UndefinedRegistry`: agent's `tools = RegistryName` where registry not declared
- `E-R02 UndefinedToolInRegistry`: tool listed in `registry.tools` not declared
- `W-R01 RegistryToolNoDescription`: tool in a `search: semantic` registry has no `description:` → emit compiler warning (not error)

**3d. Sandbox validation** (`validate_sandbox`):
- `E-S01 UndefinedSandbox`: `using: sandbox(Name)` where Name not declared
- `E-S02 UndefinedBridgeTool`: bridge_tool name not in declared tools
- `E-S03 SandboxNetworkConflict`: `network: none` with non-empty `bridge_tools`
- `E-S04 BridgeToolNotSynthesized`: bridge_tool has `invoke:` but not `using:`

**3e. Agent tools validation** — update existing agent tool check to handle `AgentTools`:

```rust
match &agent.tools {
    AgentTools::List(names) => { /* existing per-name validation */ }
    AgentTools::Registry(name) => {
        if !symbols.registries.contains_key(name) {
            errors.push(UndefinedRegistry { name, span });
        }
        // validate each tool in the registry exists
    }
}
```

**3f. Example validation** (`validate_examples`):
- `E-E01 UnknownExampleField`: example output key not in tool's output type fields
- `E-E02 MissingExampleInput`: example input missing a declared tool argument
- `E-E03 NestedObjectInExample`: example value is not a scalar literal (should be caught at parse time too, but double-check here)
- `W-E01 TooManyExamples`: tool has > 10 examples
- `W-E02 DescriptionTooLong`: `description:` > 500 characters
- `W-S02 AgentUsedInReasonHasSandboxTool`: `reason {}` block's agent has a sandbox tool

**3g. Add new error variants to `CompilerError`** in `src/errors.rs`:

```rust
UndefinedRegistry { name: String, span: Span },
UndefinedToolInRegistry { registry: String, tool: String, span: Span },
UndefinedSandbox { name: String, span: Span },
UndefinedBridgeTool { sandbox: String, tool: String, span: Span },
SandboxNetworkConflict { sandbox: String, span: Span },
BridgeToolNotSynthesized { sandbox: String, tool: String, span: Span },
UnknownExampleField { tool: String, field: String, span: Span },
MissingExampleInput { tool: String, field: String, span: Span },
```

**After Task 3:** Run `cargo test`.

---

### Task 4: Artifact codegen (`src/codegen/artifact.rs`)

Update `build_artifact` to include all new sections:

**4a. Tool additions:**

```rust
fn emit_tool(t: &ToolDecl) -> Value {
    let mut obj = json!({
        "name":        t.name,
        "inputs":      t.arguments.iter().map(emit_field).collect::<Vec<_>>(),
        "output_type": t.return_type.as_ref().map(type_name).unwrap_or("void"),
        "using":       t.using.as_ref().map(using_expr_str).unwrap_or_default(),
    });
    if let Some(desc) = &t.description {
        obj["description"] = json!(desc);
    }
    if let Some(ex) = &t.examples {
        obj["examples"] = emit_example_block(ex);
    }
    if let Some(tb) = &t.test_block {
        obj["tests"] = emit_test_block(tb, &t.arguments);
    }
    obj
}

fn emit_example_block(eb: &ExampleBlock) -> Value {
    Value::Array(eb.entries.iter().map(|e| {
        json!({
            "input":  e.input.iter().map(|(k,v)| (k.clone(), expr_to_json(v))).collect::<serde_json::Map<_,_>>(),
            "output": e.output.iter().map(|(k,v)| (k.clone(), expr_to_json(v))).collect::<serde_json::Map<_,_>>(),
        })
    }).collect())
}
```

**4b. Add registries section:**

```rust
fn emit_registry(r: &RegistryDecl, tools: &[ToolDecl]) -> Value {
    let index: Vec<Value> = r.tools.iter().filter_map(|name| {
        tools.iter().find(|t| &t.name == name).map(|t| {
            json!({
                "name":          t.name,
                "description":   t.description.clone().unwrap_or_else(|| auto_description(t)),
                "input_summary": input_summary(&t.arguments),
            })
        })
    }).collect();

    json!({
        "name":          r.name,
        "search":        if r.search == RegistrySearch::Semantic { "semantic" } else { "all" },
        "max_results":   r.max_results.unwrap_or(5),
        "min_relevance": r.min_relevance.unwrap_or(0.1),
        "tools":         r.tools,
        "index":         index,
    })
}

fn auto_description(t: &ToolDecl) -> String {
    format!("{}({}) -> {}",
        t.name,
        t.arguments.iter().map(|a| format!("{}: {}", a.name, type_name(&a.data_type))).collect::<Vec<_>>().join(", "),
        t.return_type.as_ref().map(type_name).unwrap_or_else(|| "void".to_owned()),
    )
}
```

**4c. Add sandboxes section:**

```rust
fn emit_sandbox(s: &SandboxDecl) -> Value {
    json!({
        "name":         s.name,
        "runtime":      match s.runtime { SandboxRuntime::Gvisor => "gvisor", SandboxRuntime::Docker => "docker", SandboxRuntime::Subprocess => "subprocess" },
        "network":      match s.network { SandboxNetwork::BridgeOnly => "bridge_only", SandboxNetwork::None => "none" },
        "bridge_tools": s.bridge_tools,
        "timeout_ms":   s.timeout_ms.unwrap_or(30000),
        "node_version": s.node_version.as_deref().unwrap_or("22"),
    })
}
```

**4d. Update `build_artifact` root:**

```rust
json!({
    "manifest":     { ... },
    "types":        ...,
    "tools":        ...,
    "agents":       ...,
    "workflows":    ...,
    "synthesizers": ...,
    "registries":   document.registries.iter().map(|r| emit_registry(r, &document.tools)).collect::<Vec<_>>(),
    "sandboxes":    document.sandboxes.iter().map(emit_sandbox).collect::<Vec<_>>(),
    "capability_registry": capability_registry(),
})
```

---

### Task 5: MCP server codegen (`src/codegen/mcp.rs`)

Read the existing `mcp.rs` fully before modifying.

**5a. Add `tool_search` and `tool_load` for semantic registries:**

When `document.registries` contains any entry with `search: Semantic`, the generated MCP server MUST:

1. NOT register registry tools at startup.
2. Register `tool_search` as a regular MCP tool:

```typescript
server.tool('tool_search', {
  query: { type: 'string' },
  max_results: { type: 'number', optional: true },
}, async ({ query, max_results = 5 }) => {
  const results = searchIndex(REGISTRY_INDEX, query, max_results, MIN_RELEVANCE);
  return { content: [{ type: 'text', text: JSON.stringify(results) }] };
});
```

3. Register `tool_load` as an MCP tool that dynamically returns schema only (NOT implementation):

```typescript
server.tool('tool_load', {
  name: { type: 'string' },
}, async ({ name }) => {
  const def = TOOL_SCHEMAS[name];
  if (!def) return { content: [{ type: 'text', text: JSON.stringify({ error: `unknown tool: ${name}` }) }] };
  return { content: [{ type: 'text', text: JSON.stringify(def) }] };
});
```

**5b. Generate the BM25 index at compile time:**

In `mcp.rs`, emit the pre-built index as a JS constant inside the generated server. This is a simple TF-IDF/BM25 approximation over the tool descriptions from the registry's `index[]` in the artifact:

```javascript
const REGISTRY_INDEX = [
  { name: 'WebSearch',  description: 'Searches the web...', input_summary: 'query: string', tokens: ['search', 'web', 'query', ...] },
  // ...
];

function searchIndex(index, query, maxResults, minRelevance) {
  const queryTokens = query.toLowerCase().split(/\s+/);
  const scored = index.map(tool => {
    const score = queryTokens.filter(t => tool.tokens.includes(t)).length / queryTokens.length;
    return { ...tool, score };
  }).filter(t => t.score >= minRelevance)
    .sort((a, b) => b.score - a.score)
    .slice(0, maxResults);
  return scored.map(({ name, description, input_summary, score }) => ({ name, description, input_summary, score }));
}
```

Tokenization: lowercase, split on non-alphanumeric, deduplicate. Computed at compile time from tool names + descriptions + input summaries.

**5c. `TOOL_SCHEMAS` constant** — for `tool_load`:

```javascript
const TOOL_SCHEMAS = {
  WebSearch: {
    name: 'WebSearch',
    description: 'Searches the web for a query string.',
    inputSchema: { type: 'object', properties: { query: { type: 'string' } }, required: ['query'] },
  },
  // one entry per tool in any semantic registry
};
```

**5d. MCP tool description injection (examples):**

When emitting a tool's MCP schema, if it has `description:` and `examples {}`, inject examples into the description field — first 3 examples max, 80 chars per example line, 500 char hard cap for the examples section. Format:

```
<description>\n\nExamples:\n  Input: {...} → Output: {...}
```

---

### Task 6: Sandbox codegen (`src/codegen/sandbox.rs` — new file)

Create `src/codegen/sandbox.rs`. This generates three files:

**6a. `generated/runtime/bridge-server.ts`:**

```typescript
// generated/runtime/bridge-server.ts — auto-generated. Do not edit.
import { createServer, IncomingMessage, Server, ServerResponse } from 'node:http';
import { randomBytes } from 'node:crypto';
{{ for each bridge_tool in all_bridge_tools }}
import { {{ tool_name }} } from '../tools/{{ tool_name }}.js';
{{ end }}

const BRIDGE_TOOLS: Record<string, (args: unknown) => Promise<unknown>> = {
{{ for each bridge_tool }}
  {{ tool_name }}: (args) => {{ tool_name }}(args as any),
{{ end }}
};

async function readBody(req: IncomingMessage): Promise<string> {
  return new Promise((res, rej) => {
    let data = '';
    req.on('data', (chunk) => { data += chunk; });
    req.on('end', () => res(data));
    req.on('error', rej);
  });
}

export function startBridge(token: string): Promise<{ url: string; server: Server }> {
  return new Promise((resolve) => {
    const server = createServer(async (req: IncomingMessage, res: ServerResponse) => {
      const auth = req.headers['authorization'];
      if (auth !== `Bearer ${token}`) {
        res.writeHead(401);
        res.end(JSON.stringify({ error: 'unauthorized' }));
        return;
      }
      const toolName = req.url?.split('/call/')[1];
      if (!toolName || !BRIDGE_TOOLS[toolName]) {
        res.writeHead(404);
        res.end(JSON.stringify({ error: `unknown tool: ${toolName}` }));
        return;
      }
      try {
        const body = await readBody(req);
        const args = JSON.parse(body);
        const result = await BRIDGE_TOOLS[toolName](args);
        res.writeHead(200, { 'Content-Type': 'application/json' });
        res.end(JSON.stringify(result));
      } catch (err: any) {
        res.writeHead(500);
        res.end(JSON.stringify({ error: err.message }));
      }
    });
    server.listen(0, '127.0.0.1', () => {
      const addr = server.address() as { port: number };
      resolve({ url: `http://127.0.0.1:${addr.port}`, server });
    });
  });
}

export async function stopBridge(server: Server): Promise<void> {
  return new Promise((res, rej) => server.close((err) => err ? rej(err) : res()));
}
```

Collect all bridge_tools across ALL sandboxes in the document. Emit one import per unique tool.

**6b. `generated/runtime/sandbox.ts`:**

```typescript
// generated/runtime/sandbox.ts — auto-generated. Do not edit.
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { randomBytes } from 'node:crypto';
import { startBridge, stopBridge } from './bridge-server.js';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const execFileAsync = promisify(execFile);
const __dirname = dirname(fileURLToPath(import.meta.url));

export interface SandboxConfig {
  runtime:      'gvisor' | 'docker' | 'subprocess';
  network:      'bridge_only' | 'none';
  timeout_ms:   number;
  node_version: string;
}

export async function runSandbox<I, O>(
  config: SandboxConfig,
  inputs: I,
  scriptPath: string,
): Promise<O> {
  const token = randomBytes(32).toString('hex');
  const { url: bridgeUrl, server } = await startBridge(token);

  try {
    const inputB64 = Buffer.from(JSON.stringify(inputs)).toString('base64');
    const env = {
      ...process.env,
      CLAW_INPUT:        inputB64,
      CLAW_BRIDGE_URL:   bridgeUrl,
      CLAW_BRIDGE_TOKEN: token,
    };

    let stdout: string;

    if (config.runtime === 'subprocess') {
      const result = await execFileAsync('node', [scriptPath], {
        env,
        timeout: config.timeout_ms,
        maxBuffer: 10 * 1024 * 1024,
      });
      stdout = result.stdout;
    } else if (config.runtime === 'docker') {
      const result = await execFileAsync('docker', [
        'run', '--rm', '--network', 'host',
        '-e', `CLAW_INPUT=${inputB64}`,
        '-e', `CLAW_BRIDGE_URL=${bridgeUrl}`,
        '-e', `CLAW_BRIDGE_TOKEN=${token}`,
        `node:${config.node_version}-slim`,
        'node', scriptPath,
      ], { timeout: config.timeout_ms, maxBuffer: 10 * 1024 * 1024 });
      stdout = result.stdout;
    } else {
      // gvisor
      const result = await execFileAsync('docker', [
        'run', '--rm', '--runtime', 'runsc',
        '--network', 'host',
        '-e', `CLAW_INPUT=${inputB64}`,
        '-e', `CLAW_BRIDGE_URL=${bridgeUrl}`,
        '-e', `CLAW_BRIDGE_TOKEN=${token}`,
        `node:${config.node_version}-slim`,
        'node', scriptPath,
      ], { timeout: config.timeout_ms, maxBuffer: 10 * 1024 * 1024 });
      stdout = result.stdout;
    }

    return JSON.parse(stdout) as O;
  } finally {
    await stopBridge(server);
  }
}
```

**6c. Generated tool wrapper for `using: sandbox(...)`** — handled by the synthesis pass (the `SandboxContext` in `SynthesisRequest` tells the pass to generate a two-file artifact). However, the Rust compiler needs to emit a STUB wrapper at compile time so the TypeScript project compiles. Emit a stub to `generated/tools/<ToolName>.ts`:

```typescript
// generated/tools/ProcessExpenses.ts
// SANDBOX STUB — replaced after synthesis
import { runSandbox } from '../runtime/sandbox.js';
import type { Employee, ExpenseReport } from '../types.js';

const SANDBOX_CONFIG = {
  runtime:      'subprocess' as const,
  network:      'bridge_only' as const,
  timeout_ms:   60000,
  node_version: '22',
};

export async function ProcessExpenses(inputs: { employees: Employee[] }): Promise<ExpenseReport> {
  return runSandbox(SANDBOX_CONFIG, inputs, new URL('./ProcessExpenses.script.js', import.meta.url).pathname);
}
```

The config values come from the `SandboxDecl` in the document.

**6d. Wire into `src/codegen/mod.rs`** — add `pub mod sandbox;` and `pub fn generate_sandbox(...)`.

**6e. Wire into `src/bin/claw.rs`** — call `codegen::generate_sandbox(&document, project_root)` in `run_compile_once` for `BuildLanguage::Opencode`.

---

### Task 7: `description:` + `examples {}` in opencode codegen and ts_tests

**7a. `src/codegen/opencode.rs`** — update `generate_workflow_command` to handle `AgentTools`:

```rust
// Before: a.tools.join(", ")
// After:
let tools_str = match &a.tools {
    AgentTools::List(names) => names.join(", "),
    AgentTools::Registry(name) => format!("registry:{}", name),
};
```

**7b. `src/codegen/ts_tests.rs`** — inject examples as additional `it()` blocks:

For each example entry in `tool.examples`, emit an `it('example:<n>', ...)` test:

```typescript
it('example:0', async () => {
  const result = await ParseDate({ text: "next Tuesday" });
  expect(result.year).toBe(2026);
  expect(result.month).toBe(3);
  expect(result.day).toBe(24);
});
```

**7c. `src/codegen/artifact.rs`** — `description:` already emitted in Task 4. Also update `emit_agent` to emit `tools` correctly:

```rust
fn emit_agent(a: &AgentDecl) -> Value {
    let tools_val = match &a.tools {
        AgentTools::List(names) => json!(names),
        AgentTools::Registry(name) => json!({ "registry": name }),
    };
    json!({
        "name":             a.name,
        "system_prompt":    a.system_prompt,
        "tools":            tools_val,
        "dynamic_reasoning": a.dynamic_reasoning.get(),
    })
}
```

---

### Task 8: `document_ast_hash` and `write_document` updates

**8a. `src/codegen/mod.rs`** — `write_document` currently skips registries and sandboxes. Add them to the canonical hash:

```rust
fn write_document(output: &mut String, document: &Document) {
    write_seq(output, "imports",     &document.imports,     write_import);
    write_seq(output, "types",       &document.types,       write_type_decl);
    write_seq(output, "clients",     &document.clients,     write_client_decl);
    write_seq(output, "tools",       &document.tools,       write_tool_decl);
    write_seq(output, "agents",      &document.agents,      write_agent_decl);
    write_seq(output, "workflows",   &document.workflows,   write_workflow_decl);
    write_seq(output, "listeners",   &document.listeners,   write_listener_decl);
    write_seq(output, "tests",       &document.tests,       write_test_decl);
    write_seq(output, "mocks",       &document.mocks,       write_mock_decl);
    write_seq(output, "registries",  &document.registries,  write_registry_decl);  // NEW
    write_seq(output, "sandboxes",   &document.sandboxes,   write_sandbox_decl);   // NEW
}
```

Add `write_registry_decl` and `write_sandbox_decl` using the existing pattern of `write_tag` + `write_string` + `write_seq`.

**8b. `write_tool_decl`** — add `description` and `examples` to the canonical representation:

```rust
fn write_tool_decl(output: &mut String, declaration: &ToolDecl) {
    // ... existing fields ...
    write_option_string(output, "description", declaration.description.as_deref());
    // examples: write each entry's input/output key-value pairs
}
```

---

### Task 9: Test coverage

**9a. Parser tests** — add to `src/parser.rs` test module:

```rust
#[test]
fn parses_registry_decl() {
    let source = r#"
registry MyReg {
    tools = [WebSearch, Screenshot]
    search = semantic
    max_results = 3
}
"#;
    let doc = parse(source).expect("parse");
    assert_eq!(doc.registries.len(), 1);
    assert_eq!(doc.registries[0].name, "MyReg");
    assert_eq!(doc.registries[0].tools, vec!["WebSearch", "Screenshot"]);
    assert!(matches!(doc.registries[0].search, RegistrySearch::Semantic));
    assert_eq!(doc.registries[0].max_results, Some(3));
}

#[test]
fn parses_sandbox_decl() {
    let source = r#"
sandbox MySandbox {
    runtime      = gvisor
    network      = bridge_only
    bridge_tools = [ExpenseAPI]
    timeout_ms   = 45000
}
"#;
    let doc = parse(source).expect("parse");
    assert_eq!(doc.sandboxes.len(), 1);
    assert_eq!(doc.sandboxes[0].name, "MySandbox");
    assert!(matches!(doc.sandboxes[0].runtime, SandboxRuntime::Gvisor));
    assert_eq!(doc.sandboxes[0].bridge_tools, vec!["ExpenseAPI"]);
}

#[test]
fn parses_tool_description_and_examples() {
    let source = r#"
tool ParseDate(text: string) -> string {
    using: fetch
    description: "Parse natural language date"
    examples {
        { input: { text: "next Tuesday" }, output: { day: 24 } }
    }
}
"#;
    let doc = parse(source).expect("parse");
    assert_eq!(doc.tools[0].description.as_deref(), Some("Parse natural language date"));
    assert!(doc.tools[0].examples.is_some());
}

#[test]
fn parses_agent_with_registry_ref() {
    let source = r#"
agent Researcher {
    client = MyClaude
    tools  = MyReg
}
"#;
    let doc = parse(source).expect("parse");
    assert!(matches!(&doc.agents[0].tools, AgentTools::Registry(n) if n == "MyReg"));
}

#[test]
fn parses_using_sandbox() {
    let source = r#"
tool ProcessData(data: string) -> string {
    using: sandbox(MySandbox)
}
"#;
    let doc = parse(source).expect("parse");
    assert!(matches!(&doc.tools[0].using, Some(UsingExpr::Sandbox(n)) if n == "MySandbox"));
}
```

**9b. Semantic tests** — add to `src/semantic/mod.rs` test module:

```rust
#[test]
fn rejects_undefined_registry_in_agent() { ... }

#[test]
fn rejects_undefined_tool_in_registry() { ... }

#[test]
fn rejects_undefined_sandbox_in_using() { ... }

#[test]
fn rejects_bridge_tool_not_synthesized() { ... }

#[test]
fn rejects_sandbox_network_conflict() { ... }
```

---

### Task 10: Final verification

```bash
# All tests pass
INSTA_UPDATE=always ~/.cargo/bin/cargo test

# Binary builds
~/.cargo/bin/cargo build --bin claw

# End-to-end: compile a .claw file with registry, sandbox, examples
# Use this test fixture:
cat > /tmp/test34.claw << 'EOF'
type SearchResult {
    url:     string
    snippet: string
}

type ExpenseReport {
    total_spend: float
    line_count:  int
}

type Employee {
    id:   int
    name: string
}

client MyClaude {
    provider = "anthropic"
    model    = "claude-sonnet-4-6"
}

synthesizer DefaultSynth {
    client      = MyClaude
    temperature = 0.1
}

sandbox LocalSandbox {
    runtime      = subprocess
    network      = bridge_only
    bridge_tools = [WebSearch]
    timeout_ms   = 30000
}

registry ToolRegistry {
    tools         = [WebSearch, ProcessExpenses]
    search        = semantic
    max_results   = 3
    min_relevance = 0.1
}

tool WebSearch(query: string) -> SearchResult {
    using:       fetch
    synthesizer: DefaultSynth
    description: "Searches the web for a query string. Returns URL and snippet."
    examples {
        { input: { query: "rust programming" }, output: { url: "https://www.rust-lang.org", snippet: "Fast, safe systems language" } }
    }
    test {
        input:  { query: "rust language" }
        expect: { url: !empty, snippet: !empty }
    }
}

tool ProcessExpenses(employees: Employee[]) -> ExpenseReport {
    using:       sandbox(LocalSandbox)
    synthesizer: DefaultSynth
    description: "Processes expense reports for a list of employees in bulk."
}

agent Researcher {
    client        = MyClaude
    tools         = ToolRegistry
    system_prompt = "Search for tools with tool_search before calling them."
    settings      = { max_steps: 5, temperature: 0.1 }
}

workflow FindInfo(topic: string) -> SearchResult {
    let result: SearchResult = execute Researcher.run(task: "Find info about: ${topic}")
    return result
}
EOF

cd /tmp && ~/.cargo/bin/claw build test34.claw

# Verify these files exist:
ls /tmp/generated/artifact.clawa.json
ls /tmp/generated/synth-runner.js
ls /tmp/generated/types.ts
ls /tmp/generated/workflows/FindInfo.ts
ls /tmp/generated/runtime/bridge-server.ts
ls /tmp/generated/runtime/sandbox.ts
ls /tmp/generated/tools/ProcessExpenses.ts    # sandbox stub
ls /tmp/generated/registry/ToolRegistry.index.json  # if generated as separate file

# Verify artifact content
node -e "const a = require('/tmp/generated/artifact.clawa.json'); console.log(JSON.stringify({ registries: a.registries?.length, sandboxes: a.sandboxes?.length, tool_examples: a.tools?.[0]?.examples?.length }));"
```

---

## Invariants — never violate these

1. **All tests must pass after every task group.** Run `INSTA_UPDATE=always ~/.cargo/bin/cargo test` after each task. Fix all failures before moving on.
2. **`try_map` is never used in parser.rs** — always use `verify_map`. Single-element `alt()` is never used — remove the wrapper.
3. **`AgentTools::List` must be the default** for all existing test fixtures. Replace `tools: vec!["X"]` with `tools: AgentTools::List(vec!["X".to_owned()])` everywhere in test code.
4. **All match statements on `Statement`** must include `Statement::Reason { .. }`. All match statements on `AgentTools` must cover both arms.
5. **The Sandbox stub wrapper** (Task 6c) is generated at compile time. The synthesis pass REPLACES it with the real implementation. The Rust compiler never synthesizes — it only emits stubs and the artifact.
6. **Bridge server never touches credentials** — API keys are on the host, inside tool implementations. The bridge only sees tool names + args.
7. **`tool_load` returns schema only** — never returns `using:`, source code, or synthesis metadata.
8. **`description:` max 500 chars** — enforced as a warning `W-E02`, not an error.
9. **BM25 index is pre-built at compile time** — no runtime embedding API, no HTTP call from `tool_search`. All computation happens inside the generated JavaScript constant.
10. **Backward compat**: `tools = [List]` still works. `invoke:` still works. No existing `.claw` files break.
