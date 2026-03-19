# Spec 27: GAN Audit — OpenCode Migration

**Type:** Adversarial audit of specs 25, 26, and all updated specs
**Status:** ACTIVE — All findings below MUST be resolved before implementation begins
**Scope:** Full adversarial review of the Phase 2 migration from openclaw-gateway to OpenCode

---

## 0. Audit Methodology

This GAN (Generative Adversarial) audit applies adversarial pressure across five dimensions:

1. **Contradiction Detection** — specs that contradict each other or themselves
2. **Gap Detection** — implementation details promised but not specified
3. **Security Holes** — attack surfaces created by the new architecture
4. **Hallucination Flags** — claims about external systems (OpenCode, MCP) that may not be accurate
5. **Ripple-Effect Misses** — specs updated but their downstream dependencies missed

Each finding is tagged:
- `FATAL` — Blocks implementation. Spec MUST be corrected before code is written.
- `HIGH` — Significant risk. Spec should be corrected before implementation of the affected area.
- `MEDIUM` — Design ambiguity. Needs clarification but workaround exists.
- `LOW` — Improvement opportunity. Does not block.

---

## 1. FATAL Findings

### FATAL-01: SDK Transport Contract Is Undefined

**Affected specs:** `specs/06-CodeGen-SDK.md`, `specs/25-OpenCode-Integration.md §8.2`, `specs/01-DSL-Core-Specification.md §10`

**Finding:**
The generated TypeScript SDK (`generated/claw/index.ts`) previously transported workflow calls to `openclaw-gateway` via WebSocket. The gateway is now retired. The new specs say the SDK "calls OpenCode's API or CLI directly" but define no transport contract.

The following is now broken with no replacement spec:
```typescript
// spec/01 §10 shows this — but ClawClient({opencode: true}) is undefined
const gateway = new ClawClient({ opencode: true })
const reports = await AnalyzeCompetitors(companies, { client: gateway })
```

`ClawClient` has no implementation path. Spec/06 was updated to say "invokes OpenCode" without defining how.

**Options (pick one):**
1. **OpenCode CLI subprocess:** The generated SDK shells out to `opencode run /WorkflowName args` and parses stdout.
2. **OpenCode HTTP API:** The generated SDK calls OpenCode's local server API (if it exposes one).
3. **OpenCode TypeScript SDK:** Import `@opencode/sdk` and use its programmatic API.
4. **Drop programmatic SDK for now:** `--lang opencode` only generates the config files, not a TS/Python SDK. Programmatic use requires `--lang ts` separately.

**Required action:** Choose one option, document it in `specs/06` and `specs/25 §8.2`, and specify the exact import, API shape, and error handling. This is the single biggest implementation gap.

**Recommendation:** Option 4 (cleanest separation). `--lang opencode` generates config only. `--lang ts` generates the programmatic SDK. Developers choose the target based on use case. The `claw init` North Star in `specs/14 §0` must be updated accordingly.

---

### FATAL-02: `subagent` Mode Breaks Workflow-Agent Invocation

**Affected spec:** `specs/25-OpenCode-Integration.md §2.3`

**Finding:**
Spec/25 §2.3 sets all Claw-defined agents to `mode: subagent` in their emitted markdown. In OpenCode's model, `subagent` agents are invoked via `@agentname` syntax inside another agent's prompt. They are NOT directly callable as the primary executor of a command.

Commands (`.opencode/commands/*.md`) specify an `agent:` field. If that agent's mode is `subagent`, the command cannot use it as a primary agent — the command needs either `mode: primary` or `mode: all` on the agent.

**Impact:** All generated commands in `.opencode/commands/` that reference a `mode: subagent` agent will fail or behave incorrectly.

**Required fix in spec/25 §2.3:** Change the default `mode` rule:
- Agents referenced as the primary agent in a `workflow` command → `mode: all`
- Agents that are only invoked by other agents via `execute` → `mode: subagent`
- The compiler must determine which mode each agent needs based on how it's used in the AST.

---

### FATAL-03: Multi-Parameter Workflows Have No Command Template Strategy

**Affected spec:** `specs/25-OpenCode-Integration.md §2.4`

**Finding:**
OpenCode commands support `$ARGUMENTS` (all args as a string), `$1`, `$2`, `$3` (positional strings). The spec only shows single-parameter workflows. For multi-parameter workflows:

```claw
workflow AnalyzeMarket(companies: list<string>, depth: int, region: string) -> Report {
    ...
}
```

There is no specified mapping. `$1`, `$2`, `$3` are plain strings — they cannot carry typed lists or structured data.

**Impact:** Multi-parameter workflows with non-string types cannot be faithfully represented as OpenCode commands. The command template will lose type information.

**Required fix in spec/25 §2.4:** One of:
1. **Structured args:** The command template uses a JSON argument convention. `$ARGUMENTS` is always a JSON object string. The command markdown explains the expected JSON format. The MCP server or SDK deserializes it.
2. **Simple commands only:** Workflows with more than one parameter or with non-string/non-primitive parameters are NOT emitted as OpenCode commands. They are accessible only via the TypeScript/Python SDK.
3. **Generated command prompt:** The command markdown describes each parameter as a numbered arg with type hint in natural language. OpenCode passes them as positional strings and the agent extracts them.

**Recommendation:** Option 1 (JSON convention). Emit:
```markdown
---
agent: Researcher
---
Run AnalyzeMarket with these parameters as JSON: $ARGUMENTS

Expected format:
{"companies": ["Apple", "Google"], "depth": 3, "region": "US"}
```

---

### FATAL-04: `retries` Field Has No Valid OpenCode Mapping

**Affected spec:** `specs/25-OpenCode-Integration.md §2.1`

**Finding:**
The spec maps `retries = 3` to `"timeout": 90000` (3 × 30s). This is incorrect:
- OpenCode's `timeout` is the total request timeout in ms — it is NOT a retry count.
- OpenCode has no `retries` field in its provider config.
- Heuristically converting `retries` to `timeout` loses semantic meaning entirely. A 3-retry config does not behave the same as a 90-second timeout.

**Impact:** The `retries` field in `client` blocks silently fails to do anything meaningful at runtime. Developers will expect retry behavior and get none.

**Required fix:** Choose one:
1. **Drop mapping:** `retries` field has no equivalent in OpenCode. Emit a compiler warning: `"warning: retries = N has no equivalent in OpenCode mode. Add retry logic to your workflow's try/catch block."` Do not emit anything for it in `opencode.json`.
2. **Map to provider timeout:** Accept the lossy mapping but document it explicitly in spec/25 with a `// NOTE: approximate — retries semantics differ from timeout` comment.
3. **Add retry wrapper:** The generated command markdown instructs the agent to retry on failure up to N times. This is natural-language "retry" only, not protocol-level.

**Recommendation:** Option 1. Emit a compiler warning, do not silently map to a different semantic.

---

## 2. HIGH Findings

### HIGH-01: `invoke:` Path Resolution Inconsistency Between Spec/25 and Spec/26

**Affected specs:** `specs/25-OpenCode-Integration.md §7`, `specs/26-MCP-Server-Generation.md §7`

**Finding:**
Spec/26 §4 shows the MCP handler using:
```javascript
const modulePath = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),  // = generated/
  "../scripts/search.js"                          // = {workspace}/scripts/search.js
);
```

But spec/26 §7 (invoke expression resolution) says the path is "relative to the workspace root (NOT relative to `generated/`)." These say the same thing computationally (`generated/../scripts` = workspace root `scripts`) but the wording is confusing and implies the implementation is `path.resolve(workspaceRoot, "scripts/search.js")` not the `../` relative form.

Additionally: if `generated/` moves (e.g., `build.output_dir = "dist"`), the `../` relative path breaks.

**Required fix in spec/26:** The generated code MUST resolve the workspace root dynamically, not use a hardcoded `../` parent. Specifically:
```javascript
// CORRECT: derive workspace root from claw.json location, not from generated/ position
const WORKSPACE_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
```
This works for the default `generated/` location. The compiler MUST embed the correct relative path from the actual `output_dir` to the workspace root, not hardcode `..`.

---

### HIGH-02: BAML Integration Is Orphaned

**Affected spec:** `specs/18-BAML-Integration-Layer.md`

**Finding:**
Spec/18 defines the BAML bridge as a component of `openclaw-gateway/src/baml-bridge.ts`. The gateway is retired. The BAML emitter in `clawc` (`src/codegen/baml.rs`) still exists and is still valid — it emits `.baml` files from `.claw` source. But the runtime integration (the BAML bridge that calls `baml_client`) is dead code since the gateway is gone.

**Questions not answered by any spec:**
1. With OpenCode as the OS, who calls the BAML client at runtime?
2. Does OpenCode natively support BAML? (Answer based on research: No, OpenCode does not have BAML integration.)
3. Is BAML still a meaningful target for `clawc`?

**Required action:** Spec/18 must be updated to one of:
1. **BAML target retained for SDK use only:** `--lang baml` emits `.baml` files. Developers who use `--lang ts` can optionally run `npx baml-cli generate` and import the BAML client directly in their application code — outside of OpenCode. The gateway integration section of spec/18 is retired.
2. **BAML target retired:** Remove `--lang baml` from the compiler. Simplify codegen.

**Recommendation:** Option 1. The BAML emitter in `clawc` is valuable for typed LLM orchestration independent of the runtime. Retire only the gateway-specific §5 of spec/18.

---

### HIGH-03: `spec/08` Testing Spec References Dead Specs

**Affected spec:** `specs/08-Testing-Spec.md`

**Finding:**
Spec/08 Step 1 says: "If touching the gateway, read `specs/07-Claw-OS.md`." and "If touching security, read `specs/12-Security-Model.md`."

Section 4 (Security Testing Requirements) lists gateway-specific tests that no longer apply to Claw-owned code:
- Timing-safe API key comparison (OpenCode handles auth)
- Request body size limit (OpenCode handles this)
- Malformed WebSocket (OpenCode handles WebSocket)
- Exit code 137 → `SandboxOOMError` (gateway sandbox is retired)

These tests do not exist in the new architecture and testing them would be testing OpenCode, not Claw.

**Required fixes in spec/08:**
1. Replace gateway references with OpenCode references.
2. Update Section 4 to remove gateway-specific tests. Replace with:
   - MCP server path traversal rejection test
   - MCP server input schema validation test
   - MCP server handler error response (no crash) test
   - OpenCode config JSON validity test
   - Compiler warning on unmappable fields (`retries`) test

---

### HIGH-04: `spec/19` Binary Distribution Has Stale Version-Sync Requirement

**Affected spec:** `specs/19-Binary-Distribution.md §4.2`

**Finding:**
Spec/19 §4.2 states: "The TypeScript execution Gateway must perfectly match the version of the raw AST generated by the Rust compiler." and "The version of the `@claw/cli` NPM package MUST be intrinsically hard-coded to download the exact matching GitHub Release tag."

The gateway is retired. This version-sync requirement was about preventing AST structure drift between the Rust compiler and the TypeScript gateway. With OpenCode as the OS, this synchronization concern no longer applies — there is no TypeScript runtime consuming the AST.

**Required fix:** Spec/19 §4.2 must be updated. The version-sync requirement is reduced to: `@claw/cli` version must match the GitHub Release tag for reproducibility. The semantic (preventing AST drift) no longer applies.

---

### HIGH-05: `claw build` Default Language Breaks Old Workflows

**Affected spec:** `specs/14-CLI-Tooling.md §3`

**Finding:**
Spec/14 sets `"language": "opencode"` as the default in `claw.json`. Existing projects using `"language": "ts"` will be unaffected since they have explicit config. But the `claw init` command generates `"language": "opencode"` as the new default.

If a developer runs `claw build` in an old project without `claw.json`, the default is `opencode`. This is a behavioral change from the old default of `ts`. Developers expecting TypeScript SDK output will be surprised.

**Required fix:** Spec/14 §3 must state the migration behavior explicitly: "If `claw.json` is absent, default to `opencode`. If `claw.json` exists with no `language` field, default to `opencode`. If `claw.json` exists with `"language": "ts"`, honor it." This is the current spec intent but needs explicit documentation of the migration path.

---

## 3. MEDIUM Findings

### MEDIUM-01: `.opencode/` Directory Ownership Is Ambiguous

**Affected spec:** `specs/25-OpenCode-Integration.md §2.3`, `specs/26 §11`

**Finding:**
Spec/25 says `.opencode/agents/` and `.opencode/commands/` are generated by `clawc`. But OpenCode itself generates and uses this directory for user-created agents and commands. If a developer has hand-written files in `.opencode/agents/`, `clawc` will overwrite them on `claw build`.

**Required clarification:** Spec/25 must state:
- `clawc` ONLY writes files with names matching agent/workflow names defined in the `.claw` source.
- Hand-written files in `.opencode/agents/` and `.opencode/commands/` with OTHER names are preserved.
- Generated files are marked with a header comment: `<!-- AUTO-GENERATED by clawc — do not edit directly -->`.
- `.gitignore` should NOT include `.opencode/agents/` and `.opencode/commands/` (unlike `generated/`). These files are useful to commit.

---

### MEDIUM-02: `generated/AGENTS.md` vs. Project-Level `AGENTS.md` Conflict

**Affected spec:** `specs/25-OpenCode-Integration.md §4`

**Finding:**
OpenCode uses an `AGENTS.md` (or `opencode.md`) file in the project root for project context. Claw generates its context doc at `generated/AGENTS.md` and references it via `"instructions": ["generated/AGENTS.md"]` in `opencode.json`.

If the developer already has a project-level `AGENTS.md`, the `instructions` array in `opencode.json` will point to the generated one in `generated/`, potentially conflicting with or ignoring the hand-written one.

**Required fix:** Spec/25 §4 must specify:
- The generated file is named `generated/claw-context.md` (not `AGENTS.md`) to avoid collision.
- `opencode.json` `instructions` is set to `["generated/claw-context.md"]`.
- If a project-level `AGENTS.md` exists, the compiler appends it: `"instructions": ["AGENTS.md", "generated/claw-context.md"]`.

---

### MEDIUM-03: `client` Block `small_model` Derivation Is Unspecified

**Affected spec:** `specs/25-OpenCode-Integration.md §3`

**Finding:**
The generated `opencode.json` includes `"small_model": "anthropic/claude-haiku-4-5"`. This is hardcoded and has no corresponding construct in the `.claw` DSL. No `.claw` `client` block specifies a small model. The compiler has no basis for choosing which model is the "small" one.

**Required fix:** Either:
1. Remove `small_model` from generated `opencode.json` entirely (OpenCode uses its own default when absent).
2. Add a `small_model` field to the `client` block DSL syntax and generate it when present.
3. Derive it: if `provider = "anthropic"`, use `claude-haiku-4-5`; if `provider = "openai"`, use `gpt-4o-mini`. Document the derivation rule.

---

### MEDIUM-04: MCP Server Does Not Handle Async Tool Modules

**Affected spec:** `specs/26-MCP-Server-Generation.md §7`

**Finding:**
Spec/26 §7 says "The function MUST be async or return a Promise; if it is synchronous, `await` is a no-op." This is correct for ES module async functions. But if the invoked module function uses callbacks (Node.js-style `callback(err, result)`) rather than Promises, `await` will not work.

**Required fix:** Add to spec/26 §7: "The invoked function MUST be `async` or return a `Promise`. Callback-based functions are NOT supported. If a developer has a callback-based tool implementation, they must wrap it in `util.promisify()` or manually convert it to a Promise."

---

### MEDIUM-05: `opencode.json` Emitted to Project Root Conflicts With Existing Config

**Affected spec:** `specs/25-OpenCode-Integration.md §3`

**Finding:**
If a developer is already using OpenCode in their project with a hand-written `opencode.json`, `clawc build --lang opencode` will overwrite it. This destroys their custom configuration (theme, keybinds, etc.).

**Required fix:** Spec/25 §3 must specify a merge strategy:
- If `opencode.json` does not exist: write fresh.
- If `opencode.json` exists: merge Claw-specific fields (`model`, `mcp`, `instructions`) into the existing file. Preserve all other fields (`theme`, `keybinds`, `formatter`, etc.).
- The merge is NOT a full overwrite. The compiler reads existing `opencode.json`, updates only the fields it owns, and writes back.

---

## 4. LOW Findings

### LOW-01: `spec/25 §5` OpenCode Feature Table Has Unverified Claims

**Affected spec:** `specs/25-OpenCode-Integration.md §5`

**Finding:**
The table claims "Financial circuit breaker → OpenCode cost tracking (Zen platform)." The Claw DSL had a `max_cost_per_session` field that enforced a hard cap. It is unclear whether OpenCode's "Zen platform" actually enforces a per-session hard cost cap or just tracks costs. If it does not hard-cap, the financial circuit breaker is silently removed without replacement.

**Required fix:** Before implementation, verify OpenCode's cost tracking behavior. If it does not hard-cap per session, add a note to spec/25 §5: "Financial circuit breaker (max_cost_per_session) has no direct equivalent in OpenCode. The `max_steps` field on agents provides a proxy safeguard. Developers should monitor costs via their provider dashboard."

---

### LOW-02: Spec/26 Emits ESM Module, But Project May Use CJS

**Affected spec:** `specs/26-MCP-Server-Generation.md §4`

**Finding:**
The generated `mcp-server.js` uses ES module syntax (`import`, `export`, `await` at top level). If the developer's project has `"type": "commonjs"` in `package.json` (or no `"type"` field), Node.js will fail to run the ESM file.

**Required fix:** Spec/14 §2's generated `package.json` must include `"type": "module"`. Spec/26 must note: "The generated MCP server requires `"type": "module"` in `package.json`. `claw init` sets this automatically."

---

### LOW-03: `spec/08` GAN References Need Updating

**Affected spec:** `specs/08-Testing-Spec.md §5` (Gateway Integration Test Patterns)

**Finding:**
Section 5 describes end-to-end tests that "Load a real compiled `document.json`" and "Execute a workflow through the traversal engine." The traversal engine is the retired gateway. These test patterns need updating to reflect the new architecture (MCP server tests + OpenCode integration tests).

---

### LOW-04: Spec/18 BAML Emitter References `openclaw-gateway` Path

**Affected spec:** `specs/18-BAML-Integration-Layer.md §2.2`

**Finding:**
The BAML bridge is at `openclaw-gateway/src/baml-bridge.ts`. This directory is archived. Even if BAML emission from `clawc` is preserved, all gateway code references in spec/18 point to dead paths.

---

## 5. Audit Summary — Pass 1 (Initial Migration Audit)

### Finding Counts
| Severity | Count | Status |
|----------|-------|--------|
| FATAL | 4 | ✅ ALL RESOLVED |
| HIGH | 5 | ✅ ALL RESOLVED |
| MEDIUM | 5 | ✅ ALL RESOLVED |
| LOW | 4 | ✅ ALL RESOLVED |

---

## 6. Verification Checklist — Pass 1

Per `AGENT.md §1.1 Verification Integrity Rules`, per-item verification:

| Finding | Fix Written | Fix Applied To Spec | Cross-Spec Ripple Checked |
|---------|------------|-------------------|--------------------------|
| FATAL-01 SDK transport | ✅ Option 4 chosen | ✅ spec/25 §8.2 | ✅ spec/14 §0, spec/09 §4 |
| FATAL-02 agent mode | ✅ Mode determined by AST usage | ✅ spec/25 §2.3 | ✅ spec/26 (no impact) |
| FATAL-03 multi-param | ✅ JSON convention defined | ✅ spec/25 §2.4 | ✅ spec/14 (claw init uses single-param) |
| FATAL-04 retries | ✅ Compiler warning + no mapping | ✅ spec/25 §2.1 | ✅ spec/14 example.claw uses no retries |
| HIGH-01 path resolution | ✅ Use output_dir-relative root | ✅ spec/26 §7 note | ⚠️ Needs implementation-time verification |
| HIGH-02 BAML orphan | ✅ Gateway section superseded, emitter retained | ✅ spec/18 §5 supersession notice | ✅ spec/18 §1-4 unaffected |
| HIGH-03 testing spec | ✅ Gateway tests replaced with MCP+offline tests | ✅ spec/08 §4, §5, §6 rewritten | ✅ spec/26 §9 consistent |
| HIGH-04 version sync | ✅ Gateway version-sync removed, MCP SDK pinning added | ✅ spec/19 §4.2 updated | ✅ spec/14 consistent |
| HIGH-05 default lang | ✅ Documented in spec/14 §3 | ✅ spec/14 §3 | ✅ spec/09 §4 updated |
| MEDIUM-01 dir ownership | ✅ Preservation rule + header comment | ✅ spec/25 §2.3 notes | ✅ spec/26 §11 consistent |
| MEDIUM-02 AGENTS.md | ✅ Renamed to claw-context.md | ✅ spec/25 §4 | ✅ spec/25 §3 instructions field updated |
| MEDIUM-03 small_model | ✅ Removed — not emitted | ✅ spec/25 §3 table | ✅ No downstream refs |
| MEDIUM-04 async tools | ✅ Documented requirement | ✅ spec/26 §7 | ✅ No downstream refs |
| MEDIUM-05 json merge | ✅ Merge strategy defined | ✅ spec/25 §3 | ✅ spec/14 §2 consistent |
| LOW-01 financial circuit breaker | ✅ OpenCode max_steps proxy documented | ✅ spec/25 §5 note | ✅ No DSL change needed |
| LOW-02 CJS vs ESM | ✅ `"type": "module"` requirement noted | ✅ spec/26 §10, spec/14 §2 | ✅ spec/14 scaffolds correctly |
| LOW-03 spec/08 GAN references | ✅ §5 gateway patterns replaced | ✅ spec/08 §5 rewritten | ✅ consistent with spec/25 §9 |
| LOW-04 spec/18 gateway paths | ✅ Supersession notice added to §5 | ✅ spec/18 §5 | ✅ spec/27 HIGH-02 |

---

## 7. GAN Pass 2 — Full Spec Read Adversarial Audit

**Scope:** After completing Pass 1 fixes, a full re-read of ALL specs was performed. The following additional findings were discovered from actual spec text (no hallucinated fixes).

---

### FATAL-P2-01: `claw test` Has No Execution Engine After Gateway Retirement

**Affected spec:** `specs/17-Phase6-Test-Runner-And-Mocks.md §2, §3.3, §4`

**Finding (sourced from actual spec text):**
Spec/17 §2 states: "the gateway traversal engine executes each `TestDecl.body`"
Spec/17 §3.3 states: "In `executeAgentRun()`, check mock registry BEFORE any LLM/tool routing"
Spec/17 §4 states: "Create `openclaw-gateway/src/test-runner.ts`"
Spec/17 §3 blast radius lists: `openclaw-gateway/src/types.ts`, `openclaw-gateway/src/engine/traversal.test.ts`

ALL of these files are archived. `claw test` had no execution path after migration.

**Fix applied:** Added `specs/17 §7` — new gateway-free test execution model using `generated/claw-test-runner.js`. The compiler emits a standalone Node.js test runner from `test` and `mock` blocks. No gateway, no OpenCode required. Execution is fully offline. Updated the spec header with an architecture update notice.

**Status:** ✅ VERIFIED — Re-read spec/17 §7, new model is present and self-consistent.

---

### HIGH-P2-01: spec/25 §1.3 and §3 Contradicted §4 on Context Document Name

**Affected spec:** `specs/25-OpenCode-Integration.md §1.3, §3, §4`

**Finding (sourced from actual spec text):**
- §1.3 separation-of-concerns table row 7: `generated/AGENTS.md`
- §3 generated `opencode.json` schema: `"instructions": ["generated/AGENTS.md"]`
- §4 explicitly renames it: "The compiler generates a project context document at `generated/claw-context.md` (NOT `AGENTS.md`)"

Direct internal contradiction in spec/25 — §1.3 and §3 said one thing, §4 said another.

**Fix applied:** Updated spec/25 §1.3 table and §3 `opencode.json` schema to use `generated/claw-context.md`. Updated `instructions` description row in §3 table.

**Status:** ✅ VERIFIED — Re-read spec/25 §1.3, §3, §4. All three now say `generated/claw-context.md`.

---

### HIGH-P2-02: spec/26 §11 File Placement Contradicted spec/25 §4

**Affected spec:** `specs/26-MCP-Server-Generation.md §11`

**Finding:** spec/26 §11 file placement tree showed `generated/AGENTS.md` while spec/25 §4 renamed it to `generated/claw-context.md`.

**Fix applied:** Updated spec/26 §11 file placement tree to `generated/claw-context.md`.

**Status:** ✅ VERIFIED — Re-read spec/26 §11. Shows `claw-context.md` with correct spec reference.

---

### HIGH-P2-03: AGENT.md Entirely Stale — References Archived Gateway Throughout

**Affected file:** `AGENT.md §1, §2, §4.2, §4.3, §7, bottom reference list`

**Finding (sourced from actual AGENT.md text):**
- §1 WWDD gate: "If touching the Gateway, read `specs/07-Claw-OS.md`"
- §2 Directory Map: listed `openclaw-gateway/` as the active execution OS with live subdirectories (`src/auth.ts`, `src/ws.ts`, `src/engine/`, `src/tools/`)
- §4.2 "The OS & Gateway Layer (`openclaw-gateway`)" — 9 bullet rules for the archived gateway
- §4.3 "WebSocket Protocol" — 5 bullet rules for the archived WebSocket implementation
- §7 dependency map: `specs/07-Claw-OS.md`, `specs/11-WebSocket-Protocol.md`, `specs/16-Phase6-Gateway-Hardening.md` as active
- Bottom reference list: missing specs 19–27 entirely

**Fix applied:**
- §1 WWDD gate updated: references spec/25 and spec/26 instead of spec/07
- §2 Directory Map: replaced with OpenCode-era layout (`.opencode/`, `generated/mcp-server.js`, `generated/claw-context.md`, `generated/claw-test-runner.js`, `archived/openclaw-gateway/`)
- §4.2 replaced with MCP Server Layer rules (path safety, input validation, error isolation)
- §4.3 replaced with OpenCode Config Layer rules (merge strategy, agent mode, retries warning, context doc name)
- §7 dependency map updated to reference active specs (25, 26, 17 §7, 08, 12 §7)
- Bottom reference list split into "Active" and "Superseded" categories with all specs 19–27 included

**Status:** ✅ VERIFIED — Re-read AGENT.md §1, §2, §4.2, §4.3, §7, bottom list. All references updated.

---

### MEDIUM-P2-01: spec/21 Not Marked Superseded

**Affected spec:** `specs/21-GAN-Audit-State-Resumption.md`

**Finding:** spec/21 audits the gateway checkpoint system (SQLite/Redis, AST hash bindings, drain mode, TypeBox bouncer on resume). It was not marked SUPERSEDED unlike specs 22, 23, 24 which were retired in the same migration pass.

**Fix applied:** Added SUPERSEDED banner to spec/21 header.

**Status:** ✅ VERIFIED — Re-read spec/21 header. SUPERSEDED banner is present.

---

### MEDIUM-P2-02: spec/20 §3 "Runtime Gateway Module" Binary Resolution Is Dead

**Affected spec:** `specs/20-GAN-Audit-Binary-Distribution.md §3`

**Finding:** §3 resolution says: "The runtime Gateway module MUST implement a deterministic binary resolution waterfall" — this was for the gateway finding and loading the `clawc` binary. The gateway is retired; this waterfall has no owner.

**Note:** spec/20 is a historical GAN audit (not an implementation spec), so no code changes derive from it. The finding is informational — implementation guidance now lives in spec/14 and spec/19.

**Status:** ⚠️ ACKNOWLEDGED — No spec fix needed for a historical GAN audit. Documented here for traceability.

---

### MEDIUM-P2-03: spec/17 Blast Radius List Referenced Archived Gateway Files

**Affected spec:** `specs/17-Phase6-Test-Runner-And-Mocks.md §3`

**Finding:** The MockDecl blast radius list included `openclaw-gateway/src/types.ts` and `openclaw-gateway/src/engine/traversal.test.ts`. These are archived. The new blast radius is different (see §7.5).

**Fix applied:** §7.5 in the new OpenCode-era execution model (added in FATAL-P2-01 fix) defines the correct blast radius for the new architecture.

**Status:** ✅ VERIFIED — spec/17 §7.5 lists the correct active files. The old §3 blast radius is still present as historical context for the MockDecl AST migration, which is still valid.

---

### LOW-P2-01: spec/10 Row 6 Referenced Retired spec/07

**Affected spec:** `specs/10-GAN-Final-Audit.md §4 table row 6`

**Finding:** Row 6 pointed to `specs/07-Claw-OS.md §6` for the Anthropic API contract. spec/07 is retired.

**Fix applied:** Row 6 updated to note gateway is archived and LLM routing is now delegated to OpenCode.

**Status:** ✅ VERIFIED — Re-read spec/10 row 6. Now references spec/25 §5.

---

### LOW-P2-02: spec/05 §2 Referenced "Claw Gateway"

**Affected spec:** `specs/05-Type-System.md §2`

**Finding:** "They will be embedded into the final SDK as string literals, ready to be sent to the **Claw Gateway**."

**Fix applied:** Updated to reference the MCP server JSON Schema usage. Now reads: "used as JSON Schema in the generated MCP server for input/output validation."

**Status:** ✅ VERIFIED — Re-read spec/05 §2. No gateway reference remains.

---

## 8. Pass 2 Summary

### Finding Counts
| Severity | Count | Status |
|----------|-------|--------|
| FATAL | 1 (F-P2-01) | ✅ RESOLVED |
| HIGH | 3 (H-P2-01, H-P2-02, H-P2-03) | ✅ ALL RESOLVED |
| MEDIUM | 3 (M-P2-01, M-P2-02, M-P2-03) | ✅ RESOLVED / ACKNOWLEDGED |
| LOW | 2 (L-P2-01, L-P2-02) | ✅ ALL RESOLVED |

**All FATAL and HIGH findings from Pass 2 are resolved. No new FATAL/HIGH findings were introduced by Pass 1 fixes.**

---

## 9. Pass 3 — Convergence Check

**Scope:** Verify that Pass 2 fixes did not introduce new contradictions.

**Checks performed:**

1. **spec/17 §7 new test runner** — references `src/codegen/test_runner.rs` which is consistent with spec/02 §4 Stage 4 CodeGen. The `node:assert` compilation table is consistent with spec/03 `assert_stmt` grammar. ✅
2. **spec/25 §1.3 + §3 + §4 claw-context.md** — all three sections now agree. spec/26 §11 also agrees. ✅
3. **AGENT.md §4.2 MCP path safety rules** — consistent with spec/26 §5 (path safety) and spec/12 §7 (compiler security). ✅
4. **AGENT.md §4.3 OpenCode config rules** — merge strategy consistent with spec/25 §3. agent mode rule consistent with spec/25 §2.3. retries warning consistent with spec/25 §2.1. context doc name consistent with spec/25 §4. ✅
5. **spec/08 §4 new security test table** — compiler panic test consistent with spec/12 §7.1. Path traversal compile-time test consistent with spec/26 §9.1. MCP runtime tests consistent with spec/26 §5-6. ✅
6. **spec/19 §4.2 MCP SDK pinning** — references `@modelcontextprotocol/sdk` version pinning, consistent with spec/26 §10 and spec/14 §2. ✅
7. **spec/18 §5 supersession notice** — correctly identifies that §1-4 remain active (BAML codegen). Does not contradict spec/02 §4 which still lists `--lang baml` as a codegen target. ✅
8. **spec/21 SUPERSEDED banner** — consistent with spec/22/23/24 which also have SUPERSEDED banners. ✅

**Convergence verdict: NO NEW FATAL OR HIGH FINDINGS.** All specs are mutually consistent after Pass 2 fixes. The system is ready for implementation against the current spec set.
