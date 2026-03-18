# OpenClaw AGENT Rules (`AGENT.md`)

This document defines the strict operational rules for any AI Agent (Antigravity, Claude, Codex) contributing to the `Open-code` repository. **Adherence is non-negotiable for project stability, security, and world-class DX.**

---

## 1. WWDD Gates (Anti-Hallucination Guardrails)

Before committing any code or proposing changes, YOU MUST pass the **What Would Developer Do (WWDD)** gates:

- [ ] **Does it compress?**: Is the logic structured and minimal, or is it verbose/repetitive?
- [ ] **Does it stay local?**: Are you avoiding unnecessary external dependencies? (Favor Node/Python built-ins unless security requires an audited library).
- [ ] **Is state observable?**: Can the changes be verified via `ls`, `grep`, or `cat`? Never invent file paths or internal states.
- [ ] **Uses existing primitives?**: Are you using Markdown, YAML, and Git appropriately? Do not invent new configuration formats.
- [ ] **Is it generated/sourced?**: Is this based on `specs/`, transcripts, or verified source files? **YOU MUST read the relevant file in `specs/` BEFORE implementing any module changes.**
- [ ] **Have you checked the spec?**: If touching `src/parser.rs`, read `specs/03-Grammar.md`. If touching the Gateway, read `specs/07-OpenClaw-OS.md`. If touching auth or security, read `specs/12-Security-Model.md`.
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
    - `codegen/`: SDK emission via `minijinja`. (Read: `specs/06-CodeGen-SDK.md`)
    - `config.rs`: openclaw.json configuration. (Read: `specs/14-CLI-Tooling.md`)
    - `lsp.rs`, `bin/claw-lsp.rs`: Language server. (Read: `specs/14-CLI-Tooling.md` Section 6)
    - `bin/openclaw.rs`: CLI commands (init, build, dev). (Read: `specs/14-CLI-Tooling.md`)
- **`openclaw-gateway/` (TypeScript)**: The execution "OS". (Read: `specs/07-OpenClaw-OS.md`)
    - `src/auth.ts`: API key authentication. (Read: `specs/12-Security-Model.md`)
    - `src/ws.ts`: WebSocket protocol. (Read: `specs/11-WebSocket-Protocol.md`)
    - `src/engine/`: Traversal, Checkpointing, LLM bridges, Schema validation.
    - `src/tools/`: Browser automation, Docker sandbox, Vision bridge. (Read: `specs/13-Visual-Intelligence.md`)
- **`packages/` & `python-sdk/`**: Hand-written client libraries (transport only, no schema validation).
- **`generated/`**: Output of `clawc build`. NEVER edit manually. Add to `.gitignore`.
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

### 4.2 The OS & Gateway Layer (`openclaw-gateway`)

- **Security (ALL rules from `specs/12-Security-Model.md`):**
  - API key comparison: `crypto.timingSafeEqual()`. NEVER `===` or `!==`.
  - Request body: enforce `MAX_REQUEST_BODY_SIZE = 1_048_576` before JSON parsing.
  - Session IDs: `crypto.randomUUID()`. NEVER `Date.now()`.
  - Tool paths: `fs.realpath()` + workspace containment check.
  - HTTP responses: include `X-Content-Type-Options: nosniff`, `X-Frame-Options: DENY`.

- **Checkpointing**: EVERY statement type MUST be checkpointed after execution — including `MethodCall`, `BinaryOp`, and `ArrayLiteral`. No expression type is exempt. See `specs/07-OpenClaw-OS.md` Section 2.6.

- **Schema Degradation**: A response is degraded ONLY when ALL leaf values are zero-values simultaneously (`""` AND `0` AND `false`). Individual `0`, `false`, or `""` values are valid data, NOT degradation. See `specs/07-OpenClaw-OS.md` Section 2.4.

- **LLM API Contracts**:
  - **OpenAI**: Use Responses API with `text.format.type = "json_schema"`. (Current implementation is correct.)
  - **Anthropic**: Use `tools` parameter with `input_schema` for constrained output. Extract result from `content[].type === "tool_use"` → `content[].input`. NEVER place `response_schema` inside message content — Anthropic ignores it there. See `specs/07-OpenClaw-OS.md` Section 6.

- **Visual Stability**: Capture screenshots ONLY after the DOM has settled. See `specs/13-Visual-Intelligence.md` Section 1.2.

- **Graceful Shutdown**: On SIGTERM, drain in-flight requests (30s), checkpoint running sessions, close stores. See `specs/07-OpenClaw-OS.md` Section 8.

### 4.3 WebSocket Protocol

- **Prototype/MVP**: Hand-rolled RFC 6455 is acceptable for development and testing.
- **Production (v1.0+)**: MUST migrate to the audited `ws` npm library. Hand-rolled WebSocket implementations lack proper fragmentation, extensions, and backpressure handling.
- **Frame Safety**: Parser MUST bounds-check buffer length before accessing any index. Return "need more data" on incomplete frames, never crash.
- **Close Frames**: Wait for `socket.write()` callback before calling `socket.end()`.
- **Version Negotiation**: Use `Sec-WebSocket-Protocol: openclaw.v1`. The previously proposed `X-OpenClaw-Protocol` header has been **removed**.
- Full protocol: `specs/11-WebSocket-Protocol.md`.

---

## 5. IDE & Tooling

- **Watch Mode**: `openclaw dev` orchestrates the local hot-reload loop (compiler watch + gateway child process). See `specs/14-CLI-Tooling.md` Section 4.
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

Refer to `specs/` for detailed architectural requirements:
- `specs/01-DSL-Core-Specification.md` — Language design and syntax
- `specs/02-Compiler-Architecture.md` — Compiler pipeline and constraints
- `specs/03-Grammar.md` — Formal PEG grammar
- `specs/04-AST-Structures.md` — Rust AST data structures
- `specs/05-Type-System.md` — Semantic analysis rules (3-pass)
- `specs/06-CodeGen-SDK.md` — SDK emission rules
- `specs/07-OpenClaw-OS.md` — Gateway execution contract
- `specs/08-Testing-Spec.md` — TDD methodology
- `specs/09-Implementation-Flow.md` — Build order
- `specs/10-GAN-Final-Audit.md` — Adversarial audit findings
- `specs/11-WebSocket-Protocol.md` — Streaming protocol
- `specs/12-Security-Model.md` — Security invariants
- `specs/13-Visual-Intelligence.md` — Vision system
- `specs/14-CLI-Tooling.md` — CLI commands and LSP
- `specs/15-Phase6-Compiler-Completeness.md` — try/catch, break/continue, binary ops, circular types, exhaustive returns
- `specs/16-Phase6-Gateway-Hardening.md` — graceful shutdown, visual stability, production WebSocket, rate limiting
- `specs/17-Phase6-Test-Runner-And-Mocks.md` — openclaw test command, mock registry, test execution
- `specs/18-BAML-Integration-Layer.md` — BAML codegen, agent resolution IR, per-call-site functions
