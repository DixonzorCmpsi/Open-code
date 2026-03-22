# Spec 42: OpenCode as the Synthesis Engine

**Status:** ACTIVE — Supersedes the generic `synth-runner.js` approach in Spec 32 §16.
**Depends on:** Spec 32 (pipeline architecture), Spec 25 (OpenCode integration baseline)

---

## 0. The Core Insight

Spec 32 defined the synthesis pipeline correctly but left the synthesis layer as a generic stdin/stdout bridge (`synth-runner.js`) with swappable adapters. That approach works but misses why the synthesis pass is reliable.

The synthesis pass is reliable because **OpenCode is an agentic coding engine** — not a chat completion endpoint. It has:
- Full bash and computer access to verify what it writes actually runs
- `descode` for automated security analysis of generated code
- Sub-agent spawning for tools that require multi-step reasoning to implement
- Session memory so synthesis of related tools shares context
- Chat ops introspection to see exactly what was reasoned and why

The generic adapter approach would treat OpenCode the same as calling `claude-sonnet` via raw API. That throws away everything that makes the synthesis pass trustworthy.

**This spec replaces the adapter model.** OpenCode is the first-class synthesis engine. Other LLMs are fallbacks for environments where OpenCode is not available.

---

## 1. Revised Architecture

```
.claw source
    │
    ▼  Stage 1: compile  (Rust, deterministic, no LLM)
.clawa artifact  ──────────────────────────────────────┐
    │                                                   │
    ▼  Stage 2: synthesize  (OpenCode agentic engine)  │
    │                                                   │
    │  ┌────────────────────────────────────────────┐  │
    │  │  OpenCode Synthesis Session                │  │
    │  │                                            │  │
    │  │  ┌──────────────────┐                     │  │
    │  │  │  Task: implement │ ← structured from   │  │
    │  │  │  tool X per spec │   .clawa, not prose │◄─┘
    │  │  └──────────────────┘                     │
    │  │          │                                 │
    │  │          ▼  agentic coding loop            │
    │  │  writes TypeScript → runs it via bash      │
    │  │  verifies output matches declared types    │
    │  │          │                                 │
    │  │          ▼  descode security sub-agent     │
    │  │  audits generated code for vulnerabilities │
    │  │  blocks if: shell injection, credential    │
    │  │  leak, unsafe eval, unvalidated input      │
    │  │          │                                 │
    │  │          ▼  contract tests (vitest)        │
    │  │  auto-generated from .claw type schema     │
    │  │  pass → accept │ fail → retry (max 3)      │
    │  └────────────────────────────────────────────┘
    │
    ▼  Stage 3: bundle  (esbuild, deterministic)
deterministic TypeScript bundle
    │
    ▼  Stage 4: execute  (OpenCode or node directly)
output + artifact placement
```

---

## 2. Why OpenCode for Synthesis

### 2.1 Agentic coding vs. completion

A raw LLM completion call for synthesis produces code once and hopes it's right. OpenCode runs a full agentic loop:

1. Reads the `.clawa` tool spec
2. Writes an initial TypeScript implementation
3. **Runs it via bash** — actually executes the code against the declared test inputs
4. Observes the output — does it match the declared return type?
5. Iterates until it does, or exhausts retries

This is the same loop a senior engineer uses when writing a new function. The bash access is not incidental — it is what makes synthesis self-correcting without requiring Claw to implement a test harness from scratch.

### 2.2 descode as the security gate

After synthesis produces passing TypeScript, OpenCode's `descode` sub-agent runs a security audit before the code is accepted into the bundle.

`descode` checks for:
- Shell injection via unescaped string interpolation in `bash` tools
- Credential leaks — hardcoded API keys, tokens written to disk
- `eval()` / `Function()` usage with external input
- Unvalidated input passed directly to network calls
- Path traversal in file system tools

If `descode` flags an issue, the synthesis loop is told exactly what the security problem is and retries with that constraint. The generated code is **never accepted with unresolved security findings**.

This makes every tool synthesized via `using:` audited by default — no separate security review step.

### 2.3 Sub-agents for complex tools

Some tools require multi-step reasoning to implement correctly:
- A `using: playwright` tool that navigates a complex multi-page flow
- A `using: mcp("some-server")` tool where the right MCP call sequence isn't obvious
- A tool whose behavior depends on inspecting a live API response to understand the schema

OpenCode can spawn a sub-agent to research and reason about these before writing the implementation. The sub-agent's findings are passed as context to the synthesis loop.

In `.claw`, complex tools can signal this:

```
tool ExtractInvoiceData(pdf_path: string) -> Invoice {
    using: bash
    synthesize {
        strategy: "research_first"
        note:     "PDF parsing — check pdftotext availability before choosing approach"
    }
}
```

`strategy: "research_first"` tells OpenCode to spawn a research sub-agent before writing. Without this hint, OpenCode uses its default single-pass synthesis.

### 2.4 Session memory across tools

OpenCode maintains session state. When synthesizing multiple tools in the same `.claw` file, the session carries:
- Which utilities were imported (avoids conflicting imports across tools)
- Which helper functions were shared (the synthesizer can reuse across tools)
- Which patterns succeeded (a fetch-based tool that worked informs the next)

This matters for consistency. Without session memory, two fetch-based tools in the same file might use different HTTP libraries. With it, they converge on the same pattern.

### 2.5 Chat ops introspection

After synthesis, `claw build` can emit a synthesis report showing what OpenCode reasoned for each tool:

```
[claw] synthesis report: generated/synthesis-report.md
  WebSearch.ts      ✓  3 agentic steps, 0 security findings
  FetchPage.ts      ✓  5 agentic steps, 1 finding resolved (input sanitization)
  ExtractData.ts    ✓  2 agentic steps, 0 security findings
```

The full report includes OpenCode's reasoning trace per tool — visible in `generated/synthesis-report.md`. Developers can audit exactly what the model did and why.

---

## 3. The `.clawa` → OpenCode Task Format

Spec 32 §3 defines the `.clawa` JSON format. This spec defines how each tool entry becomes a structured OpenCode task — not a prose prompt.

### 3.1 Task structure

For each tool with `using:`, the Rust compiler emits a task entry into `.clawa`:

```json
{
  "synthesis_tasks": [
    {
      "tool": "WebSearch",
      "task_type": "implement_tool",
      "spec": {
        "signature":    "WebSearch(inputs: { query: string }): Promise<SearchResult>",
        "return_type":  {
          "name": "SearchResult",
          "fields": [
            { "name": "url",        "type": "string" },
            { "name": "snippet",    "type": "string" },
            { "name": "confidence", "type": "float", "constraints": [{"min": 0.0}, {"max": 1.0}] }
          ]
        },
        "capability":   "fetch",
        "constraints":  ["no eval", "validate all inputs", "handle network errors"],
        "test_cases": [
          {
            "input":  { "query": "rust language" },
            "expect": { "url": "!empty", "snippet": "!empty", "confidence": {"range": [0.0, 1.0]} }
          }
        ]
      },
      "strategy": "default",
      "security_gate": true
    }
  ]
}
```

### 3.2 What OpenCode receives

OpenCode receives this task entry as a **structured coding task** — equivalent to a well-specified GitHub issue, not a free-form chat message. The task contains:

- Exact TypeScript signature to implement
- Full return type schema with field-level constraints
- Capability it must use (`fetch`, `playwright`, `bash`, etc.)
- Explicit constraints (security requirements, error handling requirements)
- Test cases to validate against

OpenCode does not need to infer any of this from prose. The `.claw` schema IS the specification.

### 3.3 Task delivery mechanism

The Rust compiler spawns OpenCode in headless task mode:

```
claw build
  │
  ├── Stage 1: emit artifact.clawa.json
  │
  ├── Stage 2: spawn opencode session
  │     command: opencode task --file generated/artifact.clawa.json --headless
  │     output:  generated/tools/*.ts  (one file per tool)
  │              generated/synthesis-report.md
  │     exit 0 = all tools synthesized and passed descode + contract tests
  │     exit 1 = synthesis failure — error output names the failing tool
  │
  └── Stage 3-4: bundle (unchanged from Spec 32)
```

`--headless` suppresses the TUI. Progress is streamed to stderr. The session is ephemeral — it exits after all tasks complete.

---

## 4. Execution Layer — OpenCode Runs the Outputs

After synthesis and bundling, **OpenCode is also the execution engine** for workflows that need full computer access.

### 4.1 Two execution modes

**Mode 1 — Direct node execution** (default, no OpenCode needed at runtime):
```bash
node generated/bin/Summarize.js --arg topic="quantum computing"
```
This is the current path. Works for workflows whose tools are pure HTTP/file operations.

**Mode 2 — OpenCode execution** (for workflows with `using: playwright`, `using: bash`, or `reason {}` blocks):
```bash
opencode run --workflow Summarize --arg topic="quantum computing"
```

OpenCode handles:
- `playwright` tools that need a real browser
- `bash` tools that need shell access
- `reason {}` blocks that need a live LLM call at execution time
- Artifact placement (the `artifact {}` block in the workflow)
- Streaming output to the terminal

The decision is automatic: `claw run` checks the bundle manifest for capability requirements. If any tool in the workflow requires `playwright`, `bash`, or contains a `reason {}` block, it routes through OpenCode. Otherwise it runs `node` directly.

### 4.2 Artifact placement at execution time

The `artifact {}` block declared in a workflow:

```
workflow FindInfo(topic: string) -> SearchResult {
    artifact {
        format = "json"
        path   = "~/Desktop/claw-results/${topic}.json"
    }
    ...
}
```

At execution time, OpenCode handles the file write — it has the full filesystem access needed to create directories, expand `~`, and write the artifact. For direct node execution, the generated bundle handles this itself (as currently implemented).

### 4.3 `reason {}` blocks at execution time

`reason {}` is the one place an LLM runs at execution time. When a workflow hits a `reason {}` block:

```
reason {
    using:       Analyst
    input:       raw_data
    goal:        "Determine if this data indicates a security incident"
    output_type: IncidentDecision
    bind:        decision
}
```

In OpenCode execution mode, this becomes a live OpenCode sub-task:
- Input value is passed as structured context (not prose)
- The `Analyst` agent's `system_prompt` and `client` config are used
- Output is validated against `IncidentDecision` Zod schema
- If validation fails: retry up to 3x, passing the schema error as feedback
- Session memory is available — the `reason {}` call can reference earlier workflow state

In direct node execution, `reason.ts` makes a raw API call (Anthropic/Ollama) — same behavior, no OpenCode dependency.

---

## 5. The Developer Mental Model

From the developer's perspective, the full system is:

```
You write:    intent in .claw (what you want, typed)
              ↓
Rust compiles: structured spec (.clawa)
              ↓
OpenCode implements: deterministic TypeScript (using its full coding capabilities)
              descode audits it
              tests gate it
              ↓
You get:      a bundle that runs forever with no LLM at execution time
              (unless you explicitly put reason {} where you need runtime reasoning)
              ↓
OpenCode runs: the bundle (computer access, browser, bash, artifact placement)
```

The `.claw` language is the **contract** between your intent and OpenCode's synthesis. The tighter and more typed the `.claw` schema, the less OpenCode needs to infer, the more reliable the output.

**You never write a prompt for synthesis.** The schema IS the prompt. This is why the synthesis output is more reliable than asking an LLM to "write a web scraper" in chat.

---

## 6. Fallback: OpenCode Not Available

For environments where OpenCode is not installed (CI, Docker, headless servers):

```
claw build --synth-backend=api
```

This falls back to the Spec 32 generic `synth-runner.js` adapter model — raw API calls to the configured synthesizer. The security gate (`descode`) is skipped in this mode, but contract tests still run.

A warning is printed:
```
warning: OpenCode not found — using API synthesis fallback.
  Security gate (descode) is disabled in this mode.
  Install OpenCode for full synthesis quality: https://opencode.ai
```

The `--synth-backend` default is `opencode` if it is on PATH, `api` otherwise.

---

## 7. Implementation Stages

### Stage A — OpenCode headless task protocol (prerequisite)
Define the exact `opencode task --headless` interface. What flags does it accept? What is the stdout/stderr contract? How does it signal completion vs. failure? This requires reading OpenCode's task API source.

### Stage B — `.clawa` synthesis task format
Extend the Spec 32 artifact format with `synthesis_tasks[]` (§3.1 above). Update the Rust compiler's `artifact.rs` to emit these entries for every tool with `using:`.

### Stage C — `claw build` task dispatch
Replace the `synth_runner.rs` stdin/stdout bridge with OpenCode task dispatch. Rust spawns `opencode task --file artifact.clawa.json --headless` and reads the output TypeScript files from disk.

### Stage D — descode integration
After each tool TypeScript is written, the synthesis session runs descode automatically. Map descode findings back to synthesis retry feedback. Define the security policy (which findings are blocking vs. warnings).

### Stage E — sub-agent strategy field
Parse `synthesize { strategy: "research_first" }` in the parser. Emit `"strategy": "research_first"` in the task JSON. OpenCode respects this when it supports task strategies.

### Stage F — execution routing
In `claw run`, inspect the bundle manifest for capability requirements. Route to `opencode run` or `node` accordingly.

### Stage G — synthesis report
After synthesis completes, emit `generated/synthesis-report.md` from OpenCode's session trace. Surface this in `claw build` output as a link.

---

## 8. What This Spec Does NOT Change

- The `.claw` DSL syntax — unchanged
- The `.clawa` JSON format from Spec 32 (extended, not replaced)
- The deterministic workflow TypeScript generation (no LLM, unchanged)
- The `reason {}` runtime behavior
- The `invoke:` → direct import path (no synthesis)
- The direct node execution path for non-computer-access workflows
- Spec 32's contract test tiers (Tier 1/2/3 unchanged)
