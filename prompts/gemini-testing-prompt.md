# Gemini Prompt: Full Test Suite for Claw DSL Compiler

## Context

You are implementing a comprehensive test suite for the **Claw DSL compiler** (`clawc`), a Rust project at the root of this repo. The compiler parses `.claw` files and generates OpenCode config + MCP server JS.

**Current state:**
- Compiler builds and runs: `~/.cargo/bin/cargo build --bin claw`
- E2E smoke test exists: `test_e2e.sh`
- Unit test snapshots exist in `src/snapshots/`
- Ollama is running locally at `http://localhost:11434` with `qwen2.5-coder:7b`
- `.env` has `LOCAL_ENDPOINT=http://localhost:11434` and `CLAW_LOCAL_MODEL=local.qwen2.5-coder:7b`

**Your job:** implement a complete test suite across 4 layers. Do not change any existing source files except to add `#[cfg(test)]` modules. Write all tests so `cargo test` passes.

---

## CRITICAL: Schema Reference (OpenCode v1.2.27)

**Do not guess the schema — use exactly these keys.** They were reverse-engineered from the installed binary and confirmed working end-to-end.

### Correct `opencode.json` structure (v1.2.27):
```json
{
  "model": "ollama/qwen2.5-coder:7b",
  "provider": {
    "ollama": {
      "api": "http://localhost:11434/v1",
      "models": {
        "qwen2.5-coder:7b": {}
      }
    }
  },
  "mcp": {
    "claw-tools": {
      "type": "local",
      "command": ["/opt/homebrew/bin/node", "generated/mcp-server.js"]
    }
  },
  "instructions": [
    "AGENTS.md",
    "generated/claw-context.md"
  ]
}
```

### WRONG keys that will cause `"Unrecognized keys"` errors:
| Wrong (old schema) | Correct (v1.2.27) |
|---|---|
| `"mcpServers"` | `"mcp"` |
| `"type": "stdio"` | `"type": "local"` |
| `"command": "node", "args": [...]` | `"command": ["/full/path/node", "generated/mcp-server.js"]` |
| `"contextPaths"` | `"instructions"` |
| `"agents": { "coder": { "model": ... } }` | `"model"` (top-level key) |
| `"providers"` (plural) | `"provider"` (singular) |

### Local model mapping:
- DSL: `model = "local.qwen2.5-coder:7b"` → JSON: `"model": "ollama/qwen2.5-coder:7b"` + `provider.ollama.api` + `provider.ollama.models`
- DSL: `model = "claude-4-sonnet"` → JSON: `"model": "claude-4-sonnet"` (no provider block)

### Commands directory:
- **`.opencode/command/`** (singular) — NOT `.opencode/commands/` (plural)

### Node binary path:
- MCP `command` array must use absolute node path, e.g. `/opt/homebrew/bin/node`
- OpenCode spawns MCP with a restricted PATH — bare `"node"` will fail with "Executable not found"

### OpenCode CLI:
- `opencode -p <task> -q` — run non-interactively
- **NO `--model` flag** — OpenCode CLI has no `--model` argument

---

## CRITICAL: BAML Integration Architecture

`invoke: baml(...)` tools are distinct from `invoke: module(...)` tools. They involve THREE components that must all work together:

### 1. `src/codegen/baml.rs` — generates BAML source files
- `collect_baml_tools(document)` filters tools where `invoke_path.starts_with("baml(")`
- `generate_baml(document)` returns `BamlOutput { generators, clients, types, functions }`
- The client name in emitted BAML functions comes from `document.clients.first().name`, NOT hardcoded `"DefaultClient"`

### 2. `src/bin/claw.rs` — build pipeline calls `generate_baml()` conditionally
```rust
// In run_compile_once(), BuildLanguage::Opencode branch:
let baml_output = codegen::generate_baml(&document)?;
if !baml_output.functions.is_empty() {
    // write generated/baml_src/generators.baml, clients.baml, types.baml, functions.baml
    println!("  BAML tools detected. Run: npx @boundaryml/baml-cli generate --from generated/baml_src");
}
```
**If you do not call `generate_baml()` here, no BAML files will ever be written.**

### 3. `src/codegen/mcp.rs` — `emit_handler()` must branch on `baml(...)`
```rust
fn emit_handler(tool: &ToolDecl, _document: &Document) -> String {
    let invoke_path = tool.invoke_path.as_deref().unwrap_or("scripts/stub");
    if invoke_path.starts_with("baml(") {
        return emit_baml_handler(tool);  // imports ../baml_client/index.js
    }
    // ... module(...) handler
}
```
A `baml(...)` tool that falls through to the module handler generates a broken stub that tries to import `../scripts/stub.js` — runtime error.

### BAML MCP handler pattern (emit_baml_handler):
```js
async function handleExtractKeywords(args) {
  try {
    const { text } = args;
    const { b } = await import("../baml_client/index.js");
    const result = await b.ExtractKeywords({ text });
    const schema = SCHEMAS.KeywordList;
    if (schema) { validateOutput(result, schema, "ExtractKeywords"); }
    return { content: [{ type: "text", text: JSON.stringify(result) }] };
  } catch (err) {
    return { content: [{ type: "text", text: `Error: ${err.message}` }], isError: true };
  }
}
```

### BAML build flow:
```
claw build → generated/baml_src/{generators,clients,types,functions}.baml
npx @boundaryml/baml-cli generate --from generated/baml_src → generated/baml_client/
```

---

## Layer 1: Parser Unit Tests

File: `src/parser_tests.rs` (new file, `#[cfg(test)]` module, include via `mod parser_tests;` in `lib.rs`)

Test the `parser::parse()` function with inline `.claw` source strings. Cover:

### 1.1 Happy Path — Full Document
```
test_parse_full_document
```
Input:
```
type SearchResult {
    url: string
    snippet: string
    confidence_score: float
}

tool WebSearch(query: string) -> SearchResult {
    invoke: module("scripts/search").function("run")
}

client LocalQwen {
    provider = "local"
    model = "local.qwen2.5-coder:7b"
}

agent Researcher {
    client = LocalQwen
    system_prompt = "You are a precise researcher."
    tools = [WebSearch]
    settings = {
        max_steps: 5,
        temperature: 0.1
    }
}

workflow FindInfo(topic: string) -> SearchResult {
    let result: SearchResult = execute Researcher.run(
        task: "Find info about: ${topic}",
        require_type: SearchResult
    )
    return result
}
```
Assert:
- `document.types.len() == 1` with name `"SearchResult"` and 3 fields
- `document.tools.len() == 1` with name `"WebSearch"`
- `document.clients.len() == 1` with model `"local.qwen2.5-coder:7b"`
- `document.agents.len() == 1` with name `"Researcher"`, `tools == ["WebSearch"]`
- `document.workflows.len() == 1` with name `"FindInfo"`, one argument `"topic"`

### 1.2 Primitive Types
```
test_parse_all_primitive_types
```
Input: a `type AllTypes` with fields: `a: string`, `b: int`, `c: float`, `d: boolean`, `e: list<string>`
Assert all 5 fields parse with correct `DataType` variants.

### 1.3 String Interpolation
```
test_parse_string_interpolation
```
Input: a workflow that has a `let` statement with `"Hello ${name}"` in a string argument.
Assert the string value contains `${name}` (parser preserves the raw string; codegen transforms it).

### 1.4 BAML Invoke Syntax
```
test_parse_baml_invoke
```
Input:
```
tool ExtractKeywords(text: string) -> KeywordList {
    invoke: baml("ExtractKeywords")
}
```
Assert `document.tools[0].invoke_path == Some("baml(\"ExtractKeywords\")")`.

### 1.5 Error Cases — Parse Errors
```
test_parse_error_missing_brace
test_parse_error_unknown_primitive
test_parse_error_empty_tools_list
```
For each, call `parser::parse()` and assert `Err(CompilerError::ParseError { .. })`.

---

## Layer 2: Semantic Analysis Unit Tests

File: `src/semantic_tests.rs` (new, included via `mod semantic_tests;` in `lib.rs`)

Call `parser::parse()` then `semantic::analyze()`. Cover:

### 2.1 Valid Document
```
test_semantic_valid_document
```
Use the full document from Layer 1 §1.1. Assert `Ok(())`.

### 2.2 Undefined Tool Reference
```
test_semantic_undefined_tool
```
Agent references `tools = [NonExistentTool]`. Assert `Err(CompilerError::UndefinedTool { .. })`.

### 2.3 Undefined Agent in Workflow
```
test_semantic_undefined_agent
```
Workflow calls `execute GhostAgent.run(...)` where `GhostAgent` is not declared. Assert `Err(CompilerError::UndefinedAgent { .. })`.

### 2.4 Undefined Client
```
test_semantic_undefined_client
```
Agent declares `client = MissingClient`. Assert `Err(CompilerError::UndefinedClient { .. })`.

### 2.5 Duplicate Declaration
```
test_semantic_duplicate_type
```
Two `type Foo { }` blocks. Assert `Err(CompilerError::DuplicateDeclaration { .. })`.

### 2.6 Type Mismatch (if type checking is implemented)
```
test_semantic_type_mismatch_skipped
```
If type mismatch checking is not yet implemented, mark this test `#[ignore]` with a comment: `// TODO: type mismatch not yet enforced`.

---

## Layer 3: Codegen Integration Tests

File: `src/codegen_tests.rs` (new, included via `mod codegen_tests;` in `lib.rs`)

Parse + analyze + codegen to a `tempdir`. Use the `tempfile` crate (add to `[dev-dependencies]` in `Cargo.toml` if not present).

### 3.1 opencode.json Generation — v1.2.27 Schema
```
test_codegen_opencode_json
```
Compile the full document (§1.1, local model). Read `opencode.json` from tempdir. Assert using the **correct v1.2.27 schema**:
- `config["model"]` is `"ollama/qwen2.5-coder:7b"` (NOT inside `agents.coder`)
- `config["provider"]["ollama"]["api"]` is `"http://localhost:11434/v1"`
- `config["provider"]["ollama"]["models"]["qwen2.5-coder:7b"]` exists
- `config["mcp"]["claw-tools"]["type"]` is `"local"` (NOT `"stdio"`)
- `config["mcp"]["claw-tools"]["command"]` is an array (NOT a string)
- `config["instructions"]` is an array containing `"generated/claw-context.md"`

**Do NOT assert `"mcpServers"`, `"contextPaths"`, or `"agents.coder.model"` — these keys do not exist in v1.2.27.**

### 3.2 opencode.json Merge Strategy
```
test_codegen_opencode_json_merge
```
Pre-write an `opencode.json` with custom user keys:
```json
{
  "theme": "dark",
  "keybindings": { "submit": "ctrl+enter" },
  "model": "old-model"
}
```
Run codegen. Assert:
- `config["theme"] == "dark"` (user key preserved)
- `config["keybindings"]["submit"] == "ctrl+enter"` (user key preserved)
- `config["model"] == "ollama/qwen2.5-coder:7b"` (Claw-managed key overwritten)
- `config["mcp"]["claw-tools"]["type"] == "local"` (written fresh)

### 3.3 Workflow Command File
```
test_codegen_workflow_command_file
```
Compile the full document. Read `.opencode/command/FindInfo.md` (singular `command`, not `commands`). Assert:
- Contains `$TOPIC` (uppercase parameter variable)
- Contains `agent_Researcher` (MCP tool name for agent)
- Does NOT contain `$topic` (lowercase — wrong format)
- Does NOT contain `$arguments` (old wrong format)

### 3.4 MCP Server Generation
```
test_codegen_mcp_server_js
```
Compile the full document. Read `generated/mcp-server.js`. Assert:
- Contains `"WebSearch"` (tool name in TOOLS array)
- Contains `"agent_Researcher"` (agent runner tool name)
- Contains `type: "object"` (JSON Schema)
- Contains `validateOutput` (output validation function)
- Contains `opencode -p` (agent runner execSync command)
- Does NOT contain `--model` (confirmed no --model flag in OpenCode CLI)
- Does NOT contain `"scripts/stub"` for the WebSearch handler (module path should be `"scripts/search"`)

### 3.5 Context Document
```
test_codegen_context_document
```
Read `generated/claw-context.md`. Assert:
- Contains `SearchResult`
- Contains `Researcher`
- Contains `FindInfo`

### 3.6 No Agent Markdown Files
```
test_codegen_no_agent_markdown_files
```
Assert `.opencode/agents/` directory does NOT exist in the output. OpenCode does not support custom agent markdown files — agents are MCP runner tools only.

### 3.7 BAML Tool — No BAML Files When No BAML Tools
```
test_codegen_no_baml_files_for_module_tools
```
Compile a document with only `invoke: module(...)` tools. Assert `generated/baml_src/` does NOT exist.

### 3.8 BAML Tool — Files Generated When BAML Tools Present
```
test_codegen_baml_files_generated
```
Compile a document with:
```
type KeywordList {
    keywords: list<string>
}

client LocalQwen {
    provider = "local"
    model = "local.qwen2.5-coder:7b"
}

tool ExtractKeywords(text: string) -> KeywordList {
    invoke: baml("ExtractKeywords")
}
```
Assert:
- `generated/baml_src/functions.baml` exists and contains `function ExtractKeywords`
- `generated/baml_src/types.baml` exists and contains `class KeywordList`
- `generated/baml_src/clients.baml` exists and contains `client<llm> LocalQwen`
- `generated/baml_src/functions.baml` contains `client LocalQwen` (NOT `client DefaultClient`)
- `generated/mcp-server.js` contains `baml_client/index.js` (BAML handler, not stub)
- `generated/mcp-server.js` does NOT contain `scripts/stub` for the ExtractKeywords handler

---

## Layer 4: CLI Integration Tests

File: `tests/cli_integration.rs` (Rust integration test, `tests/` directory)

These tests build the binary and invoke it as a subprocess. Add `assert_cmd` and `tempfile` to `[dev-dependencies]`.

### 4.1 `claw init` scaffolding
```
test_cli_init_creates_expected_files
```
Run `claw init` in a tempdir. Assert exit code 0. Assert these files exist:
- `example.claw`
- `claw.json`
- `package.json`
- `scripts/search.js`

### 4.2 `claw build` on example.claw
```
test_cli_build_example_claw
```
Create a tempdir. Write the full `.claw` document (§1.1). Run `claw build example.claw`. Assert exit code 0. Assert `generated/mcp-server.js` and `opencode.json` exist.

### 4.3 `claw build` parse error exits with code 1
```
test_cli_build_parse_error_exit_code
```
Write a `.claw` file with a syntax error. Assert exit code 1. Assert stderr contains `"error:"`.

### 4.4 `claw build` semantic error exits with code 2
```
test_cli_build_semantic_error_exit_code
```
Write a `.claw` file with `tools = [MissingTool]`. Assert exit code 2.

### 4.5 `claw build --watch` starts without crashing
```
test_cli_build_watch_starts
```
Spawn `claw build example.claw --watch` with a 2 second timeout. Assert the process starts (does not immediately exit with error). Kill after initial build completes.

---

## Layer 5: MCP Server Smoke Test

File: `tests/mcp_smoke_test.sh` (bash, run via `cargo test` using `std::process::Command` in `tests/mcp_smoke_test.rs`)

### 5.1 MCP Server Starts
After `claw build`, run:
```bash
node generated/mcp-server.js &
sleep 1
kill %1
```
Assert node exits without error (syntax OK and MCP SDK loads).

### 5.2 MCP Server Lists Tools
Send a JSON-RPC `tools/list` request over stdin. Assert response contains `"WebSearch"` and `"agent_Researcher"`.

Use the MCP SDK test client pattern:
```js
// test_mcp.mjs
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";

const transport = new StdioClientTransport({
  command: "node",
  args: ["generated/mcp-server.js"]
});
const client = new Client({ name: "test", version: "1.0.0" });
await client.connect(transport);
const { tools } = await client.listTools();
console.log(JSON.stringify(tools.map(t => t.name)));
await client.close();
```

Run from `tests/mcp_smoke_test.rs` using `std::process::Command`. Assert output contains `"WebSearch"` and `"agent_Researcher"`.

---

## Layer 6: Live Ollama Workflow Test (Manual / `--ignored`)

File: `tests/live_ollama_test.rs`

Mark all tests with `#[ignore]` — run explicitly with `cargo test -- --ignored`.

### 6.1 Ollama Connectivity
```
test_ollama_is_running
```
HTTP GET `http://localhost:11434/v1/models`. Assert response contains `"qwen2.5-coder:7b"`.

### 6.2 OpenCode + Claw End-to-End
```
test_live_e2e_with_local_qwen
```
Prerequisite: `opencode` installed in PATH, Ollama running.

Steps:
1. Create tempdir
2. Write `example.claw` with `model = "local.qwen2.5-coder:7b"`
3. Run `claw build example.claw`
4. `npm install` to install `@modelcontextprotocol/sdk`
5. Start MCP server: `node generated/mcp-server.js`
6. Run: `opencode -p "Find info about quantum computing" -q` (NOT `opencode /FindInfo` — slash syntax is TUI-only)
7. Assert exit code 0

---

## Cargo.toml Dev Dependencies to Add

```toml
[dev-dependencies]
insta = "1.46.3"     # already present
tempfile = "3"
assert_cmd = "2"
predicates = "3"
reqwest = { version = "0.12", features = ["blocking", "json"] }
```

---

## Test Runner Commands

```bash
# Run all unit + integration tests (use full cargo path — cargo may not be in PATH)
~/.cargo/bin/cargo test

# Run only codegen tests
~/.cargo/bin/cargo test codegen

# Run only CLI tests
~/.cargo/bin/cargo test cli

# Run live Ollama tests (requires Ollama running)
~/.cargo/bin/cargo test -- --ignored

# Run e2e smoke test
bash test_e2e.sh
```

---

## Success Criteria

All of the following must pass:

| Check | Command |
|-------|---------|
| All unit tests | `cargo test` → 0 failures |
| CLI tests | `cargo test cli` → 0 failures |
| E2E smoke | `bash test_e2e.sh` → `✓ E2E Smoke Test Passed!` |
| MCP starts | `node generated/mcp-server.js` → no crash, lists tools |
| Ollama live | `curl http://localhost:11434/v1/models` → contains `qwen2.5-coder:7b` |

---

## Key Invariants to Enforce

These were confirmed via GAN audit and live testing against OpenCode v1.2.27:

1. **`opencode.json` top-level `"model"` key** — model lives at root, NOT inside `agents.coder.model`
2. **`"mcp"` not `"mcpServers"`** — v1.2.27 uses `"mcp"` as the key
3. **`"type": "local"` not `"stdio"`** — v1.2.27 MCP server type is `"local"`
4. **`"command"` is an array** — `["full/path/to/node", "generated/mcp-server.js"]`, not a string + args
5. **`"instructions"` not `"contextPaths"`** — v1.2.27 uses `"instructions"` for context file paths
6. **`"provider"` singular** — v1.2.27 uses `"provider"` not `"providers"`
7. **`.opencode/command/` singular** — NOT `.opencode/commands/`
8. **Node absolute path** — `find_node_binary()` checks `/opt/homebrew/bin/node` etc.; bare `"node"` fails
9. **No `--model` flag** — `opencode -p <task> -q`; there is no `--model` CLI argument
10. **No `.opencode/agents/`** — agents are MCP runner tools only; no markdown files
11. **BAML: three wiring points** — `baml.rs` (file generation) + `claw.rs` (call in pipeline) + `mcp.rs` (baml handler branch) — all three must exist or BAML tools are silently broken
12. **BAML client name** — emitted BAML functions use `document.clients.first().name`, not `"DefaultClient"`
13. **Merge strategy** — codegen must preserve user-set keys in `opencode.json` when rebuilding
