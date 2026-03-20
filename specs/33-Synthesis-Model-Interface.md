# Spec 33: Synthesis Model Interface

**Status:** Specced 2026-03-19. Defines the model-agnostic contract for the Synthesis Pass, the training data schema for a future Claw-specific model, and the evaluation protocol.

---

## 1. What the Synthesis Pass Is

The Synthesis Pass is the LLM/SLM that reads a Claw Artifact and outputs TypeScript. It is not an agent. It does not take actions. It is a **compiler backend that uses an LLM as its code generation engine**.

Current: any user-configured LLM/SLM works (Claude, GPT-4o, Mistral, local Ollama model).
Future: a fine-tuned model trained specifically on Claw artifacts — faster, cheaper, and more reliable than a general-purpose LLM for this specific task.

This spec defines the interface so both current and future models satisfy the same contract.

---

## 2. Model-Agnostic Interface Contract

Any model serving as the Synthesis Pass MUST implement this interface. The Claw compiler calls it the same way regardless of which model is behind it.

### 2.1 Input: SynthesisRequest

```typescript
interface SynthesisRequest {
  // What to synthesize
  target: SynthesisTarget;

  // The full artifact for type resolution
  artifact: ClawArtifact;

  // Reference implementations the model should study before writing
  references: ReferenceImpl[];

  // Test cases the output must pass (for re-synthesis context)
  tests: TestContract[];

  // On retry: the previous attempt and why it failed
  prior_attempt?: {
    code:           string;
    failing_tests:  TestFailure[];
    attempt_number: number;   // 1 or 2 (hard error on 3rd)
  };
}

type SynthesisTarget =
  | { kind: 'tool';     tool_name: string }
  | { kind: 'workflow'; workflow_name: string };

interface ReferenceImpl {
  label:       string;   // e.g. "OpenCode WebSearch implementation"
  language:    string;   // "typescript"
  source_code: string;
}

interface TestContract {
  description: string;
  input:        Record<string, unknown>;
  assertions:   Assertion[];
}

interface Assertion {
  field: string;
  op:    '!empty' | 'range' | 'typeof' | 'equals' | 'matches';
  value?: unknown;
  min?:   number;
  max?:   number;
}

interface TestFailure {
  test_description: string;
  expected:         string;
  received:         string;
}
```

### 2.2 Output: SynthesisResponse

```typescript
interface SynthesisResponse {
  // The generated TypeScript implementation
  code: string;

  // Imports required (used by bundler)
  imports: string[];

  // npm packages this code depends on
  dependencies: string[];

  // Optional: model's confidence 0.0-1.0
  // Used to decide whether to skip straight to test or warn user
  confidence?: number;
}
```

### 2.3 Invocation

The compiler invokes the model through a thin adapter. Each adapter implements:

```typescript
interface SynthesisModelAdapter {
  synthesize(request: SynthesisRequest): Promise<SynthesisResponse>;
}
```

Adapters ship for: Anthropic API, OpenAI API, Ollama (local), and the future Claw-native model. The user configures which adapter is used via the `synthesizer {}` block in their `.claw` file.

---

## 3. Prompt Template (Current General-Purpose LLMs)

Until a Claw-native model exists, the Synthesis Pass uses a structured prompt template. This template is what a future fine-tuned model would internalize and no longer need.

```
SYSTEM:
You are a TypeScript code synthesizer. Your only output is valid TypeScript.
Do not explain. Do not use markdown code fences. Output only the implementation.

USER:
## SYNTHESIS TARGET
Type: {{ target.kind }}
Name: {{ target.name }}

## TYPE CONTRACTS
{{ serialize(artifact.types) }}

## TOOL SPECIFICATION
Inputs:      {{ tool.inputs }}
Output type: {{ tool.output_type }}
Capability:  {{ tool.using }}

## TESTS YOUR CODE MUST PASS
{{ format_tests(request.tests) }}

## REFERENCE IMPLEMENTATIONS
Study these before writing. Your code should follow the same patterns.

{{ for ref in request.references }}
### {{ ref.label }}
```typescript
{{ ref.source_code }}
```
{{ end }}

{{ if request.prior_attempt }}
## PREVIOUS ATTEMPT FAILED
Attempt {{ request.prior_attempt.attempt_number }}/3

Failed code:
```typescript
{{ request.prior_attempt.code }}
```

Failing tests:
{{ format_failures(request.prior_attempt.failing_tests) }}

Fix the above failures in your new implementation.
{{ end }}

## OUTPUT FORMAT
Export the function as a named export matching: {{ tool.name }}
The function signature must be:
  export async function {{ tool.name }}(inputs: {{ InputType }}): Promise<{{ OutputType }}>
```

---

## 4. Reference Implementation Injection

The Synthesis Pass is most effective when given reference implementations to study. The compiler collects these from two sources:

### 4.1 Capability primitives library (shipped with Claw)

A curated set of reference implementations for each `using:` primitive, maintained in `src/synthesis/references/`:

```
src/synthesis/references/
├── fetch.ts          # canonical fetch-based tool pattern
├── playwright.ts     # canonical Playwright automation pattern
├── mcp-client.ts     # canonical MCP client call pattern
├── baml-client.ts    # canonical BAML extraction pattern
└── bash.ts           # canonical exec pattern
```

These are hand-written, tested, and updated by Claw maintainers. They represent the "correct" way to use each capability, and the model studies them before synthesizing any tool.

### 4.2 User-provided references (future)

Users can specify additional reference implementations in their `.claw` file:

```
tool WebSearch(query: string) -> SearchResult {
    using: fetch
    reference: "scripts/search.js"   // existing code the model should study
}
```

---

## 5. Training Data Schema (Future Claw-Native Model)

Every successful `claw build` that passes tests is a training example. The training data schema defines how these are collected and stored.

### 5.1 Training example format

```json
{
  "id": "sha256:abc...",
  "created_at": "2026-03-19T20:00:00Z",

  "input": {
    "target": { "kind": "tool", "name": "WebSearch" },
    "tool_spec": {
      "inputs": [{ "name": "query", "type": "string" }],
      "output_type": "SearchResult",
      "using": "fetch"
    },
    "type_context": [...],
    "tests": [...],
    "references": [...]
  },

  "output": {
    "code": "export async function WebSearch(...) { ... }",
    "imports": ["import fetch from 'node-fetch';"],
    "dependencies": ["node-fetch"]
  },

  "quality": {
    "tests_passed":   true,
    "attempt_number": 1,        // 1 = first try, 2 = needed one retry
    "test_results": [
      { "name": "contract:output_shape", "passed": true },
      { "name": "behavior:non_empty_url", "passed": true }
    ]
  }
}
```

### 5.2 Training data collection

The compiler writes training examples to `~/.claw/synthesis-telemetry/` (opt-in, off by default). Users enable it with:

```json
// claw.json
{
  "telemetry": {
    "synthesis": true,
    "upload": false    // local only unless user opts in to upload
  }
}
```

### 5.3 Quality filtering for fine-tuning

Not all examples are good training data. Filter criteria:

| Criterion | Threshold | Reason |
|---|---|---|
| Tests passed | required | Failed syntheses are negative examples only |
| Attempt number | 1 preferred, 2 acceptable | Multi-attempt examples show the model struggling |
| Code length | 5-200 lines | Filter outliers |
| Dependency count | ≤5 packages | Overly complex dependencies suggest bad synthesis |

---

## 6. Evaluation Protocol

When evaluating whether a Synthesis Pass model is good enough for a given task:

### 6.1 Metrics

- **Pass@1**: % of tools synthesized correctly on the first attempt
- **Pass@3**: % of tools synthesized correctly within 3 attempts
- **Contract coverage**: % of type fields correctly typed in generated code
- **Latency P50/P95**: synthesis time in milliseconds
- **Token cost**: average tokens consumed per tool synthesis

### 6.2 Benchmark suite

A set of canonical `.claw` files with known-correct TypeScript outputs. Maintained in `tests/synthesis-bench/`:

```
tests/synthesis-bench/
├── basic-fetch/          # simple fetch tool, should hit Pass@1 > 95%
├── playwright-nav/       # multi-step browser automation
├── mcp-client/           # MCP tool call
├── baml-extract/         # BAML extraction function
├── chained-workflow/     # multi-step workflow with type threading
└── reason-block/         # workflow with dynamic reasoning
```

### 6.3 Model selection guidance

| Use case | Recommended model tier |
|---|---|
| Prototyping / low budget | 7B-14B local (qwen2.5:14b, mistral-nemo) |
| Production / high reliability | 70B+ or cloud (claude-sonnet-4-6, gpt-4o) |
| Future: Claw-native model | TBD — target: matches 70B quality at 7B speed |

---

## 7. The Future Claw-Native Model

### 7.1 What it is

A model fine-tuned specifically to translate Claw Artifacts into TypeScript. It internalizes:
- The `.clawa` artifact schema
- All capability primitive patterns
- The type system and constraint system
- Common synthesis patterns (fetch pagination, Playwright navigation, etc.)

It does not need the full prompt template from §3 — it has internalized the task.

### 7.2 Architecture target

- **Base**: a 7B or 14B code model (Qwen2.5-Coder-7B or similar)
- **Fine-tuning**: supervised fine-tuning on the training corpus from §5
- **Inference**: local via Ollama, fast enough for `claw build` hot path (<2s per tool)
- **Quality target**: Pass@1 ≥ 90% on the benchmark suite

### 7.3 Training pipeline (future)

```
Community claw build runs
    │ (telemetry, opt-in)
    ▼
Raw training examples
    │ (quality filter from §5.3)
    ▼
Curated training set
    │ (supervised fine-tuning)
    ▼
claw-synth-7b base checkpoint
    │ (benchmark eval from §6.2)
    ▼
Released as: ollama pull claw/synth
```

### 7.4 Model declaration in .claw (future)

```
synthesizer FastSynth {
    client = ClawNativeModel    // fine-tuned model
    temperature = 0.05          // very low — model is highly specialized
}

client ClawNativeModel {
    provider = "local"
    model    = "local.claw/synth"
}
```

### 7.5 Interface compatibility guarantee

The Claw-native model MUST implement the same `SynthesisModelAdapter` interface defined in §2.3. The compiler does not change. Users switch models by changing their `synthesizer {}` declaration. This is the reason the interface must be stable before the fine-tuned model is trained.

---

## 8. synth-runner.js Protocol (R1-04)

The Rust compiler cannot directly instantiate a TypeScript `SynthesisModelAdapter`. Communication happens via a child process using newline-delimited JSON on stdin/stdout.

### 8.1 synth-runner.js (auto-generated by Stage 1)

Stage 1 (Rust compile) generates `generated/synth-runner.js` — a Node.js script that:
1. Reads the synthesizer config from the artifact
2. Imports the correct provider adapter (Anthropic, OpenAI, Ollama, etc.)
3. Reads `SynthesisRequest` objects from stdin (one JSON object per line)
4. Writes `SynthesisResponse` objects to stdout (one JSON object per line)
5. Writes progress/errors to stderr

```typescript
// generated/synth-runner.js (auto-generated — do not edit)
import { AnthropicAdapter } from '@claw/synth-adapters/anthropic.js';
import { readlineInterface } from 'node:readline';

const adapter = new AnthropicAdapter({
  model: 'claude-sonnet-4-6',
  temperature: 0.1,
  maxTokens: 8192,
  apiKey: process.env.ANTHROPIC_API_KEY,
});

const rl = readlineInterface({ input: process.stdin });
const pending: Promise<void>[] = [];

rl.on('line', (line) => {
  const request: SynthesisRequest = JSON.parse(line);
  const p = adapter.synthesize(request).then((response) => {
    process.stdout.write(JSON.stringify(response) + '\n');
  }).catch((err) => {
    process.stdout.write(JSON.stringify({ error: err.message }) + '\n');
  });
  pending.push(p);
});

rl.on('close', () => Promise.all(pending));
```

### 8.2 Provider adapters package: `@claw/synth-adapters`

A small npm package (shipped with Claw CLI) containing adapters for each supported provider. Each adapter implements `SynthesisModelAdapter`. The package is NOT user-facing — it is an implementation detail of `synth-runner.js`.

Supported adapters: `anthropic`, `openai`, `ollama`, `openrouter`.

---

## 9. Telemetry Privacy (R1-15)

Training examples written to `~/.claw/synthesis-telemetry/` may contain:
- Business logic expressed as type names and field names
- Proprietary API patterns inferred from `using:` declarations
- Internal tooling names from `mcp("...")` values
- System prompt content from `agent` declarations

**Privacy rules:**
- Telemetry is OFF by default. User must explicitly set `"telemetry": { "synthesis": true }` in `claw.json`.
- The upload flag (`"upload": true`) requires a SECOND explicit opt-in.
- When enabled, the following fields are SCRUBBED from training examples before storage:
  - `agents[].system_prompt` → replaced with `"<redacted>"`
  - `synthesizers[].model` API key env vars → never written (keys are never in the artifact)
  - String literals in `test.input` values → replaced with `"<test-input>"`
- The scrubbing happens in Rust before writing. Users can inspect `~/.claw/synthesis-telemetry/` to verify.
- Local telemetry (not uploaded) is never transmitted anywhere.

---

## 10. `SynthesisRequest` — MCP Endpoint Resolution (R2-07)

For `using: mcp("brave-search")` tools, the synthesis request includes endpoint resolution context:

```typescript
interface McpCapabilityContext {
  server_name: string;          // "brave-search"
  resolved_url?: string;        // from opencode.json mcp config if present
  tool_schema?: JsonSchema;     // fetched from server's ListTools if resolvable
}
```

Resolution order:
1. Check `opencode.json` → `mcp["brave-search"].command` — extract the server's declared tools
2. If not found → synthesize a generic MCP client call using server name only
3. The synthesized code uses the MCP SDK's `Client.callTool()` with dynamic dispatch

If the MCP server is not reachable at build time, the synthesizer generates a runtime-resolved call — the tool will fail at execution if the server is unavailable, not at build time.

---

## 11. What This Spec Does NOT Cover

- The full Claw compiler changes needed (see Spec 32)
- BAML integration for `reason {}` output validation (see Spec 31)
- The capability primitives reference library content (implementation concern)
- CLI changes (`claw synthesize`, `claw verify`) (see Spec 14 update needed)
- `@claw/synth-adapters` package implementation details (separate implementation task)
