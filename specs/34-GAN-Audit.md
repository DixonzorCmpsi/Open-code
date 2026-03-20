# Spec 34 GAN Audit Log

---

## Round 1: Structural Integrity

### Maker Pass — What is solid

1. **Registry backward compat** — `tools = [List]` still works. New path is `tools = RegistryName`. No breaking change.
2. **Sandbox isolation model** — Tool Bridge on localhost-only, no direct internet, credentials never enter container. Security perimeter is clear.
3. **Examples vs test{}** — Distinction is clean: `test {}` = build-time validator, `examples {}` = specification/grounding data. Different lifecycles.
4. **Error code table** — All new error conditions are named and scoped.
5. **Artifact additions** — All three features add clearly-delimited sections to the artifact JSON.

### Breaker Pass — Gaps found

**B1-01: `description:` is an unparented property**
Spec §5 introduces `description:` as a tool property but §4 (DSL Grammar for tool declaration) only lists `using:`, `examples {}`, `test {}`. The parser section in Spec 32 §4.1 doesn't mention `description:` at all. There's no grammar production for it in either spec. This will cause the parser to reject `description: "..."` in a tool body.

**B1-02: Registry `search: all` produces no generated code — but it's still a registry**
Spec §2.5 says `search: all` compiles to the same opencode output as an explicit tool list. But then: what is the `tools` field of the `AgentDecl` AST when it references a registry? The AST `AgentDecl.tools: Vec<String>` currently holds tool names directly. A registry reference is an identifier, not a list. The spec doesn't define the AST node for `tools = RegistryName` vs `tools = [List]`.

**B1-03: Sandbox synthesis prompt is undefined**
Spec §3.3 describes *what* the synthesis pass generates for sandbox tools (wrapper + script), but the SynthesisRequest format (Spec 33 §2.1) doesn't include sandbox configuration. The synthesis pass would not know about the bridge URL, the bridge tools, or the runtime. There's no `sandbox_context` field in `SynthesisRequest`.

**B1-04: `tool_search` uses embeddings but no embedding source is defined**
Spec §2.3 says `tool_search` uses embedding similarity, with BM25 fallback. But it doesn't define WHERE the embedding computation runs. At build time? At query time? If at query time, it needs access to an embedding API — but the agent may be running locally with `client = OllamaLocal`. This creates a runtime dependency not declared in the `.claw` file.

**B1-05: Sandbox script `node:fetch` — not available in all Node.js versions**
The generated sandbox script (§3.3) uses `fetch()` globally. Global `fetch` is only available in Node 18+. If the user's container image uses `node:16-slim`, this silently fails. The spec says nothing about the minimum Node version for sandbox scripts.

**B1-06: `examples {}` output values — how deeply nested?**
Spec §4.2 shows flat `{ year: 2026, month: 3, day: 24 }` output. But what if the output type has nested custom types? E.g., `output: { result: { url: "https://...", snippet: "..." } }`. The grammar shows only `<field>: <value>` which implies only literal scalars. No nested object literals. This needs to be explicit.

---

### Round 1 Fixes

**Fix B1-01: Add `description:` to tool property grammar**
Add to Spec 34 §4 and cross-reference to Spec 32 §4.1. The `description:` property is a string literal, parsed as a `ToolProperty::Description(String)` variant. It appears in the tool body alongside `using:`, `invoke:`, `test {}`, `examples {}`.

**Fix B1-02: Define AST for tools-as-registry-ref**
`AgentDecl.tools` becomes `AgentTools` enum:
```rust
pub enum AgentTools {
    List(Vec<String>),           // tools = [A, B, C]
    Registry(String),            // tools = RegistryName
}
```
The semantic analyzer resolves the registry to its tool list for validation. The opencode codegen expands `Registry(name)` to the full tool list when `search: all`, or generates the deferred-load path when `search: semantic`.

**Fix B1-03: Add `sandbox_context` to SynthesisRequest**
Add to Spec 33 §2.1:
```typescript
interface SandboxContext {
  sandbox_name:  string;
  runtime:       'gvisor' | 'docker' | 'subprocess';
  network:       'bridge_only' | 'none';
  bridge_tools:  string[];         // names of tools callable via bridge
  bridge_url_env: string;          // env var name: 'CLAW_BRIDGE_URL'
  timeout_ms:    number;
}
// Added to SynthesisRequest:
sandbox_context?: SandboxContext;  // present iff using: sandbox(...)
```
The synthesis pass, when `sandbox_context` is present, generates the two-file pattern (wrapper + script) instead of the standard single-file pattern.

**Fix B1-04: Define embedding strategy**
Add to Spec 34 §2.3: The keyword index (`generated/registry/ToolRegistry.index.json`) is pre-built at compile time using BM25 — no embedding API required. BM25 is always the implementation. The spec's reference to "embedding similarity" is renamed to "relevance search" with BM25 as the concrete algorithm. Semantic embeddings are marked as a future extension (post v1.0). This removes the runtime embedding API dependency entirely.

**Fix B1-05: Mandate Node ≥ 18 in sandbox**
Add to Spec 34 §3.2: A `node_version` field (optional, default `"22"`). The generated container invocation uses `node:${node_version}-slim`. The compiler emits a warning if the user's system Node is < 18 at build time. The sandbox script uses `node-fetch` import as a fallback if needed (the synthesis pass is instructed to use `import fetch from 'node-fetch'` inside the script for maximum compatibility).

**Fix B1-06: Restrict examples output to scalar values**
Add to Spec 34 §4.7: `examples {}` output values must be scalar literals (string, int, float, boolean) or arrays of scalars. Nested object literals are NOT supported in the examples block. For tools with complex nested output types, only the top-level scalar fields need to be exemplified. The compiler emits `UnknownExampleField` if a non-scalar is provided.

---

## Round 2: Integration with Specs 32 and 33

### Maker Pass — What integrates well

1. **Artifact format extensibility** — The existing `artifact.clawa.json` is a JSON object. New `registries[]`, `sandboxes[]`, and `description` fields on tools are purely additive. Existing Synthesis Pass implementations ignore unknown fields.
2. **Synthesis Pass contract unchanged** — The `SynthesisModelAdapter` interface doesn't change. Sandbox context is an addition to `SynthesisRequest` which Spec 33 §2.1 marks as extensible.
3. **MCP server extensibility** — `generated/mcp-server.js` already adds tools dynamically. `tool_search` and `tool_load` are generated as additional MCP tools using the same pattern.
4. **Test tier compatibility** — `examples {}` doesn't touch the test tier. Contract tests and behavior tests continue to work exactly as in Spec 32 §6.

### Breaker Pass — Integration gaps

**B2-01: `tool_load` race condition with MCP tool registration**
Spec §2.3 says agents call `tool_search` to discover tools, then call them. But the MCP server currently registers ALL tools at startup. The deferred-load promise is broken: even with `search: semantic`, all tool definitions are still registered in the MCP server and exposed to the agent. The agent can bypass `tool_search` entirely and call any tool by name.

**B2-02: `AgentTools::Registry` breaks semantic analyzer**
`semantic/mod.rs` validates agent tools by checking `agent.tools` against declared tool names. After the AST change (Fix B1-02), the semantic analyzer needs to resolve `AgentTools::Registry(name)` → expand to tool list from the registry → validate each tool exists. The current semantic pass doesn't know about registries.

**B2-03: Sandbox bridge tools not validated as synthesis-path tools**
The Bridge proxies calls to host tool implementations. But `bridge_tools` might include tools that use `invoke:` (old path) not `using:` (synthesis path). The bridge server generator imports from `generated/tools/`, but `invoke:` tools don't have generated TypeScript files in `generated/tools/`. They're in `generated/mcp-server.js` as MCP tool handlers. The import would fail at runtime.

**B2-04: `reason {}` in a sandbox-using workflow — undefined interaction**
A workflow can contain both `reason {}` blocks and calls to sandbox tools. The `reason {}` block generates a runtime LLM call. Inside a sandbox script, there's no LLM access. What happens if the synthesis pass generates a sandbox script that contains a `reason {}` equivalent? The spec is silent on this.

**B2-05: `description:` vs `system_prompt` — synthesis prompt injection conflict**
Both `description:` and `system_prompt` go into the synthesis pass prompt. Spec 33 §3 shows the prompt template but doesn't define WHERE in the template `description:` appears. If `description:` is long, it can crowd out the `## REFERENCE IMPLEMENTATIONS` section which is empirically the most important part of the synthesis prompt.

**B2-06: Registry index file is pre-built but tool descriptions can change**
The `generated/registry/ToolRegistry.index.json` is built at compile time. If a user edits a tool's `description:` without re-running `claw build`, the index is stale. The `tool_search` tool would return outdated descriptions. Spec 32's synthesis cache mechanism (§15) uses `sha256(source_hash + ...)` — this would catch the change and invalidate the cache, but only if the synthesis cache is enabled.

---

### Round 2 Fixes

**Fix B2-01: MCP server deferred registration for semantic registries**
When a registry has `search: semantic`, the MCP server does NOT register the registry's tools at startup. Instead, it registers only `tool_search` and `tool_load`. When `tool_load(name)` is called, the MCP server dynamically registers the tool for that session and returns its definition. This gives genuine deferred loading. Add to Spec 34 §2.3 and Spec 32 §5 (Generated TypeScript Structure).

**Fix B2-02: Semantic analyzer registry resolution**
Add to Spec 34 §2.5: The semantic analyzer resolves `AgentTools::Registry(name)` in two steps:
1. Check that the registry is declared in `document.registries`.
2. For each tool name in `registry.tools`, verify it is declared in `document.tools`.
The existing `UndefinedTool` error applies if a registry tool is not declared. Add `UndefinedRegistry` (E-R01) if the registry itself is not declared.

**Fix B2-03: Bridge tools restricted to synthesis-path tools**
Add to Spec 34 §3.7: Validation rule — tools listed in `bridge_tools` MUST have `using:` declared (synthesis path). If a `bridge_tools` entry has `invoke:` only → new error `E-S04: BridgeToolNotSynthesized` with message:
```
error E-S04: bridge_tools entry 'LegacyTool' uses invoke: but sandbox bridge requires a synthesized TypeScript implementation.
  hint: change 'invoke:' to 'using: fetch' (or another capability) and re-run claw build.
```

**Fix B2-04: `reason {}` forbidden inside sandbox scripts**
Add to Spec 34 §3: A tool using `using: sandbox(...)` may not be referenced inside a `reason {}` block (it has no LLM access). More precisely: the synthesis pass is instructed (prompt constraint in Spec 33 §3) that sandbox scripts must be pure compute — no LLM calls, no external HTTP except bridge, no `reason`. The compiler enforces this at the DSL level: if a `reason {}` block's `using:` agent has any tool with `using: sandbox(...)`, emit a warning `W-S02: AgentUsedInReasonHasSandboxTool`. This is a warning not an error because the sandbox tool might not actually be called in the reasoning path.

**Fix B2-05: `description:` placement in synthesis prompt**
Add to Spec 33 §3 (prompt template update):
```
## TOOL SPECIFICATION
Name:        {{ tool.name }}
Description: {{ tool.description }}       ← new, before inputs
Inputs:      {{ tool.inputs }}
Output type: {{ tool.output_type }}
Capability:  {{ tool.using }}
```
`description:` goes immediately after the name in the tool spec section, before the parameter list. It is limited to 500 characters in the prompt — the compiler emits `W-E02: DescriptionTooLong` if it exceeds this.

**Fix B2-06: Registry index included in source hash**
Add to Spec 32 §15 (synthesis cache): The cache key is:
```
sha256(source_hash + synthesizer_model + reference_lib_version + registry_index_hash)
```
where `registry_index_hash = sha256(registry.index JSON)`. Any change to tool descriptions rebuilds the index and invalidates the cache. This is already correct behavior since `source_hash` is `sha256(document AST)` which includes `description:` fields.

---

## Round 3: Security and Edge Cases

### Maker Pass — What is secure

1. **Credential isolation** — The sandbox script never sees API keys. The bridge server holds credentials server-side. Network isolation (bridge_only) prevents the script from exfiltrating credentials over the internet.
2. **Bridge is localhost-only** — The bridge listens on `127.0.0.1:PORT` with a random ephemeral port. Only the spawned container can reach it (via port forwarding).
3. **No ambient authority** — The sandbox script cannot call `bridge_tools` not in the declared list — the bridge server rejects unknown tool names (404).
4. **`subprocess` runtime warning** — Dev-only runtime is warned against in production.

### Breaker Pass — Security and edge cases

**B3-01: Bridge server has no authentication**
The bridge server listens on localhost and routes any call to registered tools. If another process on the same machine sends a request to `http://127.0.0.1:PORT/call/ExpenseAPI`, it would succeed. There's no bearer token or request signing between the sandbox script and the bridge.

**B3-02: Sandbox script injection via `CLAW_INPUT`**
The bridge server gets `inputs` from `CLAW_INPUT` env var. If the original tool inputs contain JSON injection characters and the sandbox script passes them directly to bridge tool calls without sanitization, an attacker controlling `employees[0].id` could craft a payload that corrupts the bridge JSON protocol.

**B3-03: Registry `tool_search` returns tool definitions — leaks implementation?**
The `tool_load(name)` endpoint returns the full tool definition (input schema, description). For MCP tools that wrap proprietary APIs, this might expose internal schema structure. This is a concern for multi-tenant deployments.

**B3-04: Large example sets bloat MCP schema**
The MCP tool description field (§4.5) injects all examples as text. If a tool has 10 examples each with complex objects, the MCP schema description could become enormous (multi-KB) and degrade MCP client performance.

**B3-05: Sandbox timeout — what happens to the bridge server?**
If a sandbox script times out (`timeout_ms` exceeded), the container is killed but the bridge server keeps running. If another sandbox invocation starts immediately, a new bridge server is spawned on a new port. Over many invocations, leaked bridge servers accumulate. The spec doesn't define bridge lifecycle management.

**B3-06: `tool_search` with `search: semantic` has no relevance threshold**
`tool_search(query)` returns matching tools but the spec doesn't define a minimum relevance score or maximum number of results. An agent could get 20 tool matches for a vague query and put all of them in context, defeating the purpose of deferred loading.

---

### Round 3 Fixes

**Fix B3-01: Bridge authentication token**
Add to Spec 34 §3.4: The bridge server generates a random 32-byte token at startup (`crypto.randomBytes(32).toString('hex')`). This token is injected into the sandbox container as `CLAW_BRIDGE_TOKEN`. The bridge server validates the `Authorization: Bearer <token>` header on every request. The sandbox script must include this header in all bridge calls. The synthesis pass is instructed (prompt constraint) to always include the `CLAW_BRIDGE_TOKEN` env var in bridge `fetch()` calls.

**Fix B3-02: JSON protocol uses structured envelope**
Add to Spec 34 §3.3: The bridge protocol is not raw JSON passing. Tool inputs from `CLAW_INPUT` are base64-encoded by the host before injection and decoded by the sandbox script. The bridge request body uses a typed envelope:
```json
{ "tool": "ExpenseAPI", "args": { "employee_id": 42 } }
```
The `args` object is validated against the tool's declared input schema (Zod parse) by the bridge server before forwarding. This prevents injection.

**Fix B3-03: tool_load does not expose implementation details**
Add to Spec 34 §2.3: `tool_load(name)` returns ONLY the tool's input schema and description — not its `using:` capability, not the TypeScript source, not synthesis metadata. The MCP tool definition response is:
```json
{ "name": "WebSearch", "description": "...", "inputSchema": { ... } }
```

**Fix B3-04: Cap example injection into MCP schema**
Add to Spec 34 §4.5: MCP schema description injection is limited to the first 3 examples, truncated to 80 characters per example, with a hard cap of 500 total characters for the examples section. Additional examples are still included in the artifact and the synthesis prompt but not injected into the MCP schema.

**Fix B3-05: Bridge server lifecycle — managed by sandbox runner**
Add to Spec 34 §3.5: The `runSandbox()` function owns the bridge server lifecycle:
```typescript
const bridgeUrl = await startBridge();
try {
  const result = await runContainer(script, inputs, bridgeUrl, timeoutMs);
  return result;
} finally {
  await stopBridge(bridgeUrl);  // always closed, even on timeout
}
```
`stopBridge()` closes the HTTP server and releases the port. This is enforced in the generated `runtime/sandbox.ts`.

**Fix B3-06: tool_search result cap and relevance floor**
Add to Spec 34 §2.3: `tool_search(query)` returns at most 5 tools ranked by BM25 score with a minimum score threshold of 0.1 (normalized). The compiler injects these defaults into the generated `tool_search` implementation. Both are configurable in the registry declaration:
```
registry ToolRegistry {
    tools         = [...]
    search        = semantic
    max_results   = 5      // optional, default 5
    min_relevance = 0.1    // optional, default 0.1 (0.0 = return all)
}
```

---

## Summary: 18 Gaps Found and Fixed

| Round | ID | Category | Fix |
|---|---|---|---|
| 1 | B1-01 | Grammar | `description:` added as ToolProperty::Description(String) |
| 1 | B1-02 | AST | AgentTools enum (List vs Registry) |
| 1 | B1-03 | Synthesis | SandboxContext added to SynthesisRequest |
| 1 | B1-04 | Runtime dep | Embedding → BM25 only (no runtime API needed) |
| 1 | B1-05 | Compat | Node ≥ 18 enforcement + node-fetch fallback |
| 1 | B1-06 | Grammar | Examples output restricted to scalar literals |
| 2 | B2-01 | MCP | Deferred registration: only tool_search+tool_load at startup |
| 2 | B2-02 | Semantic | Analyzer resolves AgentTools::Registry |
| 2 | B2-03 | Validation | bridge_tools must be synthesis-path only (E-S04) |
| 2 | B2-04 | Interaction | reason{} agent with sandbox tools → W-S02 |
| 2 | B2-05 | Prompt | description: placement in synthesis prompt |
| 2 | B2-06 | Cache | registry_index_hash added to cache key |
| 3 | B3-01 | Security | Bridge auth token (CLAW_BRIDGE_TOKEN) |
| 3 | B3-02 | Security | Zod validation + base64 input encoding in bridge |
| 3 | B3-03 | Privacy | tool_load returns schema only, not implementation |
| 3 | B3-04 | Perf | MCP example injection capped at 3 examples / 500 chars |
| 3 | B3-05 | Lifecycle | Bridge server closed in finally block |
| 3 | B3-06 | UX | tool_search capped at 5 results, min_relevance configurable |
