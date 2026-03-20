# Spec 32: Code Synthesis Pipeline

**Status:** Specced 2026-03-19. Supersedes runtime MCP orchestration as the primary execution model.

---

## 1. Core Concept: The Intent Compiler

Claw is no longer a runtime orchestration language. It is a **code synthesis specification language**.

The fundamental shift:

| Old model | New model |
|---|---|
| LLM reads command file at runtime | LLM reads artifact at build time |
| LLM calls MCP tools to take actions | LLM writes TypeScript that takes actions |
| Non-deterministic — depends on LLM following instructions | Deterministic — TypeScript executes the same way every time |
| Fails when model hallucinates wrong tool name | Fails fast at build time with a test failure |

The LLM in this pipeline is called the **Synthesis Pass**. Its contract is:
- Input: a structured Claw Artifact (`.clawa` JSON)
- Output: TypeScript that implements the declared intent
- It never calls APIs, never takes actions, never runs at execution time

Once the TypeScript is synthesized and passes its tests, the LLM is done. The TypeScript runs deterministically forever after.

---

## 2. Pipeline Stages

```
.claw source
    │
    ▼  Stage 1: compile  (Rust, deterministic, no LLM)
Claw Artifact  (generated/artifact.clawa.json)
    │
    ▼  Stage 2: synthesize  (Synthesis Pass — user-configured LLM/SLM)
TypeScript files  (generated/tools/*.ts, generated/workflows/*.ts)
TypeScript tests  (generated/__tests__/*.test.ts)
    │
    ▼  Stage 3: test  (vitest, no LLM)
  pass ──→ Stage 4: bundle  (esbuild)
  fail ──→ retry synthesis (max 3 attempts) → hard error on 3rd fail
    │
    ▼  Stage 4: bundle  (esbuild → generated/bin/*.js)
Portable single-file artifacts
    │
    ▼  Stage 5: execute  (node, no LLM)
Deterministic output
```

### Stage commands

```bash
claw build          # runs all 5 stages
claw compile        # stage 1 only — emit artifact
claw synthesize     # stages 1-2 — emit TypeScript
claw test           # stages 1-3 — run contract tests (fast, mocked)
claw verify         # stages 1-3 with real network/browser (slow, optional)
claw bundle         # stages 1-4 — emit bin artifacts
```

---

## 3. Claw Artifact Format (.clawa)

The artifact is the interface between the Claw compiler (Rust) and the Synthesis Pass (LLM). It is designed to be information-dense so the LLM generates maximally aligned code without additional context.

```json
{
  "manifest": {
    "claw_version": "0.2.0",
    "source": "demo.claw",
    "source_hash": "sha256:a3f9...",
    "generated_at": "2026-03-19T20:00:00Z"
  },

  "types": [
    {
      "name": "Summary",
      "fields": [
        { "name": "title",      "type": "string" },
        { "name": "body",       "type": "string" },
        { "name": "confidence", "type": "float",
          "constraints": [{ "min": 0.0 }, { "max": 1.0 }] }
      ]
    }
  ],

  "tools": [
    {
      "name": "WebSearch",
      "inputs":      [{ "name": "query", "type": "string" }],
      "output_type": "SearchResult",
      "using":       "fetch",
      "tests": [
        {
          "input":  { "query": "rust language" },
          "expect": {
            "url":     { "op": "!empty" },
            "snippet": { "op": "!empty" },
            "confidence": { "op": "range", "min": 0.0, "max": 1.0 }
          }
        }
      ]
    },
    {
      "name": "Screenshot",
      "inputs":      [{ "name": "url", "type": "string" }],
      "output_type": "ImageFile",
      "using":       "playwright",
      "tests": [
        {
          "input":  { "url": "https://example.com" },
          "expect": { "path": { "op": "!empty" } }
        }
      ]
    }
  ],

  "agents": [
    {
      "name":          "Writer",
      "synthesis_client": "DefaultSynth",
      "system_prompt": "You are a concise technical writer.",
      "tools":         ["WebSearch"],
      "dynamic_reasoning": true
    }
  ],

  "workflows": [
    {
      "name":        "Summarize",
      "inputs":      [{ "name": "topic", "type": "string" }],
      "output_type": "Summary",
      "steps": [
        {
          "kind":   "tool_call",
          "tool":   "WebSearch",
          "args":   { "query": { "interpolate": "Write a concise summary about: ${topic}" } },
          "bind":   "result"
        },
        {
          "kind":  "return",
          "value": "result"
        }
      ]
    },
    {
      "name":        "ResearchAndDecide",
      "inputs":      [{ "name": "query", "type": "string" }],
      "output_type": "Decision",
      "steps": [
        {
          "kind":   "tool_call",
          "tool":   "WebSearch",
          "args":   { "query": { "ref": "query" } },
          "bind":   "raw"
        },
        {
          "kind":        "reason",
          "agent":       "Writer",
          "input":       "raw",
          "goal":        "Analyze the results and decide the best course of action",
          "output_type": "Decision",
          "bind":        "decision"
        },
        {
          "kind":  "return",
          "value": "decision"
        }
      ]
    }
  ],

  "synthesizers": [
    {
      "name":        "DefaultSynth",
      "provider":    "anthropic",
      "model":       "claude-sonnet-4-6",
      "temperature": 0.1,
      "max_tokens":  8192
    }
  ],

  "capability_registry": {
    "fetch": {
      "runtime":    "node-fetch",
      "import":     "import fetch from 'node-fetch';",
      "pattern":    "async function(inputs: T): Promise<U>",
      "mock_strategy": "intercept_fetch"
    },
    "playwright": {
      "runtime":    "@playwright/test",
      "import":     "import { chromium } from 'playwright';",
      "pattern":    "async function(inputs: T): Promise<U>",
      "mock_strategy": "skip_in_unit_tests"
    },
    "mcp": {
      "runtime":    "@modelcontextprotocol/sdk",
      "pattern":    "async function(inputs: T): Promise<U>",
      "mock_strategy": "mock_client"
    },
    "baml": {
      "runtime":    "@boundaryml/baml",
      "pattern":    "async function(inputs: T): Promise<U>",
      "mock_strategy": "mock_baml_client"
    },
    "bash": {
      "runtime":    "node:child_process",
      "import":     "import { exec } from 'node:child_process';",
      "pattern":    "async function(inputs: T): Promise<U>",
      "mock_strategy": "mock_exec"
    }
  }
}
```

The `steps[]` array in workflows is **fully structured** — no natural language. The Synthesis Pass translates steps to TypeScript mechanically, not interpretively.

---

## 4. DSL Changes

### 4.1 Tool declaration: `using:` replaces `invoke:`

```
// OLD
tool WebSearch(query: string) -> SearchResult {
    invoke: module("scripts/search").function("run")
}

// NEW
tool WebSearch(query: string) -> SearchResult {
    using: fetch
    test {
        input:  { query: "rust language" }
        expect: { url: !empty, snippet: !empty }
    }
}

tool Screenshot(url: string) -> ImageFile {
    using: playwright
}

tool ExtractData(html: string) -> PageData {
    using: baml("ExtractPageData")
}

tool RunScript(cmd: string) -> ScriptOutput {
    using: bash
}

tool BraveSearch(query: string) -> SearchResult {
    using: mcp("brave-search")
}
```

`using:` values: `fetch`, `playwright`, `mcp("server-name")`, `baml("FunctionName")`, `bash`

### 4.2 Dynamic reasoning: `reason {}` block

For cases where synthesized static code is not sufficient — the next step depends on runtime LLM interpretation of previous results.

```
workflow ResearchAndDecide(query: string) -> Decision {
    let raw: SearchResult = execute Searcher.run(query: query)

    reason {
        using:       Writer
        input:       raw
        goal:        "Analyze the results and decide the best course of action"
        output_type: Decision
        bind:        decision
    }

    return decision
}
```

The `reason {}` block compiles to a TypeScript function that:
1. Calls the declared agent's LLM at runtime with the goal as prompt
2. Passes the `input` value serialized as context
3. Validates the response against `output_type` schema (BAML or Zod)
4. Retries up to 3x on schema validation failure

This is the **only place** where an LLM runs at execution time. Everything else is synthesized code.

### 4.3 Synthesizer declaration

```
synthesizer DefaultSynth {
    client = MyClaude
    temperature = 0.1
}

// Can declare multiple synthesizers per tool if needed
tool WebSearch(query: string) -> SearchResult {
    using:       fetch
    synthesizer: DefaultSynth
}
```

If no `synthesizer:` is specified on a tool, the first declared `synthesizer` block is used as default.

### 4.4 Agent declaration (updated role)

Agents now serve two roles:
1. **Synthesis context**: tells the Synthesis Pass what capabilities and constraints apply when generating code for this agent's tools
2. **Runtime reasoning**: used in `reason {}` blocks for dynamic LLM calls

```
agent Writer {
    client      = MyClaude
    system_prompt = "You are a concise technical writer."
    tools       = [WebSearch]
    settings    = {
        max_steps:   3,
        temperature: 0.2
    }
}
```

The `tools` field tells the synthesis pass which tools this agent's generated code can import and call.

---

## 5. Generated TypeScript Structure

```
generated/
├── types.ts                    # All type interfaces (auto-generated, no LLM)
├── tools/
│   ├── WebSearch.ts            # Synthesized implementation
│   ├── Screenshot.ts           # Synthesized implementation
│   └── index.ts                # Re-exports all tools (auto-generated)
├── workflows/
│   ├── Summarize.ts            # Generated from step AST (deterministic)
│   └── ResearchAndDecide.ts    # Generated, includes reason() call
├── runtime/
│   └── reason.ts               # Auto-generated: LLM runtime call for reason{} blocks
├── __tests__/
│   ├── WebSearch.contract.test.ts   # Auto-generated contract tests
│   ├── Screenshot.contract.test.ts
│   └── WebSearch.behavior.test.ts   # Generated from test{} blocks in .claw
└── bin/
    ├── Summarize.js            # Bundled artifact (esbuild, no node_modules)
    └── ResearchAndDecide.js
```

### Example: generated `WebSearch.ts` (synthesized)

```typescript
// generated/tools/WebSearch.ts
// SYNTHESIZED by claw build — do not edit manually
// Source: demo.claw → artifact hash sha256:a3f9...
import fetch from 'node-fetch';
import type { SearchResult } from '../types.js';

export async function WebSearch(inputs: { query: string }): Promise<SearchResult> {
  const response = await fetch(
    `https://api.duckduckgo.com/?q=${encodeURIComponent(inputs.query)}&format=json`
  );
  const data = await response.json() as any;
  return {
    url:        data.AbstractURL ?? data.Results?.[0]?.FirstURL ?? '',
    snippet:    data.Abstract    ?? data.Results?.[0]?.Text     ?? '',
    confidence: data.Abstract ? 0.9 : 0.5,
  };
}
```

### Example: generated `Summarize.ts` (deterministic from AST — no LLM)

```typescript
// generated/workflows/Summarize.ts
// GENERATED from step AST — deterministic, no synthesis
import { WebSearch } from '../tools/index.js';
import type { Summary } from '../types.js';

export async function Summarize(inputs: { topic: string }): Promise<Summary> {
  const result = await WebSearch({
    query: `Write a concise summary about: ${inputs.topic}`
  });
  return result;
}
```

### Example: generated `ResearchAndDecide.ts` (with reason block)

```typescript
// generated/workflows/ResearchAndDecide.ts
import { WebSearch } from '../tools/index.js';
import { reason }    from '../runtime/reason.js';
import type { Decision, SearchResult } from '../types.js';

export async function ResearchAndDecide(inputs: { query: string }): Promise<Decision> {
  const raw: SearchResult = await WebSearch({ query: inputs.query });

  const decision: Decision = await reason<SearchResult, Decision>({
    agent:       'Writer',
    input:       raw,
    goal:        'Analyze the results and decide the best course of action',
    outputType:  'Decision',
  });

  return decision;
}
```

---

## 6. Test Tiers

### Tier 1 — Contract tests (always run, ~100-300ms total)

Auto-generated from type declarations. No user configuration needed. No network.

```typescript
// generated/__tests__/WebSearch.contract.test.ts
// AUTO-GENERATED — do not edit
import { describe, it, expect, vi } from 'vitest';
import { WebSearch } from '../tools/WebSearch.js';

vi.mock('node-fetch', () => ({
  default: vi.fn().mockResolvedValue({
    json: () => Promise.resolve({
      AbstractURL: 'https://example.com',
      Abstract:    'Example result',
    })
  })
}));

describe('WebSearch contract', () => {
  it('output matches SearchResult schema', async () => {
    const result = await WebSearch({ query: 'test' });
    expect(typeof result.url).toBe('string');
    expect(typeof result.snippet).toBe('string');
    expect(typeof result.confidence).toBe('number');
    expect(result.confidence).toBeGreaterThanOrEqual(0.0);
    expect(result.confidence).toBeLessThanOrEqual(1.0);
  });
});
```

### Tier 2 — Behavior tests (run when declared in .claw, ~1-3s)

Generated from `test {}` blocks. Uses mocked network, validates exact assertions.

```typescript
// generated/__tests__/WebSearch.behavior.test.ts
// Generated from test{} block in demo.claw
it('query "rust language" returns non-empty url and snippet', async () => {
  const result = await WebSearch({ query: 'rust language' });
  expect(result.url).not.toBe('');
  expect(result.snippet).not.toBe('');
});
```

### Tier 3 — E2E tests (`claw verify` only, 5-30s)

Real network and browser. Never blocks `claw build`. Run in CI or manually.

---

## 7. Re-synthesis on Failure

If tests fail after synthesis:

1. Retry synthesis up to 3 times, passing the failing test output back to the Synthesis Pass as context:
   ```
   Previous attempt failed this test:
     input: { query: "rust language" }
     expected: url to be non-empty
     received: url = ""

   Revise the implementation to pass this test.
   ```
2. On 3rd failure: hard error with a diff showing the last generated code and failing assertions
3. User adjusts the `.claw` declaration or `test {}` expectations and rebuilds

---

## 8. Capability Primitives

| `using:` value | Runtime | What it generates |
|---|---|---|
| `fetch` | node-fetch | HTTP request function |
| `playwright` | @playwright/test | Browser automation script |
| `mcp("name")` | @modelcontextprotocol/sdk | MCP client call |
| `baml("Fn")` | @boundaryml/baml | Typed LLM extraction call |
| `bash` | node:child_process | Shell command execution |

`playwright` tools are skipped in Tier 1/2 tests (mock mode) and only run in `claw verify`.

---

## 9. Backward Compatibility

`invoke: module(...).function(...)` continues to work. It is treated as a static tool with no synthesis — the compiler emits a direct import. This allows existing projects to migrate incrementally.

The `--lang opencode` flag continues to emit command files + MCP server for projects that want OpenCode as runtime. The synthesis pipeline is opt-in via `using:` on tool declarations.

### Tool path decision rule (invariant — R2-05)

For every `tool` declaration the compiler applies exactly this decision:

| Tool has | Build path |
|---|---|
| `invoke:` only | MCP path (Spec 26) |
| `using:` only | Synthesis path (this spec) |
| Neither | Compiler error: `tool 'X' must declare either invoke: or using:` |
| Both | Compiler error: `tool 'X' cannot declare both invoke: and using:` |

---

## 10. Grammar Additions for Synthesis Constructs (R1-01, R1-02, R1-03)

These PEG rules extend Spec 03. The base grammar is unchanged; these are additive.

```peg
// Top-level document gains synthesizer_decl
document = _{ SOI ~ (import_decl | type_decl | client_decl | tool_decl |
               synthesizer_decl | agent_decl | workflow_decl |
               listener_decl | test_decl | mock_decl)* ~ EOI }

// Synthesizer declaration
synthesizer_decl = { "synthesizer" ~ identifier ~ "{" ~ synthesizer_prop+ ~ "}" }
synthesizer_prop = {
    ("client"      ~ "=" ~ identifier) |
    ("temperature" ~ "=" ~ number_lit) |
    ("max_tokens"  ~ "=" ~ number_lit)
}

// Tool block gains using:, test{}, synthesizer: alongside existing invoke:
tool_block_item = {
    ("invoke"      ~ ":" ~ invoke_expr) |
    ("using"       ~ ":" ~ using_expr)  |
    ("synthesizer" ~ ":" ~ identifier)  |
    test_block
}
tool_decl = { "tool" ~ identifier ~ "(" ~ tool_args? ~ ")" ~ ("->" ~ data_type)? ~
              ("{" ~ tool_block_item* ~ "}")? }

// using: values
using_expr = {
    "fetch" | "playwright" | "bash" |
    ("mcp"  ~ "(" ~ string_lit ~ ")") |
    ("baml" ~ "(" ~ string_lit ~ ")")
}

// test {} block inside tool declarations
test_block = { "test" ~ "{" ~ test_input ~ test_expect ~ "}" }
test_input  = { "input"  ~ ":" ~ "{" ~ (identifier ~ ":" ~ expr ~ ","?)+ ~ "}" }
test_expect = { "expect" ~ ":" ~ "{" ~ (identifier ~ ":" ~ expect_op ~ ","?)+ ~ "}" }
expect_op = {
    "!empty" |
    (">" ~ number_lit) |
    ("<" ~ number_lit) |
    (">=" ~ number_lit) |
    ("<=" ~ number_lit) |
    ("==" ~ expr) |
    ("matches" ~ string_lit)
}

// reason {} block inside workflow bodies
reason_stmt = {
    "reason" ~ "{" ~
    ("using"       ~ ":" ~ identifier      ~ ","?) ~
    ("input"       ~ ":" ~ identifier      ~ ","?) ~
    ("goal"        ~ ":" ~ string_lit      ~ ","?) ~
    ("output_type" ~ ":" ~ data_type       ~ ","?) ~
    ("bind"        ~ ":" ~ identifier      ~ ","?) ~
    "}"
}

// statement gains reason_stmt
statement = {
    let_stmt | for_stmt | if_stmt | try_stmt | execute_stmt |
    return_stmt | continue_stmt | break_stmt | assert_stmt |
    reason_stmt | expr
}
```

---

## 11. Artifact Step Serialization — Full Control Flow (R1-07, R2-01)

All workflow statement types serialize to artifact `steps[]`. The type binding from `let` declarations is preserved.

```json
// let raw: SearchResult = execute Searcher.run(query: query)
{ "kind": "tool_call", "tool": "Searcher", "bind": "raw", "bind_type": "SearchResult",
  "args": { "query": { "ref": "query" } } }

// reason {} block
{ "kind": "reason", "agent": "Writer", "input": "raw", "input_type": "SearchResult",
  "goal": "...", "output_type": "Decision", "bind": "decision" }

// for (item in items) { ... }
{ "kind": "for_loop", "item": "item", "iterable": { "ref": "items" },
  "item_type": "SearchResult", "body": [ /* nested steps */ ] }

// if (condition) { ... } else { ... }
{ "kind": "if", "condition": { "op": "==", "left": { "ref": "x" }, "right": { "lit": true } },
  "then": [ /* steps */ ], "else": [ /* steps */ ] }

// try { ... } catch (e: AgentExecutionError) { ... }
{ "kind": "try_catch", "body": [ /* steps */ ],
  "catch_var": "e", "catch_type": "AgentExecutionError", "handler": [ /* steps */ ] }

// return result
{ "kind": "return", "value": { "ref": "result" }, "value_type": "Summary" }
```

The `bind_type` and `value_type` fields are populated by the semantic analyzer during Stage 1. The TypeScript workflow generator uses these to emit correct type annotations — no type inference is required at synthesis time for workflows.

---

## 12. Agent Dual-Client Model (R1-06)

Agents serve two distinct roles that use DIFFERENT client configurations:

**Role 1 — Synthesis context** (build time):
The `synthesizer {}` block on a tool (or the default synthesizer) calls the synthesis LLM. The agent's `client` field is NOT used for synthesis.

**Role 2 — Runtime reasoning** (`reason {}` blocks, execution time):
When an agent is referenced in `reason {}`, the agent's own `client` field is the LLM that handles the reasoning call at runtime.

In the artifact:
```json
"agents": [{
  "name": "Writer",
  "runtime_client": "MyClaude",       // used by reason{} at execution time
  "system_prompt": "...",
  "tools": ["WebSearch"]
  // NOTE: no synthesis_client here — synthesis client comes from synthesizer_decl
}]
```

`dynamic_reasoning` is **derived by the compiler** (set to `true` if the agent appears in any `reason {}` block in the document). It is NOT a user-declared field.

---

## 13. `reason {}` Input Serialization (R2-03)

When generated `runtime/reason.ts` passes the `input` value to the LLM, it serializes as:

- Scalar types (`string`, `int`, `float`, `bool`): JSON primitive
- Object types: pretty-printed JSON object
- List types: JSON array

The serialized input is injected into the LLM prompt as:
```
CONTEXT (type: SearchResult):
{"url": "https://...", "snippet": "...", "confidence": 0.9}

GOAL: Analyze the results and decide the best course of action

Respond with valid JSON matching this schema: {"field": type, ...}
```

Output is validated with Zod schema generated from the declared `output_type`. On validation failure, retries up to 3x with the schema error appended to the prompt.

---

## 14. `using: baml(...)` vs Spec 18/31 BAML (R1-12)

These are distinct and do NOT conflict:

| | Spec 18/31 BAML | Spec 32 `using: baml(...)` |
|---|---|---|
| What it is | Generates `.baml` source files from tool declarations | Generates TypeScript that calls an already-generated BAML client |
| When it runs | `claw build` → emits `generated/baml_src/*.baml` | Synthesis Pass → emits `generated/tools/X.ts` importing `baml_client/` |
| Prerequisite | None | `npx baml-cli generate` must have already run |
| Use case | Tool internals need LLM-backed typed extraction | Tool calls a named BAML function in an existing `baml_client/` |

`using: baml("ExtractData")` in a tool declaration means: "synthesize TypeScript that imports and calls `b.ExtractData(...)` from the generated `baml_client/`. The BAML function `ExtractData` must exist (either hand-written or generated by the Spec 18/31 pipeline)."

---

## 15. Mocking Systems — Clear Separation (R1-09)

Two mocking systems exist with non-overlapping scopes:

**System 1 — DSL `mock {}` blocks** (Spec 08/17):
- Scope: offline `claw test` runs of the Rust AST interpreter
- Purpose: intercept `execute Agent.run(...)` calls in test blocks
- Lives in: `.claw` source file
- Does NOT apply to synthesized TypeScript

**System 2 — Capability `mock_strategy`** (this spec):
- Scope: vitest Tier 1/2 tests of synthesized TypeScript tools
- Purpose: mock fetch, exec, MCP clients so tools run without network
- Lives in: `capability_registry` in the artifact, applied by the test generator
- Does NOT apply to DSL test blocks / AST interpreter

`claw test` (Spec 08) runs DSL test blocks via AST interpreter — System 1.
`claw build` runs vitest contract tests against synthesized tools — System 2.
These commands test different things and do not share mock state.

---

## 16. Synthesis Pass Invocation from Rust (R1-04)

The Rust compiler cannot directly call a TypeScript `SynthesisModelAdapter`. Invocation is via **child process with stdin/stdout JSON protocol**:

```
claw build
  │
  ├── Stage 1: Rust emits artifact.clawa.json
  │
  ├── Stage 2: Rust spawns `node generated/synth-runner.js`
  │     │
  │     │  stdin: SynthesisRequest JSON (one per tool, newline-delimited)
  │     │  stdout: SynthesisResponse JSON (one per tool, newline-delimited)
  │     │  stderr: progress/error logs (printed to terminal)
  │     │
  │     └── synth-runner.js is auto-generated by claw compile (Stage 1)
  │           It imports the user's synthesizer adapter and processes requests
  │
  └── Rust reads stdout responses, writes TypeScript files, proceeds to Stage 3
```

`generated/synth-runner.js` is generated by Stage 1 (deterministically, no LLM). It contains the synthesis adapter wiring for the configured provider. Rust controls the process lifecycle — kills it if synthesis exceeds a configurable timeout (default: 120s per tool).

**Synthesis concurrency (R2-02):** Tools within a workflow are independent. Stage 2 sends ALL tool synthesis requests to `synth-runner.js` simultaneously (newline-delimited stream). The runner processes them concurrently (bounded by `Promise.all` with concurrency limit of 4). Workflow TypeScript is generated only after all tool synthesis completes.

---

## 17. esbuild Invocation (R1-05)

Stage 4 invokes esbuild as a child process:

```bash
node_modules/.bin/esbuild \
  generated/workflows/Summarize.ts \
  --bundle \
  --platform=node \
  --target=node18 \
  --format=esm \
  --outfile=generated/bin/Summarize.js \
  --external:playwright  # playwright is never bundled — too large
```

esbuild must be available in the project's `node_modules`. `claw init` adds it to `package.json` `devDependencies`. If esbuild is not found, Stage 4 emits a clear error: `"claw bundle requires esbuild. Run: npm install"`.

---

## 18. Synthesis Cache (R3-02, R3-04)

Stage 2 caches synthesis results keyed on:
```
cache_key = sha256(source_hash + synthesizer_model + reference_library_version)
```

Cache location: `.claw-cache/synthesis/{cache_key}/{tool_name}.ts`

Behavior:
- Cache hit → skip LLM call, use cached TypeScript, still run tests (Stage 3)
- Cache miss + online → synthesize, cache result
- Cache miss + offline → error: `"Synthesis cache miss for 'WebSearch'. Run claw build with network access to populate cache, or commit generated/tools/ to version control."`

**Commit recommendation (R3-03):** Treat `generated/tools/` like generated protobuf — commit it to version control. This gives deterministic builds offline, makes synthesis changes visible in diffs, and allows rollback. Add to `.gitignore` docs as opt-out, not opt-in.

---

## 19. CLI Changes Required (R1-13)

`claw test` is overloaded between old and new behaviors. Resolution:

| Command | Behavior |
|---|---|
| `claw test` | Runs DSL test blocks via Rust AST interpreter (Spec 08). Unchanged. |
| `claw build` | Runs Tier 1/2 vitest tests as part of the synthesis pipeline. |
| `claw verify` | Runs Tier 3 E2E tests (real network/browser). Never auto-run. |

`claw test` and `claw build` test different things. They do not conflict.

---

## 20. Error Cases (R1-14, R3-01, R2-04)

**Missing synthesizer:**
```
error: tool 'WebSearch' uses `using: fetch` but no synthesizer is declared.
  Add a synthesizer block to demo.claw:

  synthesizer DefaultSynth {
      client = MyClaude
      temperature = 0.1
  }

  See: https://docs.claw-lang.dev/synthesis
```

**Synthesis failed after 3 retries:**
```
error: synthesis failed for tool 'WebSearch' after 3 attempts.

  Last attempt:
  ─── generated/tools/WebSearch.ts (attempt 3) ───────────────────────
  [full generated code]
  ─────────────────────────────────────────────────────────────────────

  Failing tests:
  ✗ output.url must be non-empty string
    received: url = ""

  Options:
  1. Adjust the test {} expectations in demo.claw
  2. Add a reference: field to guide synthesis
  3. Try a more capable synthesizer model
```

Failed synthesis artifacts ARE written to `generated/tools/.failed/WebSearch.attempt3.ts` so the user can inspect them.

**Synthesis timeout:**
Default 120s per tool. Configurable in `claw.json`:
```json
{ "synthesis": { "timeout_ms": 180000, "max_retries": 3, "concurrency": 4 } }
```

---

## 21. Agent Inheritance in Synthesis (R1-16)

When `SeniorResearcher extends Researcher`:
- The Synthesis Pass receives the MERGED tool list: `Researcher.tools ∪ SeniorResearcher.tools`
- The `synthesizer:` on individual tools is respected — inheritance does not override per-tool synthesizer
- The `system_prompt` is the child's prompt (NOT inherited); if absent, parent's prompt is used
- `reason {}` blocks referencing `SeniorResearcher` use `SeniorResearcher.client` at runtime

---

## 22. Workflow Generation Dependency Order (R2-06, R2-08)

Stage 2 emits files in this strict order:
1. `generated/types.ts` — always first, deterministic, no LLM
2. `generated/tools/*.ts` — synthesized in parallel (concurrency 4)
3. `generated/runtime/reason.ts` — deterministic, no LLM
4. `generated/workflows/*.ts` — deterministic from step AST, imports already-written tools

If any tool in step 2 fails all retries → Stage 2 aborts. Workflow generation (step 4) does NOT run with placeholder imports. The error output names which tool failed and why.
