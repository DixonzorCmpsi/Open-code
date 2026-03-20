# Spec 26: MCP Server Generation

**Status:** ACTIVE
**Introduced:** Phase 2 migration to OpenCode as execution OS
**Depends on:** `specs/25-OpenCode-Integration.md`, `specs/03-Grammar.md`, `specs/05-Type-System.md`

---

## 0. Overview

When `clawc` compiles a `.claw` file with `--lang opencode`, it generates a single Node.js MCP (Model Context Protocol) server file at `generated/mcp-server.js`. This server:

1. Implements the MCP protocol using `@modelcontextprotocol/sdk`
2. Exposes every `tool` block from the `.claw` file as an MCP tool with typed JSON Schema
3. Exposes every `agent` block as an `agent_<Name>` runner tool that spawns `opencode -p` non-interactively
4. Handles input validation and invokes the underlying implementation (`invoke: module(...)`)
5. Is started automatically by OpenCode as a child process via the `opencode.json` `mcpServers` config

The MCP server is the **only** component that executes tool code. OpenCode routes all tool calls to it.

---

## 1. MCP Protocol Primer

MCP (Model Context Protocol) is an open standard for connecting LLMs to external tools and data sources. OpenCode supports MCP natively. A Claw-generated MCP server:

- Communicates with OpenCode via stdin/stdout (not HTTP)
- Responds to `tools/list` requests with all available tools and their schemas
- Responds to `tools/call` requests by invoking the tool handler and returning the result
- Is started as a stdio subprocess (`type: "stdio"` in `opencode.json` under `mcpServers`)

---

## 2. DSL → MCP Tool Mapping

### 2.1 Simple Tool

**Claw source:**
```claw
type SearchResult {
    url: string
    confidence_score: float
    snippet: string
    tags: list<string>
}

tool WebSearch(query: string) -> SearchResult {
    invoke: module("scripts.search").function("run")
}
```

**Generated in `mcp-server.js`:**
```javascript
// Tool registration entry for WebSearch
{
  name: "WebSearch",
  description: "WebSearch tool",
  inputSchema: {
    type: "object",
    properties: {
      query: { type: "string" }
    },
    required: ["query"]
  }
}

// Handler for WebSearch
async function handleWebSearch(args) {
  // Input is pre-validated against inputSchema by the MCP SDK
  const { query } = args;

  // Dynamic import of implementation module
  const { run } = await import(
    new URL("../scripts/search.js", import.meta.url)
  );

  const result = await run(query);

  // Output validation against SearchResult schema
  const SearchResultSchema = {
    type: "object",
    properties: {
      url: { type: "string" },
      confidence_score: { type: "number" },
      snippet: { type: "string" },
      tags: { type: "array", items: { type: "string" } }
    },
    required: ["url", "confidence_score", "snippet", "tags"]
  };

  validateOutput(result, SearchResultSchema, "WebSearch");
  return { content: [{ type: "text", text: JSON.stringify(result) }] };
}
```

### 2.2 Tool with Primitive Return

```claw
tool ReadFile(path: string) -> string {
    invoke: module("scripts.fs").function("read")
}
```

```javascript
// Primitive return — wrapped in MCP text content
async function handleReadFile(args) {
  const { path } = args;
  const { read } = await import(new URL("../scripts/fs.js", import.meta.url));
  const result = await read(path);
  if (typeof result !== "string") {
    throw new Error(`ReadFile: expected string, got ${typeof result}`);
  }
  return { content: [{ type: "text", text: result }] };
}
```

### 2.3 Tool with Multiple Parameters

```claw
tool AnalyzeSentiment(text: string, language: string, detailed: bool) -> float {
    invoke: module("scripts.nlp").function("sentiment")
}
```

```javascript
{
  name: "AnalyzeSentiment",
  description: "AnalyzeSentiment tool",
  inputSchema: {
    type: "object",
    properties: {
      text: { type: "string" },
      language: { type: "string" },
      detailed: { type: "boolean" }
    },
    required: ["text", "language", "detailed"]
  }
}
```

### 2.4 Tool with `optional<T>` Parameter

```claw
tool Search(query: string, max_results: optional<int>) -> list<SearchResult> {
    invoke: module("scripts.search").function("search")
}
```

```javascript
{
  name: "Search",
  inputSchema: {
    type: "object",
    properties: {
      query: { type: "string" },
      max_results: { type: "integer" }   // optional = not in required[]
    },
    required: ["query"]
    // max_results is NOT in required — it's optional
  }
}
```

### 2.5 Agent Runner Tool

Each `agent` block generates an `agent_<Name>` MCP tool whose handler calls the LLM provider API **directly**. It MUST NOT spawn a child `opencode` process.

**Claw source:**

```claw
agent Researcher {
    client = FastClaude          // provider = "anthropic", model = "claude-4-sonnet"
    system_prompt = "You form exact hypotheses before executing tools."
    tools = [WebSearch, ReadFile]
    settings = { max_steps: 5, temperature: 0.1 }
}
```

**Generated handler (Anthropic provider):**

```javascript
async function handleagent_Researcher(args) {
  try {
    const { task } = args;
    const Anthropic = (await import("@anthropic-ai/sdk")).default;
    const client = new Anthropic(); // reads ANTHROPIC_API_KEY from env

    const messages = [{ role: "user", content: task }];
    let steps = 0;
    const MAX_STEPS = 5; // from settings.max_steps

    while (steps < MAX_STEPS) {
      steps++;
      const response = await client.messages.create({
        model: "claude-4-sonnet",
        system: "You form exact hypotheses before executing tools.",
        messages,
        tools: [
          TOOLS.find(t => t.name === "WebSearch"),
          TOOLS.find(t => t.name === "ReadFile"),
        ].filter(Boolean),
        max_tokens: 4096,
      });

      if (response.stop_reason === "end_turn") {
        const text = response.content.find(b => b.type === "text")?.text ?? "";
        return { content: [{ type: "text", text }] };
      }

      if (response.stop_reason === "tool_use") {
        const toolUseBlocks = response.content.filter(b => b.type === "tool_use");
        messages.push({ role: "assistant", content: response.content });
        const toolResults = [];
        for (const toolUse of toolUseBlocks) {
          const handler = HANDLERS[toolUse.name];
          let resultContent;
          if (handler) {
            const r = await handler(toolUse.input);
            resultContent = r.isError
              ? `Error: ${r.content[0]?.text}`
              : r.content[0]?.text ?? "";
          } else {
            resultContent = `Error: unknown tool "${toolUse.name}"`;
          }
          toolResults.push({
            type: "tool_result",
            tool_use_id: toolUse.id,
            content: resultContent,
          });
        }
        messages.push({ role: "user", content: toolResults });
        continue;
      }

      // Unexpected stop reason
      break;
    }

    return {
      content: [{ type: "text", text: `Agent Researcher reached max_steps (${MAX_STEPS}) without finishing.` }],
      isError: true,
    };
  } catch (err) {
    return {
      content: [{ type: "text", text: `Agent Researcher failed: ${err.message}` }],
      isError: true,
    };
  }
}
```

**Generated handler (Ollama / local provider):**

When `client.provider = "local"`, replace the Anthropic SDK call with an Ollama-compatible OpenAI-format call:

```javascript
// For provider = "local", model = "local.qwen2.5:14b"
const response = await fetch("http://localhost:11434/v1/chat/completions", {
  method: "POST",
  headers: { "Content-Type": "application/json" },
  body: JSON.stringify({
    model: "qwen2.5:14b",
    messages: [{ role: "system", content: systemPrompt }, ...messages],
    tools: toolSchemas,
    stream: false,
  }),
});
const data = await response.json();
```

**Rules for agent handler generation:**

- The handler is named `handleagent_<Name>` (lowercase `agent_` prefix, camelCase name).
- `client` in the `.claw` source resolves to provider + model. The handler uses the matching SDK.
- `system_prompt` is passed as the `system` parameter (Anthropic) or as a system-role message (Ollama).
- `settings.max_steps` becomes the `MAX_STEPS` loop limit. Default: `10` if not declared.
- `settings.temperature` is passed in the API call. Default: `1.0` if not declared.
- Only the tools listed in `agent.tools` are passed to the LLM in this handler — NOT all tools.
- If `require_type` is declared on the `execute` call (resolved at workflow level), the handler validates the final text as JSON against the named schema before returning.
- The MCP tool registration entry (`TOOLS` array) includes the agent's system prompt and available tool names in the `description` field so OpenCode's coder agent knows what each agent does.

---

## 3. Type System → JSON Schema Mapping

The MCP server uses JSON Schema for all tool input and output validation. The mapping from Claw types to JSON Schema is:

| Claw Type | JSON Schema Type | Notes |
|-----------|-----------------|-------|
| `string` | `{ "type": "string" }` | |
| `int` | `{ "type": "integer" }` | |
| `float` | `{ "type": "number" }` | |
| `bool` | `{ "type": "boolean" }` | |
| `list<T>` | `{ "type": "array", "items": <T schema> }` | Recursive |
| `list<list<T>>` | `{ "type": "array", "items": { "type": "array", "items": <T schema> } }` | |
| `optional<T>` / `T?` | Same as `T` schema but NOT in `required[]` | |
| User type `Foo` | `{ "type": "object", "properties": {...}, "required": [...] }` | Flattened |
| `@regex("...")` | `{ "type": "string", "pattern": "..." }` | |
| `@min(N)` on `int`/`float` | `{ "minimum": N }` | |
| `@max(N)` on `int`/`float` | `{ "maximum": N }` | |
| `@min(N)` on `string` | `{ "minLength": N }` | |
| `@max(N)` on `string` | `{ "maxLength": N }` | |

---

## 4. Full Generated MCP Server Structure

The compiler emits a single self-contained ESM file. The full structure is:

```javascript
// generated/mcp-server.js
// AUTO-GENERATED by clawc build --lang opencode
// DO NOT EDIT — re-run clawc to regenerate

import { Server } from "@modelcontextprotocol/sdk/server/index.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import {
  CallToolRequestSchema,
  ListToolsRequestSchema,
} from "@modelcontextprotocol/sdk/types.js";
import { fileURLToPath } from "node:url";
import path from "node:path";

// ── Schema Validation ─────────────────────────────────────────────────────────

function validateOutput(value, schema, toolName) {
  // Lightweight structural validation (no external dependencies)
  if (schema.type === "object") {
    if (typeof value !== "object" || value === null) {
      throw new Error(`${toolName}: expected object, got ${typeof value}`);
    }
    for (const req of (schema.required ?? [])) {
      if (!(req in value)) {
        throw new Error(`${toolName}: missing required field "${req}"`);
      }
    }
  } else if (schema.type === "array") {
    if (!Array.isArray(value)) {
      throw new Error(`${toolName}: expected array, got ${typeof value}`);
    }
  } else if (schema.type === "string" && typeof value !== "string") {
    throw new Error(`${toolName}: expected string, got ${typeof value}`);
  } else if (schema.type === "number" && typeof value !== "number") {
    throw new Error(`${toolName}: expected number, got ${typeof value}`);
  } else if (schema.type === "integer" && !Number.isInteger(value)) {
    throw new Error(`${toolName}: expected integer, got ${value}`);
  } else if (schema.type === "boolean" && typeof value !== "boolean") {
    throw new Error(`${toolName}: expected boolean, got ${typeof value}`);
  }
}

// ── Tool Definitions ──────────────────────────────────────────────────────────

const TOOLS = [
  {
    name: "WebSearch",
    description: "WebSearch tool",
    inputSchema: {
      type: "object",
      properties: {
        query: { type: "string" }
      },
      required: ["query"]
    }
  },
  // ... one entry per `tool` block
];

// ── Type Schemas (for output validation) ─────────────────────────────────────

const SCHEMAS = {
  SearchResult: {
    type: "object",
    properties: {
      url: { type: "string" },
      confidence_score: { type: "number" },
      snippet: { type: "string" },
      tags: { type: "array", items: { type: "string" } }
    },
    required: ["url", "confidence_score", "snippet", "tags"]
  }
  // ... one entry per `type` block
};

// ── Tool Handlers ─────────────────────────────────────────────────────────────

async function handleWebSearch(args) {
  const { query } = args;

  // Path-safe module resolution (workspace-rooted)
  const modulePath = path.resolve(
    path.dirname(fileURLToPath(import.meta.url)),
    "../scripts/search.js"
  );
  const real = await import("node:fs/promises").then(fs => fs.realpath(modulePath));
  const wsRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
  const rel = path.relative(wsRoot, real);
  if (rel.startsWith("..") || path.isAbsolute(rel)) {
    throw new Error(`Tool module resolves outside workspace: scripts/search`);
  }

  const mod = await import(new URL(real, "file://").href);
  const result = await mod.run(query);
  validateOutput(result, SCHEMAS.SearchResult, "WebSearch");
  return { content: [{ type: "text", text: JSON.stringify(result) }] };
}

// ... one handler per `tool` block

// ── Handler Dispatch ──────────────────────────────────────────────────────────

const HANDLERS = {
  WebSearch: handleWebSearch,
  // ... all handlers
};

// ── MCP Server Setup ──────────────────────────────────────────────────────────

const server = new Server(
  { name: "claw-tools", version: "1.0.0" },
  { capabilities: { tools: {} } }
);

server.setRequestHandler(ListToolsRequestSchema, async () => ({
  tools: TOOLS
}));

server.setRequestHandler(CallToolRequestSchema, async (request) => {
  const { name, arguments: args } = request.params;
  const handler = HANDLERS[name];
  if (!handler) {
    throw new Error(`Unknown tool: ${name}`);
  }
  return handler(args ?? {});
});

// ── Start ─────────────────────────────────────────────────────────────────────

const transport = new StdioServerTransport();
await server.connect(transport);
```

---

## 5. Path Safety Rules (MANDATORY)

Tool module paths MUST be validated before dynamic import. The generated server enforces:

1. **Resolve to absolute path** using `path.resolve()` relative to the workspace root (parent of `generated/`)
2. **Canonicalize symlinks** using `fs.realpath()` (Node.js built-in)
3. **Containment check**: `path.relative(wsRoot, real)` must NOT start with `..` and must NOT be absolute
4. If the check fails: throw `Error("Tool module resolves outside workspace: {module}")` — do NOT proceed with the import

This prevents `invoke: module("../../etc/passwd")` path traversal attacks.

---

## 6. Error Handling in Handlers

Tool handlers MUST handle errors gracefully and return MCP-formatted error responses:

```javascript
async function handleWebSearch(args) {
  try {
    // ... handler logic
  } catch (err) {
    // MCP error response format
    return {
      content: [{ type: "text", text: `Error: ${err.message}` }],
      isError: true
    };
  }
}
```

Errors MUST NOT crash the MCP server process. Every handler is wrapped in try/catch.

---

## 7. `invoke:` Expression Resolution

The `invoke:` field in a `tool` block has this grammar:
```
invoke: module("path/to/module").function("functionName")
```

- `path/to/module` is resolved relative to the workspace root (NOT relative to `generated/`)
- File extensions are NOT included in the module path (compiler adds `.js` for ES module import)
- The function named by `functionName` is imported by name from the module
- If the module exports a default function and `functionName` is `"default"`, the default export is used
- The function is called with the tool's positional arguments in declaration order
- The function MUST be async or return a Promise; if it is synchronous, `await` is a no-op

**Example resolutions:**
| `invoke:` expression | Import path | Called as |
|---------------------|-------------|-----------|
| `module("scripts/search").function("run")` | `../scripts/search.js` | `mod.run(query)` |
| `module("src/tools/nlp").function("getSentiment")` | `../src/tools/nlp.js` | `mod.getSentiment(text)` |
| `module("tools/fs").function("default")` | `../tools/fs.js` | `mod.default(path)` |

---

## 8. Generated MCP Server: Compiler Emitter (`src/codegen/mcp.rs`)

The Rust emitter for MCP server generation:

```rust
pub struct McpOutput {
    pub server_js: String,  // Complete mcp-server.js content
}

pub fn generate_mcp(document: &Document) -> Result<McpOutput, CompilerError> {
    let tools_json = emit_tool_list(document)?;
    let schemas_json = emit_type_schemas(document)?;
    let handlers = emit_handlers(document)?;
    let dispatch = emit_dispatch_map(document)?;

    Ok(McpOutput {
        server_js: format!(TEMPLATE,
            tools = tools_json,
            schemas = schemas_json,
            handlers = handlers,
            dispatch = dispatch,
        )
    })
}
```

**Key functions:**
- `emit_tool_list(document)` → JSON array of MCP tool descriptors
- `emit_type_schemas(document)` → JSON object of type schemas for output validation
- `emit_handlers(document)` → One async function per `tool` block
- `emit_dispatch_map(document)` → `HANDLERS` object mapping tool name → handler

**Template approach:** Direct string building (NOT minijinja), same pattern as `src/codegen/baml.rs`. The MCP server uses template literals that would conflict with minijinja delimiters.

---

## 9. Testing Requirements (TDD)

All tests follow the 7-step TDD cycle from `specs/08-Testing-Spec.md`.

### 9.1 Compiler Unit Tests (Rust)

```rust
#[test]
fn test_emit_tool_with_primitive_input() {
    // Input: tool WebSearch(query: string) -> SearchResult
    // Assert: inputSchema has "query" as string, required: ["query"]
}

#[test]
fn test_emit_tool_with_optional_param() {
    // Input: tool Search(q: string, limit: optional<int>) -> list<string>
    // Assert: "limit" NOT in required[], "q" IS in required[]
}

#[test]
fn test_emit_type_schema_with_constraints() {
    // Input: type User { email: string @regex("..."), age: int @min(18) }
    // Assert: schema has pattern and minimum fields
}

#[test]
fn test_emit_handler_path_resolution() {
    // Input: invoke: module("scripts/search").function("run")
    // Assert: generated path is "../scripts/search.js"
}

#[test]
fn test_path_traversal_rejected() {
    // Input: invoke: module("../../etc/passwd").function("read")
    // Assert: compiler emits CompilerError::InvalidToolPath with span
}

#[test]
fn test_empty_tool_list_emits_valid_server() {
    // Input: .claw file with no tool blocks
    // Assert: generated mcp-server.js is syntactically valid, TOOLS = []
}
```

### 9.2 MCP Server Integration Tests (Node.js)

```javascript
// generated/mcp-server.test.js
import assert from "node:assert/strict";
import { test } from "node:test";

test("ListTools returns all declared tools", async () => {
  // Start MCP server in-process, send ListToolsRequest
  // Assert: response.tools.length === N (number of tool blocks)
});

test("CallTool validates input schema before handler", async () => {
  // Call WebSearch with missing 'query' argument
  // Assert: isError: true, message includes "required"
});

test("CallTool returns validated output", async () => {
  // Call WebSearch with valid query (mock module)
  // Assert: content[0].text is valid JSON matching SearchResult schema
});

test("CallTool rejects path traversal module", async () => {
  // If a handler somehow has a bad path (defense in depth)
  // Assert: isError: true, message includes "outside workspace"
});

test("Handler errors are returned as MCP error response, not crash", async () => {
  // Mock module throws an error
  // Assert: isError: true, server process still running
});
```

---

## 10. Dependency

The generated `mcp-server.js` requires `@modelcontextprotocol/sdk` at runtime. This is:

- Listed in the project's `package.json` as a `dependency` (not devDependency)
- Installed automatically by `npm install` in the generated project
- `claw init` adds it to the scaffolded `package.json`

```json
{
  "dependencies": {
    "@modelcontextprotocol/sdk": "^1.0.0"
  }
}
```

**Version pinning:** The MCP SDK is at v1.x and stable. A caret range is acceptable. If MCP protocol breaking changes occur, `clawc` version is bumped to match and `claw init` scaffolds the correct version.

---

## 11. File Placement

```
{project-root}/
  generated/
    mcp-server.js        ← generated by clawc build --lang opencode
    claw/
      index.ts           ← TypeScript SDK (from specs/06)
      __init__.py        ← Python SDK (from specs/06)
    claw-context.md      ← project context doc (from specs/25 §4)
  opencode.json          ← OpenCode config (from specs/25)
  .opencode/
    agents/
      Researcher.md      ← per-agent (from specs/25)
    commands/
      AnalyzeCompetitors.md  ← per-workflow (from specs/25)
```

`generated/mcp-server.js` MUST be added to `.gitignore` alongside other generated files. It is rebuilt by `clawc build` on every change to the `.claw` source.
