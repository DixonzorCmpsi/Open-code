# Gemini Prompt: Full Test Suite for Claw DSL Compiler

## Context

You are implementing a comprehensive test suite for the **Claw DSL compiler** (`clawc`), a Rust project at the root of this repo. The compiler parses `.claw` files and generates OpenCode config + MCP server JS.

**Current state:**
- Compiler builds and runs: `cargo build --bin claw`
- E2E smoke test exists: `test_e2e.sh`
- Unit test snapshots exist in `src/snapshots/`
- Ollama is running locally at `http://localhost:11434` with `qwen2.5-coder:7b`
- `.env` has `LOCAL_ENDPOINT=http://localhost:11434` and `CLAW_LOCAL_MODEL=local.qwen2.5-coder:7b`

**Your job:** implement a complete test suite across 4 layers. Do not change any existing source files except to add `#[cfg(test)]` modules. Write all tests so `cargo test` passes.

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

### 1.4 Error Cases — Parse Errors
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

### 3.1 opencode.json Generation
```
test_codegen_opencode_json
```
Compile the full document (§1.1). Read `opencode.json` from tempdir. Assert:
- `config["agents"]["coder"]["model"] == "local.qwen2.5-coder:7b"`
- `config["mcpServers"]["claw-tools"]["type"] == "stdio"`
- `config["mcpServers"]["claw-tools"]["command"] == "node"`
- `config["mcpServers"]["claw-tools"]["args"] == ["generated/mcp-server.js"]`
- `config["contextPaths"]` is an array containing `"generated/claw-context.md"`

### 3.2 opencode.json Merge Strategy
```
test_codegen_opencode_json_merge
```
Pre-write an `opencode.json` with custom user keys:
```json
{
  "theme": "dark",
  "keybindings": { "submit": "ctrl+enter" },
  "agents": { "coder": { "model": "old-model", "temperature": 0.7 } }
}
```
Run codegen. Assert:
- `config["theme"] == "dark"` (user key preserved)
- `config["keybindings"]["submit"] == "ctrl+enter"` (user key preserved)
- `config["agents"]["coder"]["model"] == "local.qwen2.5-coder:7b"` (overwritten)
- `config["agents"]["coder"]["temperature"] == 0.7` (user sub-key preserved)
- `config["mcpServers"]["claw-tools"]["type"] == "stdio"` (written fresh)

### 3.3 Workflow Command File
```
test_codegen_workflow_command_file
```
Compile the full document. Read `.opencode/commands/FindInfo.md`. Assert:
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

### 3.5 Context Document
```
test_codegen_context_document
```
Read `generated/claw-context.md`. Assert:
- Contains `SearchResult`
- Contains `Researcher`
- Contains `FindInfo`
- Contains `local.qwen2.5-coder:7b`

### 3.6 No Agent Markdown Files
```
test_codegen_no_agent_markdown_files
```
Assert `.opencode/agents/` directory does NOT exist in the output. OpenCode does not support custom agent markdown files — agents are MCP runner tools only.

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
2. Write `example.claw` with `local.qwen2.5-coder:7b`
3. Run `claw build example.claw`
4. `npm install` to install `@modelcontextprotocol/sdk`
5. Start MCP server: `node generated/mcp-server.js`
6. Run: `opencode /FindInfo "quantum computing" -q` with `LOCAL_ENDPOINT=http://localhost:11434`
7. Assert exit code 0 and stdout contains JSON-like output with `url`, `snippet`, `confidence_score` fields

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
# Run all unit + integration tests
cargo test

# Run only codegen tests
cargo test codegen

# Run only CLI tests
cargo test cli

# Run live Ollama tests (requires Ollama running)
cargo test -- --ignored

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

1. **`opencode.json` keys** — always `agents.coder.model`, `mcpServers` (not `mcp`), `contextPaths` (not `instructions`)
2. **MCP server type** — always `type: "stdio"` in mcpServers config
3. **No `--model` flag** — `opencode` CLI has no `--model` flag; agent runner uses only `opencode -p <task> -q`
4. **No `.opencode/agents/`** — this directory must NOT be created; agents are MCP runner tools only
5. **Command variables** — workflow parameters in `.md` files are `$UPPERCASE_NAME` not `$arguments`
6. **Merge strategy** — codegen must preserve user-set keys in `opencode.json` when rebuilding
