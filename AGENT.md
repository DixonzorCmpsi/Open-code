# Claw AGENT Rules (`AGENT.md`)

This document defines the strict operational rules for any AI Agent (Antigravity, Claude, Codex) contributing to the `Open-code` repository. **Adherence is non-negotiable for project stability, security, and world-class DX.**

---

## 0. Product Vision — What We Are Building

**Claw is N8N as code.** It is a statically-typed, deterministic orchestration language that compiles `.claw` source files into native OpenCode configuration. Think of it as the relationship between SQL and a database engine — Claw is the high-level typed language; OpenCode is the execution runtime that runs it.

### The Core Problem

OpenCode is powerful: it supports 75+ LLM providers, has an MCP tool protocol, supports named agents and workflow commands, and can be driven entirely from the CLI. But raw OpenCode configuration is hand-written JSON and markdown — untyped, unverified, and not composable. There is no compile-time guarantee that agent A's output matches agent B's input. There is no control flow, no typed loops, no static analysis.

Claw fixes this. A developer writes `.claw` source once. `clawc` verifies types, validates agent boundaries, and emits the complete OpenCode config bundle. OpenCode executes it.

### The Canonical Use Case

The prototype use case that defines this product:

> A workflow that goes to GitHub, opens VS Code (installing it first if it is not present), implements a list of specified features via a coding agent, pulls credentials from `.env` for any CLI authentication steps, commits each feature, and opens a pull request.

Written as a `.claw` program, compiled with `claw build`, and executed with a single `opencode /AddFeaturesToRepo` command.

### Other Core Use Cases

| Use Case | What It Shows |
| -------- | ------------- |
| **Dev environment bootstrap** | Install detection, secret injection from vault, tool orchestration |
| **Multi-repo dependency audit** | `for` loop over repos, conditional branching on `affected`, auto-PR creation |
| **Release engineering pipeline** | Multi-step determinism: bump → changelog → publish NPM → publish PyPI → tag → notify Slack |
| **Automated code review** | CI integration, structured typed output, conditional merge blocking |
| **Incident response** | Monitoring trigger → log query → runbook lookup → fix → human escalation |
| **Data pipeline** | Scrape → normalize → validate → deduplicate → write DB, fully typed at each stage |

Full use case specifications with complete `.claw` source examples are in `specs/28-Use-Cases.md`.

### The Determinism Guarantee

Claw workflows are programs, not prompts. `for feature in features` is a real compiler-verified loop — not a suggestion to an LLM. `require_type: PRResult` is a compile-time contract. Secrets are loaded via MCP tool calls, never injected into prompt strings. Every agent boundary is a typed, validated interface.

**Every implementation decision must make the canonical use case simpler, faster, and more reliable to execute.**

---

## 1. WWDD Gates (Anti-Hallucination Guardrails)

Before committing any code or proposing changes, YOU MUST pass the **What Would Developer Do (WWDD)** gates:

- [ ] **Does it compress?**: Is the logic structured and minimal, or is it verbose/repetitive?
- [ ] **Does it stay local?**: Are you avoiding unnecessary external dependencies? (Favor Node/Python built-ins unless security requires an audited library).
- [ ] **Is state observable?**: Can the changes be verified via `ls`, `grep`, or `cat`? Never invent file paths or internal states.
- [ ] **Uses existing primitives?**: Are you using Markdown, YAML, and Git appropriately? Do not invent new configuration formats.
- [ ] **Is it generated/sourced?**: Is this based on `specs/`, transcripts, or verified source files? **YOU MUST read the relevant file in `specs/` BEFORE implementing any module changes.**
- [ ] **Have you checked the spec?**: If touching `src/parser.rs`, read `specs/03-Grammar.md`. If touching the MCP server emitter or OpenCode config, read `specs/25-OpenCode-Integration.md` and `specs/26-MCP-Server-Generation.md`. If touching compiler security, read `specs/12-Security-Model.md §7`.
- [ ] **Have you checked the security model?**: Does your change handle untrusted input safely? (See `specs/12-Security-Model.md`)
- [ ] **Did you VERIFY, not just claim?**: If you say something is fixed, you MUST have re-read the actual file after your edit to confirm the change is present and handles the specific audit concern. (See §1.1 below.)

---

## 1.1 Verification Integrity (Anti-Lying Rules)

**NEVER claim a fix, change, or task is "done" without physically verifying the result.** Saying "all 19 issues are fixed" when 10 were simply moved to "Non-Goals" is a CRITICAL trust violation. These rules are mandatory:

### The Anti-Hallucination Checklist

1. **Re-read after every edit.** After modifying a file, you MUST re-read the relevant lines of that file to confirm your change actually landed. Do NOT assume "I wrote it, so it's there."
2. **Per-item proof, not summary claims.** When fixing a list of issues (e.g., audit findings), you MUST verify EACH item individually. Never say "all fixed" based on the fact that you attempted edits. Provide per-item status:
   - ✅ **VERIFIED** — I re-read `spec/04.md:182` and confirmed `Span` is now present on `Expr`.
   - ❌ **NOT DONE** — I was unable to apply this fix because [reason].
3. **No "Non-Goal" Avoidance.** If an audit finding identifies a safety or architectural flaw (e.g., missing Spans or broken try/catch), you CANNOT "fix" it by adding it to a "Non-Goals" section. A known vulnerability marked as a "Non-Goal" is STILL a vulnerability.
4. **Syntax validation.** When writing code snippets in specs (Rust, TypeScript, Python), mentally compile the snippet. Check for:
   - Dangling `else` branches after an `.expect()` / `.unwrap()` that removed the `Option`
   - Missing semicolons or braces
   - Type mismatches between the fix and surrounding code
   - Orphaned variables from a half-applied refactor
5. **Cross-reference integrity.** If your fix in Spec A changes behavior that Spec B depends on, you MUST check Spec B for contradictions. Example: commenting out `import_decl` in one part of a grammar file while it's still active in another part of the same file is a contradiction.
6. **Never "fix" by adding comments that contradict active code.** If a grammar rule is still in the document's production rules, adding a comment below saying "this is Phase 7, not implemented" does NOT disable it. Either remove the rule from the active grammar OR keep it with a clear annotation.

### Reporting Template

When reporting on a set of fixes or tasks, you MUST use this format:

| # | Item | Status | Evidence |
|---|------|--------|----------|
| 1 | Add Span to Expr | ✅ VERIFIED | Re-read spec/04.md:182-202, Span now on all variants |
| 2 | Fix try/catch traversal | ❌ NOT DONE | Requires design decision on frame metadata |
| 3 | Remove X-Protocol header | ✅ VERIFIED | AGENT.md:116 now references Sec-WebSocket-Protocol |

**NEVER replace this table with a summary like "all items fixed."**

---

## 2. Directory Stewardship Map

Enforce DSL and architectural boundaries by respecting directory ownership. **Reading the linked spec is a PRE-REQUISITE for work in these directories.**

- **`src/` (Rust)**: The `clawc` compiler foundation. (Read: `specs/02-Compiler-Architecture.md`)
    - `parser.rs`: `winnow` combinators. (Read: `specs/03-Grammar.md`)
    - `semantic/`: The 3-pass type engine. (Read: `specs/05-Type-System.md`)
    - `codegen/`: SDK emission. (Read: `specs/06-CodeGen-SDK.md`)
        - `codegen/opencode.rs`: OpenCode config emitter — `opencode.json`, agent/command markdown. (Read: `specs/25-OpenCode-Integration.md`)
        - `codegen/mcp.rs`: MCP server emitter — `generated/mcp-server.js`. (Read: `specs/26-MCP-Server-Generation.md`)
        - `codegen/test_runner.rs`: Test runner emitter — `generated/claw-test-runner.js`. (Read: `specs/17-Phase6-Test-Runner-And-Mocks.md §7`)
        - `codegen/baml.rs`: BAML emitter — `generated/baml_src/`. (Read: `specs/18-BAML-Integration-Layer.md §1-4`)
        - `codegen/typescript.rs`: TypeScript SDK emitter. (Read: `specs/06-CodeGen-SDK.md`)
        - `codegen/python.rs`: Python SDK emitter. (Read: `specs/06-CodeGen-SDK.md`)
    - `config.rs`: claw.json configuration. (Read: `specs/14-CLI-Tooling.md`)
    - `lsp.rs`, `bin/claw-lsp.rs`: Language server. (Read: `specs/14-CLI-Tooling.md` Section 6)
    - `bin/claw.rs`: CLI commands (init, build, dev, test). (Read: `specs/14-CLI-Tooling.md`)
- **`archived/openclaw-gateway/`**: The retired TypeScript execution gateway. **DO NOT modify.** Historical reference only. (Was: `specs/07-Claw-OS.md`, now superseded by `specs/25-OpenCode-Integration.md`)
- **`.opencode/`**: OpenCode agent and command config. Generated by `clawc build --lang opencode`.
    - `.opencode/agents/*.md`: Per-agent system prompt + settings. (Read: `specs/25-OpenCode-Integration.md §2.3`)
    - `.opencode/commands/*.md`: Per-workflow command templates. (Read: `specs/25-OpenCode-Integration.md §2.4`)
    - **PRESERVE** hand-written files with names NOT matching Claw-declared agents/workflows.
- **`generated/`**: Output of `clawc build`. NEVER edit manually. Add to `.gitignore`.
    - `generated/mcp-server.js`: MCP tool server — all `tool` blocks. (Read: `specs/26-MCP-Server-Generation.md`)
    - `generated/claw-context.md`: OpenCode project context doc. (Read: `specs/25-OpenCode-Integration.md §4`)
    - `generated/claw-test-runner.js`: Offline test runner. (Read: `specs/17-Phase6-Test-Runner-And-Mocks.md §7`)
    - `generated/claw/index.ts`: TypeScript SDK (from `--lang ts`). (Read: `specs/06-CodeGen-SDK.md`)
    - `generated/claw/__init__.py`: Python SDK (from `--lang python`). (Read: `specs/06-CodeGen-SDK.md`)
    - `generated/baml_src/`: BAML project files (from `--lang baml`). (Read: `specs/18-BAML-Integration-Layer.md §4`)
- **`packages/` & `python-sdk/`**: Hand-written client libraries (transport only, no schema validation).
- **`specs/`**: **THE SOURCE OF TRUTH.** Any deviation from specs requires a spec update FIRST.

---

## 3. Test-Driven Development (TDD) — The 7-Step Cycle

**NON-NEGOTIABLE.** Every feature, bug fix, and refactor follows this exact workflow:

### The Cycle

1. **Read the spec.** Identify which spec(s) govern the module you're changing. Read them before writing anything.
2. **Write the test.** Create the `#[test]` (Rust) or `test()` (TypeScript) block with explicit assertions. Include BOTH:
   - **Happy path**: valid input produces expected output
   - **Error path**: invalid input produces specific error type with span/message
   - **Security path** (if applicable): malicious input is rejected
3. **Run the test suite — confirm FAILURE (RED).** If the test passes before you implement, it's testing something that already exists or the test is wrong.
4. **Write the MINIMUM code** to make the test pass. No extra features, no premature abstractions.
5. **Run the test suite — confirm PASS (GREEN).** All tests must pass, not just the new one.
6. **Refactor** for clarity and performance. Keep functions under 50 lines.
7. **Run `cargo clippy` / `eslint` AND the full test suite again.** The refactored code must pass all static analysis.

### What Counts as a Test

| Type | Purpose | Example |
|------|---------|---------|
| Happy path | Valid input → expected output | `test_parse_agent()` returns correct `AgentDecl` |
| Error path | Invalid input → specific error | Missing tool → `CompilerError::UndefinedTool` with span |
| Security path | Malicious input → rejection | >1MB request body → connection reset |
| Regression path | Bug fix → test that would have caught it | Symlink → must fail with "outside workspace" |

### Test Placement

- **Rust:** `#[cfg(test)] mod tests` at the bottom of the module file. Integration tests in `tests/integration.rs`.
- **TypeScript:** `*.test.ts` files adjacent to the module. Use `node:test` and `node:assert/strict`.
- **Snapshots:** Use `cargo-insta` for AST snapshots. Assert against approved golden files.

---

## 4. Layer-Specific Rules

### 4.1 The Compiler Layer (`clawc`)

- **Safe Rust**: Use `thiserror` for error enums. NEVER use `.unwrap()` or `.expect()` on user-derived data. `.expect()` is ONLY permitted with a `// SAFETY:` comment proving the branch is unreachable.
- **Error Recovery**: Collect up to 50 errors per compilation pass before halting. Do not stop at the first error.
- **3-Pass Analysis**: Strictly separate Symbol resolution (Pass 1), Reference validation (Pass 2), and Type checking (Pass 3). See `specs/05-Type-System.md`.
- **Circular Type Detection**: Pass 1 MUST detect circular type references (`type A { b: B }` + `type B { a: A }`).
- **Exhaustive Return Analysis**: Pass 3 MUST verify all workflow code paths reach a `return` statement.
- **SDK Generation**: Templates MUST emit Zod schemas (TS) and Pydantic models (Py) with runtime `.parse()` / `model_validate()` calls at all boundaries. Type assertions (`as Type`) are NOT sufficient.
- **Exit Codes**: Map errors to distinct exit codes per `specs/02-Compiler-Architecture.md` Section 5.

### 4.2 The MCP Server Layer (`generated/mcp-server.js`)

> **Architecture change:** The `openclaw-gateway` TypeScript OS is retired (archived). Runtime security,
> sessions, and WebSocket streaming are delegated to OpenCode. Claw owns only the **compiler** and the
> **generated MCP server**. Rules below apply to the MCP server generator (`src/codegen/mcp.rs`) and
> the generated `mcp-server.js`. See `specs/26-MCP-Server-Generation.md` and `specs/25-OpenCode-Integration.md §6`.

- **Path Safety (MCP server tool handlers):**
  - All `invoke: module(...)` paths MUST be resolved with `path.resolve()` + `fs.realpath()`.
  - Containment check: `path.relative(wsRoot, real)` MUST NOT start with `..` and MUST NOT be absolute.
  - Fail with `Error("Tool module resolves outside workspace: {module}")` on violation — do NOT proceed with `import()`.
  - See `specs/26-MCP-Server-Generation.md §5`.

- **Input Validation**: MCP tool inputs are pre-validated by the MCP SDK against the `inputSchema`. Handlers MUST NOT trust args without schema validation. Output validation runs `validateOutput()` before returning.

- **Error Isolation**: Every tool handler MUST be wrapped in try/catch. Errors return `{ content: [...], isError: true }` — they MUST NOT crash the MCP server process. See `specs/26-MCP-Server-Generation.md §6`.

- **No External Network**: The MCP server is started as a localhost child process by OpenCode. It has no authority to make outbound network calls. Tool implementations (`invoke: module(...)`) are developer-controlled and may make network calls — but the MCP server scaffold itself does not.

### 4.3 OpenCode Config Layer

- **Merge Strategy**: `clawc build --lang opencode` MUST use a merge strategy on `opencode.json` — never overwrite. Read existing file, update only Claw-owned fields (`agents.coder.model`, `mcpServers.claw-tools`, `contextPaths`), write back. Preserve all user fields. See `specs/25-OpenCode-Integration.md §3`.

- **Correct `opencode.json` field names** (sourced from `opencode/opencode-schema.json`):
  - Model goes under `agents.coder.model` — NOT a top-level `model` field.
  - MCP server goes under `mcpServers` — NOT `mcp`. Type is `"stdio"` — NOT `"local"`.
  - Context file goes in `contextPaths` array — NOT `instructions`. Field `instructions` does not exist.
  - API keys are NOT emitted — OpenCode reads `ANTHROPIC_API_KEY` etc. from the environment automatically.

- **No `.opencode/agents/*.md`**: OpenCode does NOT support custom agent markdown files. Named agents from `.claw` are implemented as `agent_<Name>` MCP runner tools in `generated/mcp-server.js`. See `specs/25-OpenCode-Integration.md §2.3`.

- **`retries` Warning**: `client` blocks with `retries = N` and `--lang opencode` MUST emit a compiler warning and NOT emit `retries` in `opencode.json`. There is no OpenCode equivalent. See `specs/25-OpenCode-Integration.md §2.1`.

- **Context Document**: The project context file is `generated/claw-context.md` (NOT `AGENTS.md`). Referenced via `"contextPaths": ["generated/claw-context.md"]` in `opencode.json`. See `specs/25-OpenCode-Integration.md §4`.

- **Command argument variables**: OpenCode command files use `$UPPERCASE_NAME` substitution (regex `\$([A-Z][A-Z0-9_]*)`). Workflow parameter `topic` → `$TOPIC`. NOT `$arguments`.

---

## 5. IDE & Tooling

- **Watch Mode**: `claw dev` orchestrates the local hot-reload loop (compiler watch + gateway child process). See `specs/14-CLI-Tooling.md` Section 4.
- **LSP Foundation**: Keep `claw-lsp` in sync with `clawc` parser/analyzer changes. The LSP reuses `parser::parse()` and `semantic::analyze()` — no code duplication. See `specs/14-CLI-Tooling.md` Section 6.
- **Error Formatting**: All compiler errors MUST include file path, line, column, source line text, and a caret (`^`) pointing to the error span.

---

## 6. Context Management & Documentation

- **Progressive Disclosure**: Do not embed large swathes of the codebase into context. Search for specific modules when needed.
- **Reference Over Copying**: Use `file:line` references (e.g., `src/parser.rs:45`) rather than copy-pasting massive snippets.
- **Conciseness**: Keep documentation, PR logs, and comments extremely concise.
- **Document the "Why"**: Comments MUST explain *why* a choice was made (e.g., "Using timingSafeEqual because === is vulnerable to timing attacks"), not what the code does.
- **Update Specs First**: If an implementation detail deviates from `specs/`, you MUST update the spec file BEFORE changing the code.

---

## 7. Spec Change Protocol — The Ripple-Effect Rule

**Any edit to a file in `specs/` is a structural change to the entire system.** Specs are not documentation — they are the load-bearing walls of the compiler, gateway, SDK, and toolchain. Changing a spec without tracing its downstream effects is like adding wings to a car: the wings might look right on paper, but you haven't asked where they mount, how they affect aerodynamics at 60mph, whether the chassis can handle the load, or what the dashboard software needs to expose to control them. The car still has to *drive*.

**Before touching any spec file, you MUST complete a full Ripple-Effect Analysis.**

---

### 7.1 The Ripple-Effect Analysis (Required Before Every Spec Edit)

A spec change is only safe to apply after you have answered all four layers of impact:

#### Layer 1 — The Change Itself

Precisely define what is being changed. Be specific about what is being *added*, *removed*, or *mutated*. Vague descriptions like "clarify the grammar section" are not acceptable.

- What is the exact old behavior/rule?
- What is the exact new behavior/rule?
- Why is this change necessary? (Link to audit finding, bug, or design decision.)

#### Layer 2 — Internal Spec Dependencies

The 18 specs in `specs/` form an interdependent graph. Before editing, you MUST identify every other spec that **references, depends on, or extends** the section you are changing.

Ask: *"If I change this rule, which other spec sections become false, incomplete, or contradictory?"*

Use this dependency map as your starting checklist:

| If you change... | Check these specs for contradictions |
|------------------|--------------------------------------|
| Grammar (`03`) | AST (`04`), Type System (`05`), CLI (`14`), Phase 6 (`15`), Test Runner (`17`) |
| AST structures (`04`) | Grammar (`03`), Type System (`05`), CodeGen (`06`), Phase 6 (`15`), Test Runner (`17`) |
| Type system rules (`05`) | Grammar (`03`), AST (`04`), CodeGen (`06`), MCP Gen (`26`) |
| CodeGen output format (`06`) | OpenCode Integration (`25`), MCP Gen (`26`), BAML (`18`), CLI (`14`) |
| OpenCode integration contract (`25`) | MCP Gen (`26`), CodeGen (`06`), CLI (`14`), Binary Dist (`19`), Test Runner (`17`) |
| MCP server generation (`26`) | OpenCode Integration (`25`), Security (`12 §7`), Testing (`08`) |
| Security rules (`12 §7` — compiler only) | Grammar (`03`), AST (`04`), CLI (`14`) |
| CLI tooling (`14`) | OpenCode Integration (`25`), Binary Dist (`19`), Test Runner (`17`) |
| Any Phase 6 spec (`15`–`18`) | Core specs (`03`–`06`), GAN Audit (`10`), OpenCode Integration (`25`) |
| GAN Audit findings (`10`, `27`) | Every spec the finding references — check it is still accurate |

If the change introduces a new concept (a new keyword, a new node type, a new error variant), trace it through **every spec that touches that concept** — not just the one you are editing.

#### Layer 3 — Code & Test Dependencies

Specs drive implementation. After identifying affected specs, identify the code that implements them:

- Which Rust modules (`src/`) implement the changed spec section?
- Which TypeScript modules (`openclaw-gateway/`) implement it?
- Which tests currently pass *because* of the rule you are changing? Would they need to be updated?
- Does the change require new error variants in `errors.rs`? New AST nodes in `ast.rs`? New semantic passes?
- Does the change alter the CLI's behavior, the LSP's diagnostics, or the generated SDK's shape?

Write this out explicitly. Do not proceed if you cannot answer it.

#### Layer 4 — Coherence Check

Read the entire changed spec from top to bottom after applying your edit. Then ask:

- Does the spec still read as a coherent, self-consistent document?
- Are there any sentences that are now contradicted by your edit?
- Does the spec still correctly describe the system as it will exist after all downstream code changes are made?
- Does the spec's Non-Goals section need to be updated to reflect what the change explicitly does NOT cover?

---

### 7.2 The Spec Change Checklist

You MUST complete this checklist and include it in your report before any spec edit is accepted:

```
## Spec Change Report: [Short description]

### Change
- File: specs/XX-Name.md
- Section: §N.M
- Old rule: [exact quote or description]
- New rule: [exact new text]
- Reason: [audit ID, bug, design decision]

### Layer 2 — Affected Specs
- [ ] specs/03-Grammar.md — [affected / not affected, and why]
- [ ] specs/04-AST-Structures.md — [affected / not affected, and why]
- [ ] specs/05-Type-System.md — [affected / not affected, and why]
- [ ] specs/06-CodeGen-SDK.md — [affected / not affected, and why]
- [ ] specs/07-Claw-OS.md — [affected / not affected, and why]
- [ ] specs/10-GAN-Final-Audit.md — [affected / not affected, and why]
- [ ] specs/15-Phase6-Compiler-Completeness.md — [affected / not affected, and why]
- [Any other spec from the dependency map above]

### Layer 3 — Affected Code
- Rust: [list src/ files that must change]
- TypeScript: [list openclaw-gateway/ files that must change]
- Tests: [list tests that need update or new tests needed]
- New error variants: [yes/no — name them]
- New AST nodes: [yes/no — name them]

### Layer 4 — Coherence
- [ ] Re-read the entire edited spec top-to-bottom after the edit
- [ ] No sentences in the spec are now self-contradictory
- [ ] Non-Goals updated if needed
- [ ] Confirmed the spec still describes the system correctly end-to-end

### Verification
[Per-item VERIFIED / NOT DONE table per §1.1 above]
```

---

### 7.3 The Cascade Rule

**If a spec change requires changes to other specs, those other specs MUST be updated in the same change set.** You cannot leave downstream specs in a contradictory state and mark the work as "done." All specs in the dependency chain must be consistent before any code is written.

The order of operations is always:
1. Identify all affected specs (Layer 2)
2. Update all affected specs atomically
3. Verify coherence across the full set
4. THEN implement code changes (Layer 3)
5. THEN run tests

Never write code to implement a spec change that has unresolved contradictions in other specs. The code will embed the contradiction and it will be much harder to fix later.

---

### 7.4 What You Are NOT Allowed to Do

- **Do NOT edit a spec to make existing (broken) code "correct."** If the code violates the spec, fix the code. Only change the spec if the spec itself is wrong.
- **Do NOT delete spec text to "simplify" without tracing what relied on it.** Deletion is the most dangerous edit — something downstream was almost certainly counting on that rule.
- **Do NOT add a new rule to one spec without checking if a contradicting rule already exists** in another spec.
- **Do NOT mark a spec change "complete" while any downstream spec still references the old behavior.** Partial consistency is no consistency.

---

Refer to `specs/` for detailed architectural requirements:

**Active specs (implement against these):**
- `specs/01-DSL-Core-Specification.md` — Language design and syntax
- `specs/02-Compiler-Architecture.md` — Compiler pipeline and constraints
- `specs/03-Grammar.md` — Formal PEG grammar
- `specs/04-AST-Structures.md` — Rust AST data structures
- `specs/05-Type-System.md` — Semantic analysis rules (3-pass)
- `specs/06-CodeGen-SDK.md` — SDK emission rules (TS, Python, OpenCode, BAML targets)
- `specs/08-Testing-Spec.md` — TDD methodology
- `specs/09-Implementation-Flow.md` — Build order
- `specs/12-Security-Model.md §7` — Compiler security invariants (§§2-6 superseded by OpenCode)
- `specs/14-CLI-Tooling.md` — CLI commands and LSP
- `specs/15-Phase6-Compiler-Completeness.md` — try/catch, break/continue, binary ops, circular types
- `specs/17-Phase6-Test-Runner-And-Mocks.md` — claw test command; §7 is the active execution model
- `specs/18-BAML-Integration-Layer.md §1-4` — BAML codegen emitter (§5 gateway integration superseded)
- `specs/19-Binary-Distribution.md` — NPM wrapper, binary distribution, proxy support
- `specs/25-OpenCode-Integration.md` — **PRIMARY EXECUTION OS CONTRACT** — replaces specs/07
- `specs/26-MCP-Server-Generation.md` — MCP server generation from `tool` blocks
- `specs/27-GAN-Audit-OpenCode-Migration.md` — Active adversarial audit findings

**Superseded / historical specs (do NOT implement against these):**
- `specs/07-OpenClaw-OS.md` — SUPERSEDED by `specs/25-OpenCode-Integration.md`
- `specs/10-GAN-Final-Audit.md` — Historical audit (references retired gateway specs)
- `specs/11-WebSocket-Protocol.md` — SUPERSEDED (OpenCode handles WebSocket)
- `specs/13-Visual-Intelligence.md` — SUPERSEDED (OpenCode handles browser tools)
- `specs/16-Phase6-Gateway-Hardening.md` — SUPERSEDED (gateway retired)
- `specs/20-GAN-Audit-Binary-Distribution.md` — Historical binary distribution audit
- `specs/21-GAN-Audit-State-Resumption.md` — SUPERSEDED (gateway checkpoint system retired)
- `specs/22-Gateway-State-Resumption-Implementation.md` — SUPERSEDED (gateway retired)
- `specs/23-GAN-Audit-OS-Kernel.md` — SUPERSEDED (gateway retired)
- `specs/24-GAN-Audit-Sandbox-Containers.md` — SUPERSEDED (OpenCode handles sandboxing)
