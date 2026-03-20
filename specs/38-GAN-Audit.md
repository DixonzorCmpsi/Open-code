# Spec 38 GAN Audit — Closed-Loop Runtime

**Date:** 2026-03-20
**Question being audited:** Should Spec 38 keep OpenCode as the orchestrator (and remove MCP server), or build a fully standalone runtime independent of OpenCode?

---

## 0. Critical Finding — The Current MCP Server Is Broken

Before the architectural debate, there is a bug that must be named explicitly.

The current `generated/mcp-server.js` `handleagent_Writer` implementation:

```javascript
async function handleagent_Writer(args) {
  const result = execSync(
    `opencode -p ${JSON.stringify(task)} -q`,   // ← spawns a CHILD opencode process
    { env: { ...process.env, OPENCODE_CONTEXT: tmpCtx }, ... }
  );
}
```

When OpenCode calls the `agent_Writer` MCP tool, the MCP server **spawns a new `opencode` child process**. The actual execution chain is:

```
OpenCode process
  → reads Summarize.md (natural language command stub)
  → LLM decides to call agent_Writer MCP tool
    → mcp-server.js calls execSync("opencode -p task -q")
      → new opencode process starts (no model configured for it)
        → FAILS or uses default model with no context
```

This is not multi-agent orchestration. It is accidental recursion. The child `opencode` process has:
- No reference to `claw.json` or the Writer agent's system prompt
- No access to the `WebSearch` MCP tool (it starts fresh)
- A race condition if the parent and child both try to use the same MCP port

**This is the real problem to fix, regardless of which architectural direction is chosen.**

---

## 1. Maker Pass — Case For Keeping OpenCode as Orchestrator

**Premise:** OpenCode is the agent runtime. `.claw` is a configuration DSL. We fix the MCP server so it is correct, not abandon it.

### 1.1 OpenCode Already Solves Hard Problems
OpenCode handles: model provider abstraction (Anthropic, Ollama, 20+ others), streaming, context window management, token counting, tool call formatting, retry on rate limits, TUI/web UI. Building all of this from scratch in `generated/runtime/index.js` is a multi-month project.

### 1.2 The Fix Is Simple
The broken `handleagent_Writer` just needs to call the LLM API directly instead of spawning a child process:

```javascript
// Correct: call Anthropic SDK directly from MCP server
async function handleagent_Writer(args) {
  const client = new Anthropic();
  const response = await client.messages.create({
    model: resolveModel(),
    system: "You are a concise technical writer...",
    messages: [{ role: "user", content: args.task }],
    tools: [WEBSEARCH_TOOL_DEFINITION],
    max_tokens: 4096,
  });
  return handleToolLoop(response, client);
}
```

This fixes the double LLM hop WITHOUT abandoning OpenCode. The MCP server becomes a proper agent runner that calls the LLM directly. OpenCode calls it once, gets a result, moves on. No child process.

### 1.3 OpenCode Is The Right UI Layer
Interactive workflows — "show me the reasoning", "revise that summary", "run it again with different parameters" — are naturally handled by the OpenCode chat UI. A standalone `claw run` produces JSON on stdout. Both are useful; neither replaces the other.

### 1.4 MCP Is The Right Tool Protocol For OpenCode Integration
OpenCode's internal architecture routes tool calls through MCP. This is a boundary we cannot move — OpenCode is closed source and `type: "local"` is the supported integration point. Generating a correct MCP server (not a broken one) is the right path for OpenCode integration.

---

## 2. Breaker Pass — Gaps In "Keep OpenCode" Argument

### B1-01: The Type Contract Is Lost At The MCP Boundary
`.claw` declares `-> Summary { title: string, body: string, confidence: float }`. OpenCode sees the MCP tool return a JSON string. It does not validate this against the Claw schema. If the LLM returns `{ "title": "...", "summary": "..." }` (wrong field name), OpenCode has no way to catch it — the MCP tool returns a string and OpenCode treats it as success.

**Fix needed:** Type validation must happen inside the MCP tool handler, not delegated to OpenCode.

### B1-02: Command Stubs Are Non-Deterministic Orchestration
`.opencode/command/Summarize.md` tells OpenCode's LLM to "Execute step 1: Call MCP tool `agent_Writer`...". The LLM reads this in natural language and decides to follow it. If the LLM paraphrases the instruction, misses a step, or adds unrequested steps, the workflow deviates. The outer LLM hop adds non-determinism where there should be none.

**The workflow logic is already fully encoded in the AST.** It does not need a second LLM to interpret it.

### B1-03: `claw run` Cannot Be Implemented Over OpenCode
"I want to run this workflow from a cron job" is a legitimate requirement. OpenCode is an interactive IDE. It does not expose a simple `opencode run-workflow Summarize --arg topic=X` command with exit code semantics. The `-q` flag used in the broken child process spawn is not documented and its behaviour is unstable.

**OpenCode alone cannot satisfy the non-interactive execution requirement.**

### B1-04: Multi-Agent Isolation Requires Multiple LLM Calls
In `.claw`, `agent Researcher` and `agent Writer` can have different `client` declarations (different models, different system prompts, different tool sets). OpenCode has one active model per session. Mapping multi-agent Claw workflows to "one OpenCode session" loses per-agent model isolation.

### B1-05: OpenCode Is Not A Stable Build Target
OpenCode is under active development. Field names in `opencode.json` have already changed once (discovered during this audit session: `model`, `mcp`, `instructions` — not `agents.coder.model`, `mcpServers`, `contextPaths`). Claw's compiler emits configuration that depends on OpenCode's exact schema. Every OpenCode release is a potential breaking change.

---

## 3. Maker Pass — Case For Standalone Runtime (Spec 38 As Written)

**Premise:** Claw compiles to a self-contained execution artifact. OpenCode becomes one optional output format (for IDE users), not the required runtime.

### 3.1 The Agent Loop Is Already Specified In The AST
`execute Writer.run(task: "...")` is a deterministic instruction: call the LLM with Writer's system prompt, manage tool calls, return a typed result. This is a 100-line agent loop. It does not require a full IDE.

### 3.2 Correct Multi-Agent Isolation By Default
Each `execute Agent.run()` uses that agent's declared `client` (model, provider, settings). Different agents use different models as specified in `.claw`. No session conflicts.

### 3.3 `claw run` Enables Real Use Cases
- Cron jobs, CI/CD pipelines, batch processing
- Embedding Claw workflows in larger systems (REST API wrapper via `claw serve`)
- Testing workflows deterministically without an IDE
- Cost-controlled execution with budget limits from Spec 36

### 3.4 No Dependency Drift
The compiled runtime depends only on the LLM provider SDK (e.g. `@anthropic-ai/sdk`) and the generated TypeScript functions. It does not depend on OpenCode's version, config schema, or undocumented flags.

---

## 4. Breaker Pass — Gaps In Standalone Runtime Argument

### B2-01: We Are Rebuilding OpenCode's Provider Layer
Supporting Anthropic, Ollama, OpenAI, Mistral, etc. with correct message formatting, tool call schemas, streaming, and token limits is 90% of what OpenCode does. The Spec 38 runtime glosses over this with `callLLM(client, messages, agent.toolSchemas)`.

**This is an under-specified function that hides months of work.**

### B2-02: Loses OpenCode IDE Integration For Users Who Want It
Many users want to use Claw workflows interactively in an IDE — "run Summarize, show me the result, now refine it." A pure standalone runtime has no chat UI. OpenCode integration must be preserved for this use case.

### B2-03: Spec 38 Generates Both Runtime AND MCP Server
Adding `generated/runtime/` on top of the existing artifact outputs increases build complexity and output surface area. If both MCP and runtime are generated, they must stay in sync.

---

## 5. Verdict: Dual-Track Architecture

Neither argument wins cleanly. The correct answer is a **layered architecture** where both execution modes exist and OpenCode remains supported but is no longer required:

```
                          .claw source
                              │
                        [claw build]
                              │
             ┌────────────────┼────────────────┐
             │                │                │
    ┌────────▼────────┐ ┌─────▼──────┐ ┌──────▼──────────┐
    │  generated/      │ │ generated/ │ │  opencode.json   │
    │  runtime/        │ │ mcp-server │ │  + command/*.md  │
    │  (standalone)    │ │ .js (fixed)│ │  (IDE config)    │
    └────────┬────────┘ └─────┬──────┘ └──────┬──────────┘
             │                │                │
       claw run          OpenCode IDE      OpenCode IDE
       claw serve        (tool calls)      (interactive)
       CI/CD             (correct, no      (unchanged UX)
       cron jobs          child spawning)
```

### 5.1 What "Fixed MCP Server" Means

The MCP server codegen (`src/codegen/mcp.rs`) must be rewritten so `handleagent_Writer`:
1. Calls the LLM API **directly** using the agent's declared `client`
2. Manages the tool loop natively (no child `opencode` spawn)
3. Validates the return type against the declared schema before returning
4. Returns typed JSON, not a raw string

This is the most urgent fix — it is a correctness bug, not an architectural decision.

### 5.2 What Spec 38 Becomes

Spec 38 should define:
1. **Fix MCP server codegen** (§6) — replace child-process spawn with direct LLM API call
2. **`claw run` CLI** (§5) — uses `generated/runtime/` for non-IDE execution
3. **`runtime {}` DSL block** (§3) — optional, controls which outputs are generated
4. **OpenCode remains the primary IDE integration** — MCP is still generated by default

The standalone runtime is an **addition**, not a replacement. OpenCode is NOT deprecated.

### 5.3 What Spec 38 Does NOT Do

- Does not remove `generated/mcp-server.js` generation
- Does not deprecate `opencode.json` generation
- Does not require users to migrate away from OpenCode
- Does not reimplement OpenCode's provider abstraction — the runtime uses the same `claw.json` client config and calls the provider SDK once per agent, not a full IDE

---

## 6. Changes Required To Spec 38

| Section | Change |
|---|---|
| §1 Problem statement | Add the child-process-spawn bug as Finding 0 |
| §2 Architecture diagram | Show OpenCode as a supported export path alongside standalone |
| §6 MCP server fix | New section: rewrite `handleagent_Writer` to call LLM directly |
| §8 MCP as optional | Reframe: MCP is NOT optional by default — it is still the default; standalone is additive |
| §13 "Why not ditch MCP" | Strengthen — MCP stays for IDE users; standalone adds CI/CD capability |

**Revised default:** `runtime {}` absent → generate both MCP server (for OpenCode) AND `generated/runtime/` (for `claw run`). No breaking change. Users gain `claw run` without losing OpenCode integration.
