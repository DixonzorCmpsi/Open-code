# Claw DSL: SDK Generation

Once the `.claw` code is parsed into an AST and validated by the Semantic Analyzer, the `clawc` compiler moves to Phase 3: Code Generation. This phase outputs the `.claw` workflows into standard, strictly-typed SDK files for use in the developer's application.

## 0. Terminology

This spec distinguishes two separate components:

| Term | Location | Author | Purpose |
|------|----------|--------|---------|
| **Generated SDK** | `generated/claw/index.ts` or `generated/claw/__init__.py` | Output of `clawc build` | Type-safe wrapper functions with Zod/Pydantic validation |
| **Client Library** | `packages/openclaw-sdk/` or `python-sdk/openclaw_sdk/` | Hand-written | HTTP/WebSocket transport to the Gateway |

The **Generated SDK** imports the **Client Library**. The developer imports the Generated SDK. The Client Library is a low-level transport layer and does NOT perform schema validation — that is the Generated SDK's responsibility.

## 1. Generation Engine

The code generation will use `minijinja` (a Rust Jinja implementation). 
* The AST nodes and TypeBox schemas are injected into templated strings representing standard TypeScript and Python boilerplate.
* The output is written to a `generated/ claw` directory in the user's workspace.

## 2. Emitting TypeScript SDK Code

For a `.claw` file containing the `AnalyzeCompetitors` workflow, `clawc` will generate standard TypeScript interfaces and async functions.

**Original `.claw`:**
```claw
workflow AnalyzeCompetitors(company: string) -> SearchResult { ... }
```

**Generated `claw/index.ts`:**
```typescript
import { ClawClient, AgentExecutionError } from "@claw/sdk";

// 1. The emitted Zod schemas (runtime validation at the SDK boundary)
import { z } from "zod";

export const SearchResultSchema = z.object({
    url: z.string(),
    confidence_score: z.number(),
    snippet: z.string(),
    tags: z.array(z.string()),
});
export type SearchResult = z.infer<typeof SearchResultSchema>;

// 2. The emitted Workflow Function
export const AnalyzeCompetitors = async (
    company: string,
    options: { client: ClawClient, resumeSessionId?: string }
): Promise<SearchResult> => {

    // The emitted function communicates with the Heavy Backend Gateway
    // to manage the actual agent execution loop or resume from a crash.
    const result = await options.client.executeWorkflow({
        workflowName: "AnalyzeCompetitors",
        arguments: { company },
        resumeSessionId: options.resumeSessionId
    });

    // 3. Runtime boundary validation — Zod .parse() throws ZodError if
    // the gateway response doesn't match the schema (no unsafe `as` casts).
    return SearchResultSchema.parse(result);
}
```

## 3. Emitting Python SDK Code

The identical process applies for Python, generating `Pydantic` models instead of TypeScript interfaces.

**Original `.claw`:**
```claw
workflow AnalyzeCompetitors(company: string) -> SearchResult { ... }
```

**Generated `claw/__init__.py`:**
```python
from pydantic import BaseModel
from claw_sdk import ClawClient
from typing import List

# 1. The emitted Pydantic Models
class SearchResult(BaseModel):
    url: str
    confidence_score: float
    snippet: str
    tags: List[str]

# 2. The emitted Workflow Function
async def analyze_competitors(company: str, client: ClawClient) -> SearchResult:
    # 3. Call the heavy Gateway for execution
    result_dict = await client.execute_workflow(
        workflow_name="AnalyzeCompetitors", 
        arguments={"company": company}
    )
    
    # 4. Enforce Pydantic validation on the result
    return SearchResult(**result_dict)
```

## 4. The OpenCode Communication Contract

When the generated SDK executes, it invokes OpenCode (the execution OS) via its CLI or API. OpenCode handles all LLM orchestration, tool execution (via the MCP server), and session management.

The SDK is a lightweight typed wrapper; the complex task of spinning up browser automation, managing tool sandboxing, and LLM provider routing is handled by OpenCode + the generated MCP server.

See `specs/25-OpenCode-Integration.md` for the full execution contract.

---

## 5. Target: OpenCode Configuration (`clawc build --lang opencode`)

In addition to TypeScript and Python SDKs, `clawc` emits OpenCode-native configuration files that allow workflows to run interactively inside the OpenCode terminal/IDE.

**Emitted files:**

| File | Purpose |
|------|---------|
| `opencode.json` | OpenCode provider, MCP, and agent config |
| `.opencode/agents/{Name}.md` | One per `agent` block |
| `.opencode/commands/{Name}.md` | One per `workflow` block |
| `generated/mcp-server.js` | MCP server for all `tool` blocks |
| `generated/claw-context.md` | Project context document |

The full mapping from `.claw` constructs to OpenCode config is defined in `specs/25-OpenCode-Integration.md`. The MCP server generation spec is in `specs/26-MCP-Server-Generation.md`.

**Usage:**
```bash
clawc build --lang opencode example.claw
opencode  # launches OpenCode with full Claw context
```

---

## 6. CodeGen Emitter Summary

| Target | Emitter | Output |
|--------|---------|--------|
| `--lang ts` | `src/codegen/typescript.rs` | `generated/claw/index.ts` (Zod schemas + async functions) |
| `--lang python` | `src/codegen/python.rs` | `generated/claw/__init__.py` (Pydantic models + async functions) |
| `--lang opencode` | `src/codegen/opencode.rs` + `src/codegen/mcp.rs` + `src/codegen/test_runner.rs` | `opencode.json`, `.opencode/agents/`, `.opencode/commands/`, `generated/mcp-server.js`, `generated/claw-context.md`, `generated/claw-test-runner.js` |
| `--lang baml` | `src/codegen/baml.rs` | `generated/baml_src/` (BAML project files) |

The `opencode` target runs all four sub-emitters in sequence:
1. `emit_opencode_json()` — provider/MCP config
2. `emit_agent_markdowns()` — `.opencode/agents/*.md`
3. `emit_command_markdowns()` — `.opencode/commands/*.md`
4. `emit_mcp_server()` — `generated/mcp-server.js`
5. `emit_context_md()` — `generated/claw-context.md`
6. `emit_test_runner()` — `generated/claw-test-runner.js` (only when `test`/`mock` blocks exist)
