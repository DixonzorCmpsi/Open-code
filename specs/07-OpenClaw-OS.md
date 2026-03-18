# OpenClaw OS: The Execution Backend

The `.claw` DSL and its generated SDKs provide strict routing, type safety, and a deterministic developer experience. However, the language itself is lightweight. The actual heavy lifting—calling LLMs, opening browsers, reading files, and spinning up sandboxes—is performed by **OpenClaw (The Backend OS)**.

This document serves as the architecture contract between the compiled `.claw` SDK and the OpenClaw Gateway.

## 1. The Separation of Concerns

* **`clawc` (The Compiler):** Validates types and generates standard TypeScript/Python APIs. 
* **The Generated SDK:** The code running in the developer's server. It serializes the inputs and waits for answers. It does *not* interact with Playwright or LLM keys directly.
* **OpenClaw OS (The Gateway):** A persistent background server (written in TypeScript/Node) that acts as the physical operating system for the agents.

## 2. The Execution Contract

When a developer calls a `.claw` workflow in their backend script:
```typescript
const report = await AnalyzeCompetitors(["Apple"], { client: gateway })
```
The SDK sends a strictly formatted JSON payload over WebSocket or REST to the OpenClaw OS.

### The Request Payload
```json
{
  "workflow": "AnalyzeCompetitors",
  "arguments": { "company": ["Apple"] },
  "ast_hash": "a1b2c3d4e5f6...",
  "session_id": "req_987654"
}
```

### Gateway Responsibilities
Upon receiving this payload, the OpenClaw OS is responsible for:
1. **Executing the Graph:** It reads the compiled `.claw` AST and begins traversing the `AnalyzeCompetitors` workflow.
2. **LLM Orchestration:** It constructs the prompt package, injects the correct conversation history, and handles the connection to the specified `client` (OpenAI, Anthropic).
3. **Constrained Decoding (The Bouncer):** It enforces the TypeBox schemas defined by the DSL, ensuring the LLM token output perfectly matches the expected tool signature.
4. **Schema Degradation Prevention:** The OS inspects the final JSON payload. If the Bouncer successfully forced the LLM into the schema but the LLM populated all fields with default empty strings (`""`) or zeroes because the schema was too complex for the model size, the OS throws a `SchemaDegradation` error. It triggers a retry block rather than passing functionally blank hallucinated parameters to the tool wrapper.
5. **Physical Tool Execution:** When the LLM calls `Browser.search`, the OpenClaw OS spins up a headless Chromium instance, executes the search, and returns the raw DOM context.
5. **State Checkpointing & Resumption:** The Gateway acts as an Event Sourcing engine. After every successfully completed AST node, the OS commits the execution graph state to a persistent internal database (by default, a local SQLite file in the `.openclaw/` directory). If the server crashes, it can resume the AST traversal exactly where it left off. In distributed production environments, this can be swapped to Redis via the `REDIS_URL` environment variable.

## 3. Sandboxing External Tools (Python/TypeScript)

If the `.claw` script uses an external custom tool defined via:
`invoke: module("scripts.analysis").function("get_sentiment")`

The OpenClaw OS must safely execute that external script. 

* **The Vision:** OpenClaw OS will use secure sandboxing (e.g., executing Python tools inside lightweight Docker containers or WebAssembly runtimes) to prevent malicious or buggy custom tools from crashing the OS.

## 4. Closing the Loop

Once the `AnalyzeCompetitors` workflow natively reaches its `return` statement inside the Gateway, the OpenClaw OS serializes the result back to the waiting Developer's Server:

### The Response Payload
```json
{
  "session_id": "req_987654",
  "status": "success",
  "result": {
    "url": "https://apple.com/news",
    "confidence_score": 0.95,
    "snippet": "Apple releases new XR headset.",
    "tags": ["hardware", "xr"]
  }
}
```

The generated SDK takes this payload, validates it using Zod/Pydantic one last time (as specified in `06-CodeGen-SDK.md`), and returns it to the user's Node.js/FastAPI application.
