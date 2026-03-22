# Spec 41: BAML-Style TypeScript Codegen

**Status:** Specced 2026-03-20.
**Depends on:** Spec 38 (runtime.js patterns), Spec 39 (zero-dep fetch approach), Spec 37 (quality gate).

---

## 1. Problem

`claw build --lang ts` currently generates SDK wrappers that delegate to `@claw/sdk`:

```typescript
import { ClawClient } from "@claw/sdk";  // does not exist
export const FindInfo = async (..., options: { client: ClawClient }) =>
    options.client.executeWorkflow(...);   // cannot run
```

This is the wrong model. BAML's model is: the DSL compiles to a **self-contained client** — types, validation, LLM calls all baked in. You `import { FindInfo }` and call it. No gateway, no runtime package.

The goal: `.claw` compiles to `generated/claw/index.ts` that is both **importable** (use in any TS project) and **directly executable** (`npx tsx generated/claw/index.ts FindInfo --arg topic=...`).

---

## 2. No New DSL Constructs

Spec 41 adds no new Claw DSL syntax and no new AST nodes. The `.claw` language is unchanged. `grammar_examples`, `ast_defined`, and `parser_note` criteria are satisfied — this is a codegen-only change.

---

## 3. Generated File Structure

`claw build --lang ts` writes a single file: `generated/claw/index.ts`.

```
generated/
  claw/
    index.ts      ← the complete self-contained TypeScript client
```

The file has six sections in order:

```typescript
// 1. Imports — only node built-ins and zod
import { fileURLToPath } from "node:url";
import path from "node:path";
import fs from "node:fs/promises";
import { z } from "zod";

// 2. Type interfaces
export interface SearchResult { ... }

// 3. Zod schemas
export const SearchResultSchema = z.object({ ... });

// 4. Tool handlers — typed, module-based
async function callTool_WebSearch(args: { query: string }): Promise<SearchResult> { ... }
async function callTool(name: string, input: unknown): Promise<unknown> { ... }

// 5. Agent runner functions — raw fetch, typed return
async function runAgent_Researcher(task: string): Promise<string> { ... }

// 6. Exported workflow functions — the public API
export async function FindInfo(args: { topic: string }): Promise<SearchResult> { ... }

// 7. CLI entry — only fires when run directly, not when imported
if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  // --list / --arg parsing, same as runtime.js
}
```

---

## 4. Type Interfaces and Zod Schemas

Identical to the current `--lang ts` output — keep this section unchanged.

```typescript
export interface SearchResult {
    url: string;
    snippet: string;
    confidence_score: number;
}

export const SearchResultSchema = z.object({
    url: z.string(),
    snippet: z.string(),
    confidence_score: z.number(),
}).strict();
```

`zod` is the only npm dependency in the generated file. It is placed in `dependencies` (not `optionalDependencies`) because the generated code calls `.parse()` at runtime for type validation.

---

## 5. Tool Handler Pattern

Each tool declared with `invoke: module(...)` generates a typed handler:

```typescript
async function callTool_WebSearch(args: { query: string }): Promise<unknown> {
  const modPath = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../scripts/search.js");
  const real = await fs.realpath(modPath);
  const mod = await import(new URL(real, "file://").href);
  return await mod.run(args.query);
}

async function callTool(name: string, input: unknown): Promise<unknown> {
  switch (name) {
    case "WebSearch": return callTool_WebSearch(input as { query: string });
    default: throw { code: "E-RUN99", message: `unknown tool: ${name}` };
  }
}
```

If no tools are declared, `callTool` is emitted as a stub that always throws E-RUN99.

---

## 6. Agent Runner Pattern

Each agent generates a typed runner using raw fetch — same logic as `shared_js.rs` but in TypeScript syntax. The runner always returns `Promise<string>` (raw LLM text); the workflow function is responsible for parsing and validating.

**Ollama agent (provider = "local"):**

```typescript
async function runAgent_Researcher(task: string): Promise<string> {
  const host = process.env.OLLAMA_HOST ?? "http://localhost:11434";
  const messages: Array<{role: string; content: string | null; tool_calls?: unknown[]}> = [
    { role: "system", content: "You are a precise researcher." },
    { role: "user", content: task },
  ];
  let steps = 0;
  while (steps < 5) {
    steps++;
    const resp = await fetch(`${host}/v1/chat/completions`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ model: "qwen2.5:14b", messages, tools: [...], stream: false, temperature: 0.0 }),
    });
    if (!resp.ok) throw { code: "E-RUN04", message: `Ollama error ${resp.status}` };
    const data = await resp.json() as { choices: Array<{finish_reason: string; message: {content: string | null; tool_calls?: unknown[]}}>};
    const choice = data.choices?.[0];
    if (!choice) break;
    if (choice.finish_reason === "stop") return choice.message.content ?? "";
    if (choice.finish_reason === "tool_calls") { /* dispatch */ continue; }
    break;
  }
  return `Agent runAgent_Researcher reached max_steps (5) without finishing.`;
}
```

**Anthropic agent (provider = "anthropic"):**

Same pattern using `https://api.anthropic.com/v1/messages` with `x-api-key` header. Checks `process.env.ANTHROPIC_API_KEY` at call time, throws E-RT03 if missing.

---

## 7. Exported Workflow Functions

Each `workflow` declaration compiles to an exported typed async function. The function body executes the plan steps inline — no PLANS object, no interpreter loop. This is the key difference from `runtime.js`: the TypeScript output is fully static and type-checked.

```typescript
export async function FindInfo(args: { topic: string }): Promise<SearchResult> {
  const task = `Find the most relevant info about: ${args.topic}`;
  const output = await runAgent_Researcher(task);
  let result: SearchResult;
  try {
    result = SearchResultSchema.parse(typeof output === "string" ? JSON.parse(output) : output);
  } catch {
    throw { code: "E-RUN03", message: `type validation failed for SearchResult\n  got: ${output}` };
  }
  return result;
}
```

**Workflow with no return type:**

```typescript
export async function MyWorkflow(args: { input: string }): Promise<unknown> {
  const output = await runAgent_MyAgent(`Process: ${args.input}`);
  return output;
}
```

**Importing in a TS project:**

```typescript
import { FindInfo } from "./generated/claw/index.js";
const result = await FindInfo({ topic: "quantum computing" });
console.log(result.url);   // typed: string
```

---

## 8. CLI Entry Guard

The CLI block only fires when the file is run directly, not when imported. This is the same pattern Node.js uses for `require.main === module` in CJS, adapted for ESM:

```typescript
if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  const [,, workflowArg, ...rawArgs] = process.argv;

  if (!workflowArg || workflowArg === "--list") {
    const workflows = [
      { name: "FindInfo", requiredArgs: ["topic"], returnType: "SearchResult" },
    ];
    process.stdout.write(JSON.stringify(workflows) + "\n");
    process.exit(0);
  }

  function parseArgs(raw: string[]): Record<string, string> {
    const out: Record<string, string> = {};
    for (let i = 0; i < raw.length; i++) {
      if (raw[i] === "--arg" && i + 1 < raw.length) {
        const eq = raw[++i].indexOf("=");
        if (eq !== -1) out[raw[i].slice(0, eq)] = raw[i].slice(eq + 1);
      }
    }
    return out;
  }

  const cliArgs = parseArgs(rawArgs);
  (async () => {
    try {
      let result: unknown;
      switch (workflowArg) {
        case "FindInfo": result = await FindInfo(cliArgs as { topic: string }); break;
        default: process.stderr.write(JSON.stringify({ error: `workflow "${workflowArg}" not found`, code: "E-RUN01" }) + "\n"); process.exit(1);
      }
      process.stdout.write(JSON.stringify(result, null, 2) + "\n");
    } catch (err: unknown) {
      const e = err as { message?: string; code?: string };
      process.stderr.write(JSON.stringify({ error: e.message ?? String(err), code: e.code ?? "E-RUN99" }) + "\n");
      process.exit(1);
    }
  })();
}
```

---

## 9. Running the Generated TypeScript

**Option A — tsx (recommended, no compile step):**
```bash
npm install -g tsx
claw build --lang ts
npx tsx generated/claw/index.ts FindInfo --arg topic="quantum computing"
```

**Option B — Node.js 22+ (experimental, no install):**
```bash
node --experimental-strip-types generated/claw/index.ts FindInfo --arg topic="quantum computing"
```

**Option C — compile then run:**
```bash
npx tsc --moduleResolution bundler --target ES2022 --outDir dist generated/claw/index.ts
node dist/index.js FindInfo --arg topic="quantum computing"
```

`claw run --lang ts` (Spec 42) will wrap Option A automatically. For now, run directly.

**OpenCode integration:** Any `.opencode/command/` file can import:
```typescript
import { FindInfo } from "../generated/claw/index.js";
```
No MCP server needed. The workflow runs as native TypeScript.

---

## 10. Files Changed

| File | Change |
| --- | --- |
| `src/codegen/typescript.rs` | Full rewrite — replace SDK wrapper with BAML-style self-contained generator |
| `src/codegen/mod.rs` | No change — `generate_ts` signature unchanged |
| `src/bin/claw.rs` | No change — `--lang ts` path already wired |

No changes to `src/ast.rs`, `src/parser.rs`, `src/codegen/runtime.rs`, or any test file.

---

## 11. Error Codes

| Code | Level | Trigger |
| --- | --- | --- |
| E-RT03 | Generated TS | `ANTHROPIC_API_KEY` not set when Anthropic agent called |
| E-RUN01 | Generated TS CLI | Unknown workflow name passed as CLI arg |
| E-RUN03 | Generated TS | Zod parse of agent output fails against declared return type |
| E-RUN04 | Generated TS | Ollama or Anthropic API returns non-2xx status |
| E-RUN99 | Generated TS | Unknown tool name dispatched in callTool() |

All existing codes — no new codes introduced. Each is thrown as `{ code: "E-XXX", message: "..." }` consistent with `runtime.js`.

---

## 12. Behavior When Optional Inputs Are Absent

**No types declared:** No interfaces or schemas emitted. `callTool` return type becomes `Promise<unknown>` throughout.

**No tools declared:** `callTool` stub emitted that always throws E-RUN99. Tool-less agents work fine — they never call `callTool`.

**No agents declared:** Agent runners block is empty. Any workflow that calls `execute AgentName.run(...)` will fail to compile (semantic analysis rejects undefined agent reference — caught before codegen).

**No workflows declared:** No exported workflow functions emitted. CLI entry `--list` outputs `[]`. File is valid TypeScript that imports cleanly.

**No clients declared:** `resolve_client` falls back to `anthropic` provider with `claude-haiku-4-5-20251001` — same fallback as `runtime.rs` and `mcp.rs`.

---

## 13. Feature Interaction: --lang ts + BAML tools

If a tool uses `invoke: baml(...)`, the tool handler in the generated TypeScript cannot call the BAML client directly (BAML generates its own `baml_client/` directory separately). The tool handler emits a stub that throws:

```typescript
throw { code: "E-RUN99", message: "baml tool 'ExtractKeywords' requires running `npx @boundaryml/baml-cli generate` first" };
```

This is consistent with how `mcp.rs` handles BAML tools — both emit a handler that requires the BAML client to be generated separately.

---

## 14. Offline Behavior

**Anthropic agent, no API key:** `runAgent_<Name>` throws E-RT03 at call time with message: `"ANTHROPIC_API_KEY not set\n  export ANTHROPIC_API_KEY=sk-ant-..."`. The check is inside the agent runner, not at module import — importing the generated file does not require an API key.

**Ollama agent, Ollama not running:** `fetch` to `localhost:11434` throws a network error. The agent runner catches it and rethrows as E-RUN04 with hint: `"start Ollama with \`ollama serve\`"`.

**Both providers offline:** Same as above per provider. The generated TypeScript file itself always loads — all provider checks are deferred to call time.
