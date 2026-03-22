# Spec 47: GAN Audit — Round 2: Synthesis Pipeline, Runtime Gaps, OpenCode DSL Expansion

**Status:** ACTIVE — filed 2026-03-20
**Depends on:** Spec 01, 03, 25, 26, 32, 42, 44, 46

---

## 0. Audit Method

All spec files were compared against all source files in `src/`. Findings are rated:

- **CRITICAL** — breaks generated code at runtime today
- **HIGH** — missing spec-required feature, synthesis pipeline incomplete
- **MEDIUM** — spec inconsistency, minor mismatch, stale wording
- **LOW** — naming, docs, non-functional gap

---

## 1. CRITICAL Findings

### C-01 — `ts_workflow.rs` imports agents from non-existent module files

**Location:** `src/codegen/ts_workflow.rs:59`

**What the code emits:**
```typescript
import { Analyst } from '../agents/Analyst.js';
// ...
const result = await Analyst.reason<PurseReport>({ input: ..., goal: ... });
```

**The problem:** Nothing generates `generated/agents/*.js`. The MCP server (`generated/mcp-server.js`) contains `handleagent_Analyst()` as an inline function — it is NOT importable as a module. The import would fail with `ERR_MODULE_NOT_FOUND` at runtime.

**Root cause:** Spec 32 §5 defines `generated/runtime/reason.ts` as a standalone module imported by workflows. The current `ts_workflow.rs` was written against a different mental model (agent-as-module) that has no corresponding codegen.

**Fix required:**
1. Generate `generated/runtime/reason.ts` — a thin async wrapper that calls the agent's LLM client directly (same logic as `handleagent_*` in mcp.rs but in a module-importable form).
2. Change `ts_workflow.rs` to import `reason` from `'../runtime/reason.js'` and call it as: `reason<AgentName, OutputType>({ agent: 'Analyst', input: ..., goal: ... })`.

**Spec reference:** Spec 32 §5, §13

---

### C-02 — `clawReSynthesize()` called in generated TS but never defined

**Location:** `src/codegen/ts_workflow.rs:160`

**What the code emits:**
```typescript
await clawReSynthesize(search_result);
```

**The problem:** `clawReSynthesize` is neither imported nor defined anywhere in the generated TypeScript. Any workflow using `on_fail: re_synthesize` would fail with a `ReferenceError` at runtime.

**Fix required:** Add an import for `clawReSynthesize` from `'../runtime/reason.js'` (or a dedicated `'../runtime/synth.js'`) in workflows that use `re_synthesize`. The runtime module should implement it by spawning `claw synthesize --tool <ToolName>` as a child process.

---

## 2. HIGH Findings

### H-01 — `@min`/`@max`/`@regex` constraints parsed but NOT emitted to MCP JSON Schema

**Location:** `src/codegen/mcp.rs` — `emit_type_schema()`, `data_type_to_json_schema()`

**What the parser produces:** `TypeField.constraints: Vec<Constraint>` with `@min(N)`, `@max(N)`, `@regex("...")` values.

**What the codegen emits:** JSON schema with no `minimum`, `maximum`, `minLength`, `maxLength`, or `pattern` fields — constraints are silently dropped.

**Spec reference:** Spec 26 §3 (JSON Schema mapping table)

**Fix required:** In `emit_type_schema()`, iterate `field.constraints` and append the appropriate JSON Schema keywords based on field type and constraint kind.

---

### H-02 — Agent `extends` not resolved in MCP handler generation

**Location:** `src/codegen/mcp.rs:emit_agent_handler()`

**What spec says:** Spec 32 §21: merged tool list = `parent.tools ∪ child.tools`; child's `system_prompt` overrides unless absent (then use parent's).

**What the code does:** `emit_agent_handler()` only reads `agent.tools` and `agent.system_prompt` directly, ignoring `agent.extends`. Parent tools are not merged. If the child has `system_prompt = None` and a parent, the generated handler uses an empty string instead of the parent's prompt.

**Fix required:** Before emitting the handler, resolve inheritance by walking `agent.extends` up to the root, merging tool lists and falling back to parent system_prompt.

---

### H-03 — `tools +=` not parsed (agent tool inheritance syntax)

**Location:** `src/parser.rs` — agent property parser

**What spec 03 says:** `("tools" ~ ("=" | "+=") ~ "[" ~ tool_ref ~ ("," ~ tool_ref)* ~ "]")`

**What the parser does:** Only parses `tools = [...]` (plain assignment). `tools += [...]` is not recognized.

**Fix required:** Add `+=` as an accepted token in the agent tools property parser and store it in the AST as an additive (merged) tool list at code-gen time.

---

### H-04 — `generated/runtime/reason.ts` not generated

**Location:** No file in `src/codegen/` generates this.

**What spec 32 §5 requires:**
```typescript
// generated/runtime/reason.ts — auto-generated
export async function reason<InputT, OutputT>(opts: {
  agent: string;
  input: InputT;
  goal: string;
  outputType: string;
}): Promise<OutputT>
```

This is the runtime LLM call wrapper used by `reason {}` blocks. Without it, generated workflows cannot import anything from `'../runtime/reason.js'`.

**Fix required:** Add `src/codegen/reason_runtime.rs` (or extend `ts_workflow.rs`) to emit this file. It should call the agent's configured LLM provider (Anthropic/Ollama) with the goal prompt and validate the response against the output schema. It is deterministic codegen — no LLM required.

---

### H-05 — `generated/specs/agents/*.md` and `generated/specs/workflows/*.md` not generated

**Location:** `src/codegen/skill_spec.rs` only emits tool specs.

**What spec 46 §6 requires:** The synthesis loop should also read agent and workflow spec files to understand the full intent context.

**Fix required:** Add `emit_agent_spec()` and `emit_workflow_spec()` to `skill_spec.rs`.

---

### H-06 — Synthesis cache not implemented

**Location:** `src/codegen/synthesize_mjs.rs` — `synthesize.mjs` runner

**What spec 32 §18 requires:** Cache at `.claw-cache/synthesis/{cache_key}/{tool_name}.ts`. Cache key = `sha256(source_hash + model + library_version)`.

**Current behavior:** Every `claw synthesize` call re-runs all tools unconditionally.

**Fix required:** Add cache read/write in `build_script()` output. Cache hit → skip LLM call, still run tests. Cache miss → synthesize, write to cache.

---

### H-07 — MCP isolation during synthesis not implemented

**Location:** `src/codegen/synthesize_mjs.rs`

**What spec 44 G1 requires:** Synthesis sessions must run with an isolated `opencode.json` (no `mcp` section) in a temp directory to prevent the model from calling project tools instead of writing code.

**Current behavior:** `synthesize.mjs` calls the Anthropic SDK directly (bypasses OpenCode entirely). This accidentally solves the MCP interference problem but is not documented as the intentional approach.

**Recommendation:** Document in spec 44/46 that the direct-API approach is the intentional fix for G1 (simpler and more portable than temp-dir isolation). Close G1 as resolved by design.

---

### H-08 — Missing CLI subcommands

**Location:** `src/bin/claw.rs`

**What spec 32 §2 requires:**
- `claw compile` — Stage 1 only (emit artifact, no synthesis)
- `claw verify` — Stage 1-3 with real network (E2E tests)
- `claw bundle` — Stage 1-4 (emit bundled bin artifacts via esbuild)

**Current commands:** `build`, `synthesize`, `run`, `test` — no `compile`, `verify`, or `bundle`.

---

### H-09 — `synthesize {}` block on tools not parsed

**Location:** `src/parser.rs`

**What spec 42 §2.3 / spec 45 §1.2 requires:**
```
tool FetchStealth(url: string) -> PageContent {
    using: playwright
    synthesize {
        strategy: "stealth"
        note:     "Use StealthyFetcher from Scrapling"
    }
}
```

This block passes hints to the synthesis loop. Not in parser or AST.

---

## 3. MEDIUM Findings

### M-01 — Spec 26 uses stale OpenCode schema (`mcpServers`, `type: "stdio"`)

**Location:** `specs/26-MCP-Server-Generation.md` §1, §5

**Spec 26 says:** `type: "stdio"` in `opencode.json` under `mcpServers`
**Spec 25 (newer) says:** `type: "local"` under `mcp`
**Code follows:** Spec 25 correctly. Spec 26 §1 and §5 need updating to match.

---

### M-02 — `optional<T>` type not in AST or parser

**Location:** `src/ast.rs` `DataType` enum; `src/parser.rs`

**What spec 26 §2.4 / spec 03 documents:** `optional<T>` type where the field is not required in JSON Schema.

**Current state:** `DataType` has no `Optional` variant. Not parsed. Spec 26's JSON Schema mapping table for `optional<T>` has no implementation.

---

### M-03 — `reason {}` TS emission does not match spec 32 §13 prompt format

**Location:** `src/codegen/ts_workflow.rs:168`

**Spec 32 §13 requires the prompt injected into the LLM be structured:**
```
CONTEXT (type: SearchResult):
{"url": "...", "snippet": "...", ...}

GOAL: Evaluate this product...

Respond with valid JSON matching this schema: {...}
```

**Current code emits:** `Analyst.reason<PurseReport>({ input: search_result, goal: "..." })` — treating the agent as a module object. The prompt format is entirely in the (yet-to-be-generated) `runtime/reason.ts`. The structured CONTEXT+GOAL prompt is not enforced anywhere.

---

### M-04 — `emit_agent_tool_descriptor()` description omits agent capabilities

**Location:** `src/codegen/mcp.rs:430`

**Spec 26 §2.5 says:** "The MCP tool registration entry includes the agent's system prompt and available tool names in the description field so OpenCode's coder agent knows what each agent does."

**Current code:** Uses a static description format that does NOT include system prompt or tool list. OpenCode gets less context than spec intends.

---

### M-05 — Synthesis `max_retries` hardcoded at 3, not configurable from `claw.json`

**Location:** `src/codegen/synthesize_mjs.rs`

**Spec 32 §20 says:** `{ "synthesis": { "timeout_ms": 180000, "max_retries": 3, "concurrency": 4 } }` in `claw.json`.

**Current code:** `max_iter` CLI arg on `claw synthesize` is configurable, but `claw.json` synthesis config block is not parsed.

---

## 4. LOW Findings

### L-01 — `listener_decl` parsed but never emitted

Spec 03 / spec 01 note this is Phase 7 and intentionally not compiled. No action needed, but it should be documented with a `// Phase 7 — not compiled` comment in `mcp.rs`.

### L-02 — `claw init` does not detect OpenCode installation

**Spec 25 §11:** `claw init` should detect whether OpenCode is installed and print setup instructions if not.

### L-03 — `generated/synthesis-report.md` not generated

**Spec 42 §2.5:** After synthesis, emit a per-tool report. Low priority (requires OpenCode session trace).

### L-04 — `claw build --resume` / `claw build --reset ToolName` not implemented

**Spec 32:** Convenience CLI flags for iterative synthesis loops.

---

## 5. New Feature Proposals: Full OpenCode Customization via `.claw` DSL

The user's intent is that **every customization possible in OpenCode should be expressible in the `.claw` language**. The following new DSL constructs are proposed. Each compiles to the appropriate `opencode.json` section or `.opencode/` file.

### 5.1 Project-level `env {}` block

Controls `opencode.json` → `env` section and shell environment for synthesis sessions.

```
env ProjectEnv {
    ANTHROPIC_API_KEY = env("ANTHROPIC_API_KEY")  // read from shell
    NODE_ENV          = "production"
    LOG_LEVEL         = "info"
}
```

Compiles to:
```json
{
  "env": {
    "ANTHROPIC_API_KEY": "${ANTHROPIC_API_KEY}",
    "NODE_ENV": "production",
    "LOG_LEVEL": "info"
  }
}
```

### 5.2 Project-level `secrets {}` block

Declares which env vars must be present at runtime for the entire project (not per-tool). Compile-time validation + runtime check at MCP server startup.

```
secrets ProjectSecrets {
    ANTHROPIC_API_KEY
    BRAVE_API_KEY
    DATABASE_URL
}
```

Compiles to:
1. MCP server startup validation (throw on missing before binding)
2. `.opencode/REQUIRED_SECRETS.md` listing required vars for onboarding docs

### 5.3 `opencode {}` config block

Top-level OpenCode configuration. All fields map 1:1 to `opencode.json` top-level keys.

```
opencode Config {
    model     = "claude-sonnet-4-6"
    autoshare = false
    theme     = "opencode"
}
```

Compiles to `opencode.json`:
```json
{
  "model": "claude-sonnet-4-6",
  "autoshare": false,
  "theme": "opencode"
}
```

### 5.4 `auth {}` block — Descope integration

For securing agent endpoints with PAT-based session management.

```
auth DescopeAuth {
    provider = "descope"
    project_id = env("DESCOPE_PROJECT_ID")
    flow_id    = "sign-in"
    session_duration = 3600
}
```

Compiles to:
1. MCP server middleware that validates Descope session tokens on incoming requests
2. `generated/auth/descope.js` — generated auth middleware module
3. `opencode.json` `auth` section with provider config

The MCP `CallToolRequestSchema` handler gains a pre-flight:
```javascript
const token = request.params._auth_token;
if (!token) throw new Error("E-AUTH01: missing auth token");
await validateDescopeSession(token, process.env.DESCOPE_PROJECT_ID);
```

### 5.5 `opencode_agent {}` block — OpenCode agent file generation

Current: `agent {}` blocks generate both MCP handlers AND `.opencode/agents/*.md`. The agent spec file content is minimal.

Proposed: Add an explicit `opencode_agent {}` block for OpenCode-specific agent customization without MCP handler generation:

```
opencode_agent DevHelper {
    model       = "claude-sonnet-4-6"
    description = "Expert TypeScript reviewer for synthesized tools"
    instructions_file = "prompts/devhelper.md"
}
```

Compiles to `.opencode/agents/DevHelper.md` with full instructions.

### 5.6 `opencode_command {}` block

Explicit mapping of a workflow to an OpenCode slash command, with custom description and usage:

```
opencode_command FindPurse {
    workflow = FindPurse
    description = "Search for a purse by brand, color, and style"
    usage = "FindPurse brand=Coach color=tan style=tote"
}
```

Compiles to `.opencode/commands/FindPurse.md`.

---

## 6. Priority Fix Order

| # | ID | Severity | Fix | Spec |
|---|---|---|---|---|
| 1 | C-01 | CRITICAL | Generate `generated/runtime/reason.ts`; fix workflow agent imports | Spec 32 §5 |
| 2 | C-02 | CRITICAL | Import/define `clawReSynthesize` in generated TS | Spec 32 §7 |
| 3 | H-01 | HIGH | Emit `@min`/`@max`/`@regex` to MCP JSON Schema | Spec 26 §3 |
| 4 | H-02 | HIGH | Resolve `extends` in agent handler generation | Spec 32 §21 |
| 5 | H-03 | HIGH | Parse `tools +=` syntax | Spec 03 |
| 6 | H-04 | HIGH | Codegen `generated/runtime/reason.ts` | Spec 32 §5 |
| 7 | H-07 | HIGH | Document direct-API approach as intentional G1 fix | Spec 44 |
| 8 | H-08 | HIGH | Add `claw compile`, `claw verify`, `claw bundle` | Spec 32 §2 |
| 9 | M-01 | MEDIUM | Update spec 26 schema to match spec 25 | Spec 26 |
| 10 | 5.1-5.6 | NEW | OpenCode DSL expansion (`env`, `secrets`, `auth`, `opencode_agent`, `opencode_command`) | This spec |
