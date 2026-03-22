# Spec 43: OpenCode Headless Interface Contract (Stage A)

**Status:** ACTIVE — Defines the exact protocol between `claw build` and OpenCode for synthesis.
**Depends on:** Spec 42 (OpenCode as synthesis engine)
**Finding:** No new OpenCode capability needed. The existing `-p` non-interactive mode is the interface.

---

## 0. Live Test Results — CONFIRMED

**Test:** `opencode run "say exactly: HELLO_WORLD" --format json --dir /project`
**Model:** ollama/qwen2.5:14b
**Result:** Protocol fully confirmed. Exit code 0.

Actual NDJSON event stream captured:
```json
{"type":"step_start","sessionID":"ses_...","part":{...}}
{"type":"text","sessionID":"ses_...","part":{"type":"text","text":"HELLO_WORLD\n..."}}
{"type":"step_finish","sessionID":"ses_...","part":{"reason":"stop","cost":0,"tokens":{"total":4157,"input":4096,"output":61}}}
```

**Key facts confirmed:**
- Final text lives in `type=text` events, `part.text` field (concatenate all text events)
- Session ends with `type=step_finish`, `part.reason="stop"` (or `"tool_use"` if calling a tool)
- Cost and token usage are tracked per event
- Total round-trip for a simple prompt: ~9 seconds on qwen2.5:14b (local Ollama)

**MCP interference finding:** The `qwen2.5-coder:7b` model (smaller, more tool-aggressive) called the project's MCP tool instead of following the synthesis prompt. The `qwen2.5:14b` model did not. Isolation via temp directory is still the correct mitigation — model behavior varies and cannot be relied upon.

---

## 1. Source Audit Finding

Reading `opencode/internal/app/app.go` and `cmd/root.go` revealed:

**The interface that already exists:**
```bash
opencode -p "<prompt>" -q -f json -c /path/to/project
```

What it does internally (`RunNonInteractive`):
1. Creates a session
2. Calls `a.Permissions.AutoApproveSession(sess.ID)` — **all tool calls auto-approved, no prompts**
3. Runs `CoderAgent.Run(ctx, sessionID, prompt)` — full agentic loop
4. Waits for the `done` channel
5. Outputs `{"response": "..."}` to stdout (with `-f json`)
6. Exits 0 on success, non-zero on error

The coder agent has these tools (from `CoderAgentTools`): `bash`, `write`, `edit`, `read`, `ls`, `grep`, `glob`, `fetch`, `patch`, `diagnostics`

**Bash constraints to know:**
- Banned: `curl`, `wget`, `nc`, `telnet` — but Node 18+ native `fetch()` is unaffected
- Non-safe commands trigger permission checks — **but `AutoApproveSession` bypasses all of them**
- Max output: 30,000 chars per bash call
- Default timeout: 1 minute per command

**No `--headless` flag or `task` subcommand exists.** Spec 42's proposed interface was theoretical. This spec replaces it with the real one.

---

## 1. The Synthesis Protocol

For each tool with `using:` in the `.claw` file, `claw build` does the following:

### Step 1 — Compose synthesis prompt

Claw writes a structured prompt to a temp file:

```
/tmp/claw_synth_<tool_name>_<hash>.txt
```

### Step 2 — Invoke OpenCode

```bash
opencode -p "$(cat /tmp/claw_synth_<tool>_<hash>.txt)" \
         -q \
         -c <project_root>
```

`-q` suppresses the spinner. No `-f json` — we parse the text response for the sentinel.

### Step 3 — Parse result

Claw reads stdout. Two outcomes:
- Response contains `SYNTHESIS_COMPLETE: <ToolName>` → success, read the written file
- Response does not contain sentinel → failure, extract error context, retry

### Step 4 — Verify written file exists

After a `SYNTHESIS_COMPLETE` signal, Claw checks that `generated/tools/<ToolName>.ts` was actually written. If not, treat as failure.

### Step 5 — Run contract tests

Even after OpenCode signals success, Claw runs vitest contract tests as the final gate (Spec 32 §6 Tier 1). OpenCode's self-verification via bash is best-effort; the contract tests are authoritative.

---

## 2. Synthesis Prompt Structure

The prompt is a structured plain-text document. Not prose. Not a chat message.

```
You are the Claw synthesis agent. Your task: implement one TypeScript tool exactly to spec.

═══════════════════════════════════════════
TOOL SPEC
═══════════════════════════════════════════
Name:       WebSearch
Signature:  async function WebSearch(inputs: { query: string }): Promise<SearchResult>

Return type schema:
  SearchResult {
    url:        string           // required, non-empty
    snippet:    string           // required, non-empty
    confidence: float            // required, range [0.0, 1.0]
  }

Capability: fetch
Constraints:
  - No eval() or Function() constructor
  - Validate all inputs before use
  - Handle network errors — never throw uncaught exceptions
  - Do not hardcode credentials or API keys

Test cases that MUST pass:
  Input:  { query: "rust language" }
  Expect: url != "", snippet != "", 0.0 <= confidence <= 1.0

═══════════════════════════════════════════
OUTPUT REQUIREMENTS
═══════════════════════════════════════════
1. Write implementation to: generated/tools/WebSearch.ts
2. File must export: export async function WebSearch(...)
3. Import types from: ../types.js
4. After writing, run: npx tsc --noEmit --target es2022 --moduleResolution bundler generated/tools/WebSearch.ts
5. If tsc fails, fix errors and rewrite the file
6. After tsc passes, run a quick smoke test:
   node --input-type=module <<'EOF'
   import { WebSearch } from './generated/tools/WebSearch.js';
   const r = await WebSearch({ query: 'rust language' });
   if (!r.url) throw new Error('url is empty');
   if (r.confidence < 0 || r.confidence > 1) throw new Error('confidence out of range');
   console.log('PASS');
   EOF
7. If smoke test fails, diagnose and rewrite

═══════════════════════════════════════════
COMPLETION SIGNAL
═══════════════════════════════════════════
When the implementation is written, compiles, and the smoke test passes, output EXACTLY:
SYNTHESIS_COMPLETE: WebSearch

If you cannot produce a working implementation after your best effort, output EXACTLY:
SYNTHESIS_FAILED: WebSearch
REASON: <one sentence>
```

### Why this format works

- OpenCode's coder agent reads structured text well — the section headers act as context anchors
- The explicit numbered steps match how the coder agent already approaches coding tasks
- The smoke test gives OpenCode's bash tool a concrete way to self-verify
- The sentinels (`SYNTHESIS_COMPLETE:` / `SYNTHESIS_FAILED:`) are unambiguous — Claw parses stdout with a simple string search
- The prompt contains zero ambiguity about what file to write, what to export, or what types to import

---

## 3. Retry Loop with Feedback

On failure (no `SYNTHESIS_COMPLETE` sentinel, or file not written, or contract tests fail), Claw retries up to 3 times. Each retry appends failure context to the prompt:

```
═══════════════════════════════════════════
PREVIOUS ATTEMPT FAILED
═══════════════════════════════════════════
Attempt: 2 of 3

Contract test failure:
  Test: output.url must be non-empty string
  Received: url = ""

  Test: confidence must be in range [0.0, 1.0]
  Received: confidence = -1

The file generated/tools/WebSearch.ts exists but fails these checks.
Fix the implementation to pass all test cases.
```

The retry prompt includes:
- Which attempt number this is
- The exact failing assertions (from vitest output)
- Whether the file was written or not
- The last 50 lines of the written file if it exists (so OpenCode can see what it produced)

Retry 3 failure → hard error with the full synthesis trace written to `generated/tools/.failed/WebSearch.attempt3.ts`.

---

## 4. Concurrency Model

Tools within a single `.claw` file are synthesized **sequentially by default**. This is intentional:

1. OpenCode uses a persistent shell session per invocation — concurrent sessions would interfere with each other's file writes
2. Session memory carries context between tools in the same file — sequential synthesis lets OpenCode see what it already wrote and maintain consistent patterns
3. The synthesis step typically takes 10–60s per tool — parallelism adds complexity without enough benefit at this scale

**Future parallel option:** `claw.json` can opt into parallel synthesis:
```json
{ "synthesis": { "concurrency": 4 } }
```
With `concurrency > 1`, Claw spawns N separate OpenCode processes, each handling independent tools. File write isolation is guaranteed because each tool writes to its own `generated/tools/<Name>.ts` path. This is safe but loses session memory continuity.

---

## 5. Exit Codes and Error Handling

| OpenCode exit code | Meaning | Claw action |
|---|---|---|
| `0` | Agent completed | Parse stdout for sentinel |
| `1` | Agent error (timeout, provider failure, etc.) | Retry if attempts remain |
| `127` | `opencode` not found | Abort — emit error E-SYN01 |

**Error codes:**

`E-SYN01` — OpenCode not found:
```
error[E-SYN01]: opencode not found on PATH.
  Synthesis requires OpenCode to be installed.
  Install: https://opencode.ai

  Fallback: claw build --synth-backend=api
```

`E-SYN02` — Synthesis failed after all retries:
```
error[E-SYN02]: synthesis failed for 'WebSearch' after 3 attempts.
  Last attempt output: generated/tools/.failed/WebSearch.attempt3.ts
  Failure reason: url field is always empty

  Options:
  1. Add a reference: or note: hint to the tool declaration
  2. Simplify the test {} expectations
  3. Try a more capable model in opencode.json
```

`E-SYN03` — OpenCode timed out:
```
error[E-SYN03]: OpenCode timed out after 120s synthesizing 'WebSearch'.
  Default timeout: 120s. Configure in claw.json: { "synthesis": { "timeout_ms": 180000 } }
```

`W-SYN01` — Synthesis used fallback API mode:
```
warning[W-SYN01]: running in API synthesis fallback mode.
  OpenCode not found — using raw API synthesis.
  descode security gate is disabled in this mode.
  Install OpenCode for full synthesis quality: https://opencode.ai
```

---

## 6. Types.ts Prerequisite

The synthesis prompt references `../types.js` for return type imports. Before any tool synthesis begins, Claw generates `generated/types.ts` deterministically from the AST (no LLM). This file must exist before OpenCode is invoked.

```typescript
// generated/types.ts — auto-generated by claw compile, do not edit
export interface SearchResult {
  url: string;
  snippet: string;
  confidence: number;
}

export interface PageSummary {
  title: string;
  content: string;
  word_count: number;
}
```

The types file is the shared contract between synthesized tools and deterministic workflow code. OpenCode imports from it; workflow codegen imports from it.

---

## 7. OpenCode Configuration for Synthesis

The synthesis session uses whatever model is configured in `opencode.json`. No Claw-specific configuration is needed — OpenCode's existing `agents.coder.model` setting applies.

Recommended model for synthesis: `claude-sonnet-4-6` or better. Smaller models (7B-14B local) can work for simple `fetch`-based tools but struggle with `playwright` or complex `bash` tools.

The synthesizer model configured in the `.claw` file (`synthesizer {}` block) maps to OpenCode's model setting:

```
// In .claw:
synthesizer DefaultSynth {
    client = MyClaude
}

// MyClaude client:
client MyClaude {
    provider = "anthropic"
    model    = "claude-sonnet-4-6"
}
```

`claw build` updates `opencode.json` temporarily for the synthesis session, then restores it. If the user's `opencode.json` already specifies `agents.coder.model`, Claw uses that and ignores the `.claw` synthesizer declaration (with a warning if they differ).

---

## 8. descode Integration (Deferred to Stage D)

The bash tool's `safeReadOnlyCommands` list and `bannedCommands` list show that OpenCode already has some security logic. However, descode as described in Spec 42 §2.2 is a separate sub-agent capability, not yet audited in the source.

Stage D will audit the descode interface. For Stage A, the security contract is:
- The synthesis prompt explicitly lists "no eval(), no hardcoded credentials" as constraints
- The coder agent's own guidelines include security best practices
- Contract tests validate input/output types but not security properties

This is a known gap. Stage D closes it.

---

## 9. Implementation in Rust (`src/codegen/synth_runner.rs`)

The existing `synth_runner.rs` generates a Node.js bridge process. Stage C replaces this with an OpenCode invocation. The Rust interface stays the same — a function that takes a tool spec and returns the written TypeScript path or an error.

The prompt template lives in Rust as a const string with format arguments. No external template file.

Key Rust function signature (Stage C will implement):

```rust
pub async fn synthesize_tool(
    tool: &ToolDecl,
    document: &Document,
    project_root: &Path,
    attempt: u32,
    previous_failure: Option<&SynthesisFailure>,
) -> Result<PathBuf, SynthesisError>
```

Returns the path to the written TypeScript file on success.

---

## 10. Verification: Does This Work Today?

Testing the protocol with the existing OpenCode binary before writing any Rust:

```bash
# 1. Make sure opencode is installed
which opencode

# 2. Create a test synthesis prompt
cat > /tmp/test_synth.txt << 'EOF'
You are the Claw synthesis agent. Implement this TypeScript tool to spec.

Name:       HelloWorld
Signature:  async function HelloWorld(inputs: { name: string }): Promise<{ greeting: string }>

Return type schema:
  { greeting: string }  // must be non-empty

Capability: none (pure logic)

OUTPUT REQUIREMENTS
1. Write implementation to: generated/tools/HelloWorld.ts
2. Export: export async function HelloWorld(inputs: { name: string }): Promise<{ greeting: string }>
3. After writing, verify it runs:
   node --input-type=module <<'NODEEOF'
   import { HelloWorld } from './generated/tools/HelloWorld.js';
   const r = await HelloWorld({ name: 'World' });
   if (!r.greeting) throw new Error('empty greeting');
   console.log('PASS', r.greeting);
   NODEEOF

When done and verified: SYNTHESIS_COMPLETE: HelloWorld
If impossible: SYNTHESIS_FAILED: HelloWorld
REASON: <reason>
EOF

# 3. Run it
mkdir -p generated/tools
opencode -p "$(cat /tmp/test_synth.txt)" -q -c /Users/dixon.zor/Documents/Open-code

# 4. Check result
cat generated/tools/HelloWorld.ts
```

If this works end-to-end, Stage A is validated and Stage C (Rust wiring) can proceed.
