# OpenClaw OS: The Execution Backend

The `.claw` DSL and its generated SDKs provide strict routing, type safety, and a deterministic developer experience. However, the language itself is lightweight. The actual heavy lifting—calling LLMs, opening browsers, reading files, and spinning up sandboxes—is performed by **OpenClaw (The Backend OS)**.

This document serves as the architecture contract between the compiled `.claw` SDK and the OpenClaw Gateway.

## 1. The Separation of Concerns

* **`clawc` (The Compiler):** Validates types and generates standard TypeScript/Python APIs. 
* **The Generated SDK:** The code running in the developer's server. It serializes the inputs and waits for answers. It does *not* interact with Playwright or LLM keys directly.
* **OpenClaw OS (The Gateway):** A persistent background server (written in TypeScript/Node) that acts as the physical operating system for the agents.

**Minimum Node.js version:** `22.6.0` or higher. The gateway uses native TypeScript execution via `--experimental-strip-types` and `node:sqlite`, so Node 18/20 are not sufficient for this implementation. Node 22 LTS or newer is recommended.

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
4. **Schema Degradation Prevention:** The OS inspects the final JSON payload from an LLM call ALONGSIDE the TypeBox schema. The `isSchemaDegraded(value, schema)` function receives BOTH the response AND the schema so it can determine zero-values per-type (0 for numbers, "" for strings, false for booleans). A response is **degraded** if and only if **ALL** leaf values are their type's zero-value simultaneously. Individual `0`, `false`, or `""` values are NOT degraded. Only when the entire response is uniformly blank/zero does the OS throw `SchemaDegradationError`.
   - **Financial Circuit Breaker:** There is a hard-coded limit of 3 sequential degradation retries per node. If the execution faults 3 consecutive times, the OS fatally faults the node to prevent infinite generation loops hitting the LLM APIs. (Enforced additionally via a `max_cost_per_session` tracking param in `claw.json`).
5. **Physical Tool Execution:** When the LLM calls `Browser.search`, the OpenClaw OS spins up a headless Chromium instance, executes the search, and returns the raw DOM context. See `specs/13-Visual-Intelligence.md` for screenshot and vision capabilities.
6. **State Checkpointing & Resumption:** The Gateway acts as an Event Sourcing engine. After **every** successfully completed AST node execution — including `LetDecl`, `ForLoop`, `IfCond`, `ExecuteRun`, `Return`, `Expression`, `MethodCall`, and `BinaryOp` — the OS commits the execution state to a persistent checkpoint store. No statement type is exempt from checkpointing. By default, state is stored in `{projectRoot}/.claw/engine.sqlite`, where `projectRoot` is resolved by walking upward from the current working directory until `openclaw.json` is found, falling back to the current working directory if no config file is present. On Windows, the `.openclaw` directory should be marked hidden when possible. In distributed production environments, this can be swapped to Redis via `REDIS_URL`. Supported formats are `redis://[user:pass@]host:port[/db]` and `rediss://[user:pass@]host:port[/db]` for TLS. If the server crashes, any gateway instance in the cluster can resume the AST traversal. 
   - **Immutable Cache Replays (Eager Event Hydration):** Upon resumption, historical nodes are NOT re-evaluated. The OS efficiently bulk-fetches all stored events into an O(1) in-memory map avoiding N+1 database queries. Outputs are instantly returned, totally bypassing the LLM.
   - **AST Hash Binding (Durable AST Registries):** Resumptions are tightly bound to `CLAW_AST_HASH`. To protect against ephemeral CI/CD filesystem destruction, the OS explicitly writes the `document.json` schema into the SQL/Redis store under an `ast_registry` namespace. Old suspended sessions load natively against their durable schemas.
   - **Lazy Zero-Trust Restoration:** The Gateway trusts nothing returning from the DB, but avoids O(N^2) CPU recursive array validation. It executes TypeBox `validateAgainstSchema` *lazily*, localized tightly to the exact read-cycle the payload re-enters the active traversal tree.
   - **Compiler Binary Resolution Waterfall:** If the Gateway OS requires spawning a child process to interact with the AST, it uses a deterministic hierarchy to find the compiler: `executable_path` (in `claw.json` gateway options) → `CLAW_BINARY_PATH` (env) → `node_modules/.bin/claw` → global `$PATH`.

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

---

## 5. `env()` Expression Resolution

The `.claw` DSL uses `env("VARIABLE_NAME")` in client declarations to reference environment variables. This is a compile-time marker and a runtime lookup:

- **Compile time:** The parser treats `env("...")` as a function call expression (`Expr::Call`). The compiler does NOT resolve the value — it serializes the expression as-is into `document.json`.
- **Runtime (gateway):** When the traversal engine encounters a client with `endpoint: env("CUSTOM_LLM_URL")`, it resolves via `process.env["CUSTOM_LLM_URL"]`. If the variable is not set, the client initialization fails with a descriptive error: `"Environment variable CUSTOM_LLM_URL is not set (required by client LocalLLM)"`.
- **BAML emission:** The BAML emitter converts `env("KEY")` to BAML's `env.KEY` syntax.

---

## 6. Security Contract

All gateway security requirements are defined in `specs/12-Security-Model.md`. Key mandates:

- **Request body size:** `MAX_REQUEST_BODY_SIZE = 1_048_576` bytes (1 MB). Reject oversized payloads before JSON parsing.
- **Session IDs:** MUST use `crypto.randomUUID()`. NEVER use `Date.now()` or any timestamp-based generation.
- **API key comparison:** MUST use `crypto.timingSafeEqual()`. NEVER use `===` or `!==` for secret comparison.
- **Tool path resolution:** MUST use `fs.realpath()` and verify the resolved path remains within the workspace root. Symlinks must not escape the workspace boundary.

---

## 6. LLM API Contracts

### OpenAI (Responses API)
The gateway uses OpenAI's Responses API with structured output:
```json
{
  "model": "gpt-4o",
  "input": [
    { "role": "system", "content": "..." },
    { "role": "user", "content": "..." }
  ],
  "text": {
    "format": {
      "type": "json_schema",
      "name": "SearchResult",
      "schema": { ... }
    }
  }
}
```

### Anthropic (Messages API with Tool Use)
The gateway MUST use Anthropic's `tools` parameter with `input_schema` for constrained output. The schema is placed in the **top-level request body** under `tools`, NOT inside `messages[].content`.

```json
{
  "model": "claude-sonnet-4-6",
  "max_tokens": 4096,
  "system": "You are a deterministic OpenClaw execution agent.",
  "tools": [
    {
      "name": "structured_output",
      "description": "Return the result matching the required schema",
      "input_schema": {
        "type": "object",
        "properties": { ... },
        "required": [ ... ]
      }
    }
  ],
  "tool_choice": { "type": "tool", "name": "structured_output" },
  "messages": [
    { "role": "user", "content": "..." }
  ]
}
```

The response is extracted from `content[].type === "tool_use"` → `content[].input`.

Model identifiers above are illustrative defaults, not eternal constants. Implementations MUST verify current provider model IDs before shipping generated defaults or examples.

**NEVER** place `response_schema` inside the message content string — Anthropic's API ignores fields embedded in message content.

---

## 7. HTTP Hardening

All HTTP responses MUST include security headers as defined in `specs/12-Security-Model.md` Section 3.2.

The gateway MUST validate `Content-Type: application/json` on POST requests and return HTTP 415 (Unsupported Media Type) for non-JSON content types.

---

## 8. Graceful Shutdown

On `SIGTERM` or `SIGINT`:
1. Stop accepting new HTTP connections and WebSocket upgrades
2. Wait up to 30 seconds for in-flight workflow executions to complete
3. Checkpoint all running sessions with status `"interrupted"`
4. Close the checkpoint store (flush SQLite WAL or disconnect Redis)
5. Exit with code 0

In-flight sessions that don't complete within the drain period are checkpointed at their current state and can be resumed later.

On Windows, `SIGTERM` is not delivered for normal process management. The gateway MUST additionally expose an authenticated `POST /shutdown` endpoint that performs the same graceful drain sequence. The `openclaw dev` command MUST use this endpoint for programmatic shutdown on Windows, and MAY use it on Unix as well before falling back to process termination.

---

## 9. WebSocket Streaming

The gateway supports real-time streaming of workflow execution events over WebSocket. See `specs/11-WebSocket-Protocol.md` for the full protocol specification.

The WebSocket endpoint is at `/workflows/stream` and requires authentication per `specs/12-Security-Model.md`.
