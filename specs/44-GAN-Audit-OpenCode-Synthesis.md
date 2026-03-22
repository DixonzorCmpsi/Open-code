# Spec 44: GAN Audit — OpenCode Synthesis Direction + NER/BAML Enrichment

**Status:** ACTIVE — Full audit of Spec 42/43 with live test findings. Covers downstream consequences, failure modes, and the NER+BAML enrichment layer.
**Audit date:** 2026-03-20
**Live test binary:** opencode v1.2.27 (TypeScript, not the Go source in `/opencode/`)

---

## Part 1: Live Test Findings

### What was confirmed

1. `opencode run "message" --dir /project -m ollama/qwen2.5-coder:7b` works as a non-interactive interface
2. The model IS called and responds — the `> build · qwen2.5-coder:7b` line confirms the agentic session starts
3. `--format json` emits a structured event stream (usable for parsing)
4. Model configuration is project-scoped — Ollama models are only available when `opencode.json` is in the working directory
5. No `--quiet` flag exists in v1.2.27 — Spec 43's `-q` flag assumption was based on the Go source (old version)

### Critical finding: MCP interference

Running the synthesis test prompt in the project directory produced:
```json
{ "name": "claw-tools_agent_Researcher", "arguments": { "task": "HELLO_WORLD" } }
```

The model called the project's MCP tool `agent_Researcher` instead of following the synthesis prompt. The project's `opencode.json` exposes `generated/mcp-server.js` as the `claw-tools` MCP server, and the model latched onto it.

**This is the most important finding of the audit.** It invalidates the Stage C approach as written.

### Interface corrections from v1.2.27

| Spec 43 assumption | Reality |
|---|---|
| `opencode -p "prompt" -q` | `opencode run "prompt"` |
| `-f json` → `{"response": "..."}` | `--format json` → NDJSON event stream |
| `--quiet` suppresses spinner | No `--quiet` flag; use `--format json` to suppress TUI |
| Go source describes current binary | Installed binary is TypeScript v1.2.27 — different CLI entirely |

---

## Part 2: GAN Audit — Synthesis Direction

### G — Gaps

**G1: MCP isolation not specified.**
Synthesis sessions must run without the project's MCP tools loaded. The current plan has OpenCode `--dir /project`, which loads `opencode.json` and its `mcp` section. The model will use MCP tools instead of following synthesis instructions. No mitigation was specified in Spec 42 or 43.

**G2: Synthesis speed with local models.**
qwen2.5-coder:7b took >60s to respond to a trivial prompt. A real synthesis task (write TypeScript, run it via bash, verify output types) will take 3-10 minutes per tool with a 7B local model. For a `.claw` file with 5 tools, synthesis would take 15-50 minutes. This is unusable as part of a build step.

**G3: Port conflict for concurrent synthesis.**
`opencode run` starts a local server on a random port. Multiple concurrent synthesis invocations (concurrency > 1) could conflict. The `--port` flag exists but Spec 43 didn't account for it.

**G4: No structured output contract.**
Spec 43 uses string matching for `SYNTHESIS_COMPLETE: ToolName`. This is fragile — the model might say "I'll output SYNTHESIS_COMPLETE: WebSearch now" as part of its reasoning, triggering a false positive. Or it might omit the sentinel entirely while still writing the file correctly.

**G5: descode not found in source.**
Spec 42 §2.2 describes descode as a sub-agent that audits generated code. This feature was not found in the opencode v1.2.27 codebase audit. It may be a planned feature, a different tool, or the name may differ. Stage D cannot proceed without confirming descode exists.

**G6: Synthesis binary version drift.**
The Go source in `/opencode/` is an old version. The installed binary (v1.2.27) is entirely different. Specs 42/43 were partially written against the Go source. Any Rust code that shells out to `opencode` must treat the CLI as an external contract that can change on upgrade.

**G7: No fallback when OpenCode hangs.**
If the model enters an infinite tool-call loop (as observed — the model kept calling `agent_Researcher`), `opencode run` does not exit. The Rust process spawning it would hang indefinitely. No timeout mechanism was specified.

### A — Assumptions that need validation

**A1: OpenCode's `--agent` flag can specify a synthesis-only agent.**
The `opencode run --agent <name>` flag exists. If a synthesis-specific agent config (no MCP, restricted tools, tailored system prompt) can be defined in `opencode.json`, it would solve the MCP isolation problem cleanly. This needs to be tested.

**A2: OpenCode writes files to `--dir`, not the calling process's cwd.**
When Rust spawns `opencode run --dir /project`, the assumption is that OpenCode's write tool creates files relative to `/project`. This needs verification — the bash tool uses `config.WorkingDirectory()` which should respect `--dir`.

**A3: The `--format json` stream contains the final text response.**
The JSON event stream format is documented as "raw JSON events" but the exact schema (event types, final text extraction) was not confirmed. Parsing the stream correctly requires knowing which event type carries the agent's final text.

**A4: BAML can parse OpenCode's structured output.**
The NER+BAML layer proposed in Part 3 assumes BAML can wrap an LLM call (or a stream of events) and extract typed output. This is BAML's core purpose and is confirmed by BAML's documentation.

### N — Non-obvious downstream consequences

**N1: Synthesis speed determines `.claw` viability.**
If synthesis takes 3-10 minutes per tool with local models, developers will not use `using:` syntax. They'll stick with `invoke: module(...)` (current direct codegen path) because it's instant. The synthesis pipeline only succeeds if synthesis is fast enough to be part of a normal build loop. This means cloud models (Anthropic/OpenAI) are required for synthesis in practice — local models are too slow. This has cost implications.

**N2: MCP isolation creates a two-opencode-config problem.**
To solve G1, Claw must either:
- Write a temp `opencode.json` (synthesis-only, no MCP) before synthesis runs, then restore it — risky if the process crashes mid-synthesis
- Use `--agent synthagent` where `synthagent` has no MCP tools — requires OpenCode to support agent-scoped MCP suppression

Either approach adds complexity that breaks if OpenCode's config format changes (version drift, G6).

**N3: The model's MCP tools are a double-edged sword.**
The MCP interference (G1) is actually useful for execution (`reason {}` blocks) but harmful for synthesis. The same binary needs to suppress MCP during synthesis and use MCP during execution. This means synthesis and execution are not symmetric — they require different OpenCode configurations.

**N4: Synthesis caching becomes critical for cost.**
If synthesis requires cloud models (N1), and each synthesis call costs ~$0.01-0.05 (Sonnet-class), a project with 10 tools costs $0.10-$0.50 per full rebuild. The synthesis cache (Spec 32 §18) is not optional — it's required to make the economics work. Cache must be keyed on tool spec hash so that unchanged tools are never re-synthesized.

**N5: The GAN audit loop itself needs synthesis.**
If Claw uses OpenCode to generate tool implementations, and those tool implementations have bugs, the retry loop (max 3 attempts) must produce useful feedback. Vague "it didn't work" feedback leads to 3 failed attempts and a hard error. The quality of the retry prompt is as important as the synthesis prompt.

**N6: Developer trust requires auditability.**
Synthesized code that developers can't inspect or understand will not be trusted. The synthesis report (Spec 42 §2.5) must be first-class, not an afterthought. Every tool synthesis must produce a readable record of: what the model did, what it tried, what passed/failed. Without this, developers will reject the feature.

---

## Part 3: NER + BAML Enrichment Layer

### What NER contributes

NER (Named Entity Recognition) applied to the synthesis pipeline extracts structured semantic information from tool declarations before the prompt is built. This makes the synthesis prompt more precise, reducing the model's need to guess.

**Where NER sits:** between the Rust compiler (Stage 1) and the synthesis prompt builder (Stage 2). It enriches the `.clawa` tool spec with extracted entities before OpenCode receives it.

#### NER extraction targets

For a tool declaration like:
```
tool FetchInvoice(invoice_id: string) -> Invoice {
    using: fetch
    // Calls the Stripe API at https://api.stripe.com/v1/invoices/{id}
    // Requires STRIPE_API_KEY env var
    // Returns 404 if not found
}
```

NER extracts:
- **API endpoint**: `https://api.stripe.com/v1/invoices/{id}` → adds to synthesis prompt as concrete URL pattern
- **Env var**: `STRIPE_API_KEY` → adds to constraints: "read from `process.env.STRIPE_API_KEY`, never hardcode"
- **Error condition**: `404 if not found` → adds to constraints: "handle 404 as a typed error, not a throw"
- **Service name**: `Stripe` → adds context: "this is a Stripe API integration"

Without NER, the synthesizer must infer all of this from the tool name alone. With NER, the synthesis prompt is:

```
Name:       FetchInvoice
Capability: fetch
API:        https://api.stripe.com/v1/invoices/{invoice_id}
Auth:       process.env.STRIPE_API_KEY (Bearer token)
Error case: 404 → return null or throw typed NotFoundError
```

This is the difference between "write a function that fetches an invoice" and "write a function that calls this specific URL with this auth header and handles this specific error". The model's output is proportionally more accurate.

#### NER library recommendation

**Compromise:** NER is implemented in TypeScript (runs as part of the `.clawa` emitter step, before synthesis). Use `compromise` — a zero-dependency NER library with 99KB bundle size.

```typescript
import nlp from 'compromise';

function extractEntities(text: string): ToolEntities {
  const doc = nlp(text);
  return {
    urls:     doc.urls().out('array'),
    envVars:  text.match(/[A-Z][A-Z0-9_]{2,}_KEY|[A-Z][A-Z0-9_]{2,}_TOKEN/g) ?? [],
    services: doc.organizations().out('array'),
    errors:   extractErrorPatterns(text),
  };
}
```

**What "Knwler" likely refers to:** This may be `knwl.js` (a discontinued NER/entity extraction library) or a typo for `compromise`. The recommendation here is `compromise` as it is actively maintained, has TypeScript types, and handles URLs, organizations, and patterns well.

**Where it runs:** As a TypeScript preprocessing step invoked by `claw compile` (Stage 1). Output is added to the `.clawa` artifact under `"extracted_entities"` per tool. Zero cost at synthesis time — happens during compilation.

### What BAML contributes

BAML (BoundaryML's typed LLM abstraction) solves two problems:

1. **Structured output from OpenCode** — instead of string-matching for `SYNTHESIS_COMPLETE`, BAML defines a typed schema for what a successful synthesis response looks like, and extracts it reliably from the model output.

2. **Deterministic alignment** — BAML uses constrained decoding or retry-with-schema-validation to ensure OpenCode's output matches the `.claw` declared types exactly. This is the "deterministic alignment with .claw declaratives" the user described.

#### BAML as the synthesis output parser

Define a BAML function that wraps the OpenCode output:

```baml
// baml_src/synthesis.baml

class SynthesisResult {
  status      "complete" | "failed" | "partial"
  tool_name   string
  file_path   string?
  warnings    string[]
  error_msg   string?
  tsc_passed  bool
  test_passed bool
}

function ParseSynthesisOutput(raw_output: string) -> SynthesisResult {
  client Haiku  // fast, cheap — this is parsing not generation
  prompt #"
    Parse this synthesis agent output and extract the structured result.

    Output: {{ raw_output }}

    Extract:
    - status: "complete" if SYNTHESIS_COMPLETE present, "failed" if SYNTHESIS_FAILED present, else "partial"
    - tool_name: the tool name mentioned after SYNTHESIS_COMPLETE or SYNTHESIS_FAILED
    - file_path: any file path mentioned as the written output
    - warnings: any warnings mentioned
    - error_msg: the REASON if SYNTHESIS_FAILED
    - tsc_passed: whether TypeScript compilation was mentioned as passing
    - test_passed: whether the smoke test was mentioned as passing

    {{ ctx.output_format }}
  "#
}
```

This means the Rust synthesis runner:
1. Calls `opencode run "..." --format json`
2. Accumulates the event stream into a full text response
3. Calls the BAML `ParseSynthesisOutput` function to get a typed `SynthesisResult`
4. Acts on the structured result — no string matching, no brittle sentinel parsing

**Why this is better than string matching:**
- The model might say "SYNTHESIS_COMPLETE" in a different position or format — BAML handles variations
- BAML extracts `tsc_passed` and `test_passed` separately — Claw knows exactly what passed and what didn't
- BAML retries if the parse fails — the result is always typed
- The `SynthesisResult` type can be extended without changing Rust code

#### BAML as the `.clawa` alignment gate

Before synthesis begins, BAML validates that the tool spec in `.clawa` is complete and unambiguous:

```baml
class SpecCompletenessCheck {
  is_complete     bool
  missing_fields  string[]
  ambiguities     string[]
  suggestions     string[]
}

function CheckToolSpec(spec: string) -> SpecCompletenessCheck {
  client Haiku
  prompt #"
    Evaluate this tool specification for completeness before code synthesis.
    A complete spec has: clear return type fields, at least one test case,
    no ambiguous capability requirements.

    Spec: {{ spec }}

    {{ ctx.output_format }}
  "#
}
```

If `is_complete = false`, `claw build` emits a warning with `suggestions` before synthesis starts:
```
warning: tool 'FetchInvoice' spec may be incomplete.
  ambiguities: ["which HTTP status codes are errors vs. empty results?"]
  suggestions: ["Add test case for 404 response", "Clarify error vs. null return"]
  Synthesis will proceed but results may be less accurate.
```

This catches underdefined tools before the model wastes 60 seconds producing wrong code.

#### BAML for type alignment

After synthesis writes `generated/tools/WebSearch.ts`, BAML verifies the exported interface matches the `.claw` declared return type:

```baml
class TypeAlignmentResult {
  aligned        bool
  mismatches     FieldMismatch[]
}

class FieldMismatch {
  field          string
  declared_type  string
  actual_type    string
}

function CheckTypeAlignment(
  declared_schema: string,
  generated_code: string
) -> TypeAlignmentResult {
  client Haiku
  prompt #"
    Check if this TypeScript function's return type exactly matches the declared schema.

    Declared schema: {{ declared_schema }}
    Generated code: {{ generated_code }}

    {{ ctx.output_format }}
  "#
}
```

This is a semantic check that `tsc --noEmit` cannot do — it verifies that the TypeScript types correspond to the intended `.claw` types, not just that TypeScript accepts the code.

---

## Part 4: Revised Architecture (Post-Audit)

### Synthesis isolation: the temp config approach

To solve MCP interference (G1), Claw writes a synthesis-scoped `opencode.json` to a temp directory before each synthesis session:

```json
// /tmp/claw_synth_<hash>/opencode.json — synthesis-only config
{
  "model": "anthropic/claude-sonnet-4-6",
  "agents": {
    "coder": {
      "model": "anthropic/claude-sonnet-4-6"
    }
  }
  // No "mcp" section — synthesis agent has NO project tools
}
```

Synthesis runs with `--dir /tmp/claw_synth_<hash>` but writes output to `<project_root>/generated/tools/`. The synthesis prompt explicitly states the absolute output path.

After synthesis, the temp directory is deleted. If the process crashes, the temp dir is cleaned up on next `claw build` via a lock file.

### Revised interface contract (v1.2.27)

```bash
# Synthesis invocation
opencode run "<synthesis_prompt>" \
  --dir /tmp/claw_synth_<tool>_<hash> \
  --format json \
  -m anthropic/claude-sonnet-4-6

# Output: NDJSON event stream on stdout
# Final text response extracted from event stream
# Written files: <project_root>/generated/tools/<ToolName>.ts
```

### Revised pipeline with NER + BAML

```
.claw source
    │
    ▼  Stage 1: compile (Rust)
    │  ├─ Parse AST
    │  ├─ Run NER on tool comments/descriptions (TypeScript subprocess)
    │  └─ Emit .clawa with extracted_entities per tool
    │
    ▼  Stage 1.5: spec validation (BAML CheckToolSpec)
    │  └─ Warn on ambiguous/incomplete specs
    │
    ▼  Stage 2: synthesize (OpenCode, isolated config)
    │  ├─ Write temp opencode.json (no MCP)
    │  ├─ Build synthesis prompt (NER entities injected)
    │  ├─ Run: opencode run "..." --dir /tmp/synth --format json
    │  ├─ Accumulate NDJSON event stream → raw text
    │  ├─ BAML ParseSynthesisOutput → SynthesisResult (typed)
    │  ├─ BAML CheckTypeAlignment → TypeAlignmentResult
    │  └─ On failure: structured retry with BAML-extracted error context
    │
    ▼  Stage 3: contract tests (vitest Tier 1/2)
    │
    ▼  Stage 4: bundle (esbuild)
```

---

## Part 5: Implementation Prompt

The following prompt can be used directly to implement the NER+BAML enrichment layer without breaking any existing patterns:

---

**IMPLEMENTATION PROMPT:**

```
You are implementing the NER + BAML enrichment layer for the Claw language compiler.
This is an ADDITIVE change — do not modify any existing codegen paths.

Repository: /Users/dixon.zor/Documents/Open-code

CONTEXT:
- `src/codegen/synth_runner.rs` — existing synthesis bridge (to be extended, not replaced)
- `specs/32-Code-Synthesis-Pipeline.md` — pipeline architecture
- `specs/43-OpenCode-Headless-Interface.md` — OpenCode interface contract
- `specs/44-GAN-Audit-OpenCode-Synthesis.md` — this audit (implementation guide)

TASK 1 — NER preprocessing (TypeScript, new file):
Create `scripts/ner_enrich.ts`:
  - Import `compromise` (add to package.json devDependencies)
  - Export: `enrichToolSpec(toolName: string, comments: string, spec: object): EnrichedSpec`
  - EnrichedSpec adds `extracted_entities: { urls, env_vars, services, error_patterns }` to the spec
  - Called by the .clawa emitter for each tool that has comments in the .claw source

TASK 2 — BAML synthesis output parser (new file):
Create `baml_src/synthesis.baml`:
  - Define `SynthesisResult` class (fields: status, tool_name, file_path, warnings, error_msg, tsc_passed, test_passed)
  - Define `ParseSynthesisOutput(raw_output: string) -> SynthesisResult` function
  - Use claude-haiku-4-5 as the client (fast + cheap for parsing)

TASK 3 — BAML type alignment checker (same file):
Add to `baml_src/synthesis.baml`:
  - Define `TypeAlignmentResult` class (fields: aligned, mismatches: FieldMismatch[])
  - Define `FieldMismatch` class (fields: field, declared_type, actual_type)
  - Define `CheckTypeAlignment(declared_schema: string, generated_code: string) -> TypeAlignmentResult`

TASK 4 — Synthesis prompt builder (Rust, extend synth_runner.rs):
Add `fn build_synthesis_prompt(tool: &ToolDecl, document: &Document, enriched: Option<&EnrichedEntities>, attempt: u32, previous_failure: Option<&str>) -> String`
  - Uses the prompt template from Spec 43 §2
  - Injects NER entities into the constraints section when present
  - Appends previous failure context on retry (attempt > 1)
  - Writes the prompt to `/tmp/claw_synth_<tool_name>_<hash>.txt`

TASK 5 — Synthesis invocation (Rust, extend synth_runner.rs):
Add `fn invoke_opencode(prompt_path: &Path, project_root: &Path, tool_name: &str, timeout_ms: u64) -> Result<String, SynthesisError>`
  - Writes isolated opencode.json to /tmp/claw_synth_<hash>/opencode.json (no mcp section)
  - Spawns: `opencode run "$(cat <prompt_path>)" --dir /tmp/claw_synth_<hash> --format json`
  - Reads NDJSON event stream, extracts final text response
  - Returns raw text or error

CONSTRAINTS (do not violate):
- Do not modify `src/codegen/typescript.rs`, `src/codegen/python.rs`, or `src/codegen/runtime.rs`
- Do not modify any existing parser or AST code
- Do not modify `opencode.json` in the project root
- The synthesis path is ONLY triggered for tools with `using:` — `invoke:` tools are unchanged
- All new Rust code must pass `cargo test` without breaking existing 91 tests
- NER runs only when tool has doc comments; if no comments, enrichment is skipped silently
- BAML clients default to haiku for speed; can be overridden in claw.json synthesis config

VERIFICATION:
After implementation, run:
  1. cargo test  (must pass all existing tests)
  2. npx baml-cli generate  (must generate baml_client/ without errors)
  3. node -e "import('./scripts/ner_enrich.js').then(m => console.log(m.enrichToolSpec('WebSearch', '// Searches the web using DuckDuckGo API at https://api.duckduckgo.com', {})))"
```

---

## Part 6: Risk Register

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| OpenCode CLI changes in v1.3+ break synthesis | High | High | Pin opencode version in Claw's package.json; test on upgrade |
| Local models too slow for synthesis in practice | High | High | Default to cloud model in synthesizer {}; local is opt-in |
| MCP isolation temp dir not cleaned on crash | Medium | Low | Lock file + cleanup on next build |
| BAML ParseSynthesisOutput hallucinates a result | Low | Medium | Always check file exists on disk regardless of BAML result |
| NER false positives add wrong constraints | Low | Low | NER entities are advisory, not mandatory; synthesis ignores nonsense |
| descode doesn't exist in OpenCode | High | Medium | Stage D audits descode; fallback is BAML TypeAlignmentResult |
| Port conflicts in concurrent synthesis | Medium | Medium | Set explicit `--port 0` per synthesis invocation |
