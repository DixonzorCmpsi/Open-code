# Spec 48: GAN Audit — Canonical Architecture Decision

**Status:** ACTIVE — filed 2026-03-21. This is the definitive architecture alignment document.
**Supersedes:** Spec 47 (Round 2 findings), resolves open questions from Spec 44, 46.
**Depends on:** Spec 25, 32, 33, 42, 43

---

## 0. The Core Abstraction (User-Facing)

The user's contract with Claw is simple:

```
User writes .claw file  →  claw build  →  claw run WorkflowName --arg ...  →  artifact
```

**The user never touches:** TypeScript, MCP servers, OpenCode configuration, synthesis APIs, or orchestration. The language handles everything downstream. This is the abstraction we are building. Every architectural decision must be evaluated against it.

---

## 1. Canonical Execution Path (Decision: Merged Track)

**Finding:** Two parallel execution tracks exist in the code and cannot coexist cleanly:

| | Track A (current) | Track B (synthesis) |
|---|---|---|
| File | `generated/runtime.js` | `generated/workflows/*.ts` → compiled `.js` |
| Tools | `invoke: module(...)` | `using: fetch/bash/playwright/mcp` |
| LLM at runtime | Yes (agent loop via runAgent_*) | Only for `reason {}` blocks |
| Status | Works today | Needs synthesis + compile step |

**Decision: Single merged path.**

`claw run WorkflowName` does this:
1. Check if `generated/workflows/WorkflowName.js` exists → **Track B** (synthesized, preferred)
2. Otherwise → **Track A** (`generated/runtime.js`, always available as fallback)

Track A is the guaranteed fallback. Track B is the future. Both are valid indefinitely — there is no deprecation of Track A.

**Implementation:** `claw run` detects which track to use by checking file existence. No flag needed.

---

## 2. Synthesis Engine: OpenCode Is the OS (Decision: Replace synthesize.mjs)

**Finding:** The current `synthesize.mjs` (Karpathy loop, direct Anthropic SDK) is misaligned with the architecture. It:
- Hardcodes the Anthropic provider
- Bypasses OpenCode entirely
- Does not use OpenCode's bash access for self-verification
- Does not use OpenCode's security analysis

**Spec 43 confirmed live (2026-03-21):**
```bash
opencode run --model ollama/qwen2.5:14b --dir . --format json "$(cat prompt.txt)"
```
Output: NDJSON stream. Concatenate all `type=text` events for the response. Exit 0 on success.

**Decision: `claw synthesize` invokes `opencode run` for each tool.**

```
claw synthesize
    │
    ├─ For each tool with using: in the .claw file:
    │     1. Generate synthesis prompt (from skill spec + types.ts)
    │     2. Run: opencode run --model <synthesizer_model> --dir <project_root> --format json "<prompt>"
    │     3. Parse NDJSON for SYNTHESIS_COMPLETE: <ToolName> sentinel
    │     4. Verify generated/tools/<ToolName>.ts was written
    │     5. Run contract tests (vitest)
    │     6. On failure: retry up to 3x with failure context appended
    │
    └─ After all tools: print synthesis summary
```

The `synthesize.mjs` file is removed from the codegen pipeline. `claw synthesize` runs the loop directly from Rust (spawning `opencode run` as a child process).

---

## 3. Model-Agnostic Synthesis (Decision: synthesizer client drives opencode model)

**User requirement:** Synthesis must work with any model — local Ollama, Anthropic, OpenAI, etc. No API key required if a local model is configured.

**The mechanism:**

The `synthesizer {}` block in `.claw` declares which client to use:
```
synthesizer DefaultSynth {
    client = LocalQwen
}

client LocalQwen {
    provider = "local"
    model    = "local.qwen2.5:14b"
}
```

`claw synthesize` maps the client to an `opencode run` model flag:

| `.claw` provider | opencode run --model flag |
|---|---|
| `"anthropic"` | `anthropic/<model>` |
| `"openai"` | `openai/<model>` |
| `"local"` / `"ollama"` | `ollama/<model-without-local-prefix>` |

Example: `model = "local.qwen2.5:14b"` → `--model ollama/qwen2.5:14b`

If no synthesizer is declared, `claw synthesize` uses whatever model OpenCode has configured in the user's global OpenCode settings. This is the zero-config path.

**No API keys are managed by Claw.** OpenCode manages its own credential store. The user runs `opencode providers` once to add their key — Claw never touches it.

---

## 4. The Full End-to-End Flow (Post-Alignment)

```
1.  User writes examples/my_workflow.claw
    (declares types, clients, tools with using:, agents, workflow)

2.  claw build examples/my_workflow.claw
    │
    ├── Parse + semantic check (Rust, <100ms)
    ├── Generate generated/types.ts          (deterministic)
    ├── Generate generated/mcp-server.js     (deterministic)
    ├── Generate generated/runtime.js        (Track A fallback, deterministic)
    ├── Generate generated/runtime/reason.ts (deterministic)
    ├── Generate generated/workflows/*.ts    (deterministic from AST)
    ├── Generate generated/specs/tools/*.md  (synthesis prompt templates)
    ├── Generate generated/artifact.clawa.json
    ├── Generate opencode.json               (project config)
    └── Generate .opencode/agents/*.md       (agent instruction files)

3.  claw synthesize [--tool ToolName]
    │
    ├── For each tool with using: (or just the named tool):
    │     opencode run --model <synthesizer_model> --dir . --format json "<prompt>"
    │     → OpenCode writes generated/tools/<ToolName>.ts (uses bash, self-verifies)
    │     → Claw reads SYNTHESIS_COMPLETE: <ToolName> sentinel
    │     → Claw runs vitest contract tests as authoritative gate
    │     → On fail: retry up to 3x with error context
    │
    └── tsc --noEmit on all generated/tools/*.ts

4.  claw run MyWorkflow --arg key=value
    │
    ├── Check: generated/workflows/MyWorkflow.js exists? (Track B)
    │     YES → node generated/workflows/MyWorkflow.js --arg key=value
    │     NO  → node generated/runtime.js MyWorkflow --arg key=value (Track A)
    │
    └── Result + artifact saved to declared path
```

---

## 5. OpenCode as Runtime (Decision: Spec 42 §4.1 implemented in claw run)

**User requirement:** `claw run` should invoke OpenCode on the backend for workflows that need it. The user doesn't know or care about the execution path.

**Decision:** `claw run` routes based on workflow capability requirements:

```
claw run WorkflowName --arg ...
    │
    ├── Has reason{} blocks? OR uses playwright/bash tools?
    │     YES → opencode run --model <agent_model> --dir . "Execute WorkflowName with args: ..."
    │           (OpenCode handles the MCP tool calls + LLM reasoning)
    │     NO  → node generated/workflows/WorkflowName.js (or runtime.js fallback)
```

For simple workflows (pure tool calls, no LLM at runtime), `claw run` uses Node directly — faster, no OpenCode dependency at execution time. For agentic workflows with `reason {}` blocks, OpenCode is the executor — it starts the MCP server, routes tool calls, and handles LLM reasoning.

**This is the abstraction:** The user writes `claw run`, the compiler decides whether to invoke Node or OpenCode based on declared capabilities. The user never chooses.

---

## 6. Spec-to-Code Gap Matrix (Current State 2026-03-21)

### 6.1 What is implemented and spec-aligned ✓

| Feature | Spec | Code location | Status |
|---|---|---|---|
| Parser: all DSL constructs | 01, 03 | `src/parser.rs` | ✓ |
| Types with `@min/@max/@regex` | 03, 05, 26 | `src/parser.rs` + `mcp.rs` | ✓ (H-01 fixed) |
| `using:` + `test{}` on tools | 32 | `src/parser.rs`, `src/ast.rs` | ✓ |
| `synthesizer {}` declaration | 32 | `src/parser.rs`, `src/ast.rs` | ✓ |
| `secrets {}` on tools | 47 | `src/parser.rs`, `mcp.rs` | ✓ |
| `on_fail:` on `reason {}` | 47 | `src/parser.rs`, `ts_workflow.rs` | ✓ |
| `agent extends` + tool merge | 03, 32 | `src/parser.rs`, `mcp.rs` | ✓ (H-02 fixed) |
| `artifact {}` block + save | 25, 32 | `src/codegen/runtime.rs` | ✓ |
| `opencode.json` generation | 25 | `src/codegen/opencode.rs` | ✓ |
| `.opencode/agents/*.md` | 25 | `src/codegen/opencode.rs` | ✓ |
| `.opencode/commands/*.md` | 25 | `src/codegen/opencode.rs` | ✓ |
| `mcp-server.js` generation | 26 | `src/codegen/mcp.rs` | ✓ |
| `runtime.js` generation | 38, 39 | `src/codegen/runtime.rs` | ✓ (Track A) |
| `types.ts` generation | 32 | `src/codegen/ts_types.rs` | ✓ |
| `workflows/*.ts` generation | 32 | `src/codegen/ts_workflow.rs` | ✓ (Track B) |
| `runtime/reason.ts` generation | 32 | `src/codegen/ts_reason.rs` | ✓ (C-01 fixed) |
| Skill spec markdown | 46 | `src/codegen/skill_spec.rs` | ✓ |
| `artifact.clawa.json` | 32 | `src/codegen/artifact.rs` | ✓ |
| Contract tests `__tests__/` | 32 | `src/codegen/ts_tests.rs` | ✓ |
| `claw build` | 32 | `src/bin/claw.rs` | ✓ |
| `claw run` (Track A) | 38 | `src/bin/claw.rs` | ✓ |
| `claw test` (DSL offline) | 08 | `src/bin/claw.rs` | ✓ |

### 6.2 Implemented but spec-misaligned ✗

| Feature | Spec says | Code does | Fix required |
|---|---|---|---|
| `claw synthesize` | OpenCode invocation (spec 43) | Runs `synthesize.mjs` (direct API, Anthropic-only) | **CRITICAL — replace with opencode run** |
| `synthesize.mjs` | Not the mechanism (spec 43 supersedes spec 46) | Generated and executed | **Remove from codegen pipeline** |
| `claw run` for Track B | Auto-detect Track B if synthesized (this spec §5) | Always uses Track A | **Fix routing** |
| `claw run` agentic | Route to OpenCode for reason{}/playwright (this spec §5) | Always uses Node | **Phase 2** |

### 6.3 Spec-required, not yet implemented

| Feature | Spec | Priority |
|---|---|---|
| `claw compile` (Stage 1 only) | 32 §2 | MEDIUM |
| `claw verify` (E2E tests) | 32 §2 | MEDIUM |
| `claw bundle` (esbuild) | 32 §2 | MEDIUM |
| `optional<T>` type | 26 §2.4 | MEDIUM |
| `tools +=` syntax | 03 | HIGH |
| `synthesize { strategy: }` block | 42, 45 | LOW |
| Synthesis cache | 32 §18 | HIGH |
| Agent/workflow spec files | 46 §6 | LOW |
| `claw init` OpenCode detection | 25 §11 | LOW |
| Project-level `env {}` block | 47 §5.1 | FUTURE |
| `auth {}` Descope integration | 47 §5.4 | FUTURE |
| `opencode {}` config block | 47 §5.3 | FUTURE |

---

## 7. What Changes in the Codebase (Implementation Plan)

### Priority 1 — Replace synthesis engine (CRITICAL, this session)

**File changes:**
1. `src/bin/claw.rs` — `run_synthesize()`: replace node synthesize.mjs call with `opencode run` loop
2. `src/codegen/synthesize_mjs.rs` — no longer used; keep file but remove call from build pipeline
3. Synthesis prompt: use existing `generated/specs/tools/<Name>.md` as the prompt base (already generated by skill_spec.rs)

The new `run_synthesize()` in Rust:
```rust
for tool in tools_with_using {
    let prompt = build_synthesis_prompt(tool, project_root);
    let model_flag = synthesizer_to_opencode_model(tool, document);
    let output = Command::new("opencode")
        .args(["run", "--model", &model_flag, "--dir", project_root, "--format", "json", &prompt])
        .output()?;
    let response_text = parse_ndjson_text_events(output.stdout);
    if response_text.contains(&format!("SYNTHESIS_COMPLETE: {}", tool.name)) {
        verify_file_written(tool, project_root)?;
        run_contract_tests(tool, project_root)?;
    } else {
        // retry or error
    }
}
```

### Priority 2 — Fix `claw run` routing (HIGH, this session)

`claw run WorkflowName` in `src/bin/claw.rs`:
```rust
let workflow_js = project_root.join("generated/workflows").join(format!("{}.js", workflow));
if workflow_js.exists() {
    // Track B: run synthesized workflow
    Command::new("node").arg(&workflow_js).args(cli_args).exec()
} else {
    // Track A: runtime.js fallback
    Command::new("node").arg(runtime_js).args([workflow, ...cli_args]).exec()
}
```

### Priority 3 — `tools +=` syntax (HIGH, next session)

Enables proper agent inheritance in DSL. Parser change in `src/parser.rs`.

---

## 8. Stale Specs (To Be Updated)

| Spec | What's stale | Update needed |
|---|---|---|
| Spec 26 §1 | `mcpServers`, `type: "stdio"` | Update to `mcp`, `type: "local"` per spec 25 |
| Spec 32 §16 | synth-runner stdin/stdout protocol | Replace with OpenCode invocation (spec 43) |
| Spec 46 | Karpathy loop as the synthesis mechanism | Superseded by spec 43; keep as reference for the score concept only |
| Spec 33 §8 | synth-runner.js protocol | Replace with OpenCode invocation |

---

## 9. Questions Answered

**Q1: Which track is best?**
A: Both. Track A (runtime.js) is the guaranteed working path always available. Track B (synthesized workflows) is the preferred path when synthesis has run. `claw run` auto-detects.

**Q2: Model-agnostic synthesis?**
A: Yes. The `synthesizer {}` block's `client` declaration drives `opencode run --model`. Local Ollama = no API key. Anthropic/OpenAI = their key stored in OpenCode's credential store, not in Claw.

**Q3: OpenCode as runtime?**
A: For Phase 1, `claw run` routes to Node (fast, no dependency). For Phase 2, `claw run` routes to `opencode run` for workflows with `reason {}` blocks or `playwright`/`bash` tools. The user never chooses — the compiler decides based on declared capabilities.
