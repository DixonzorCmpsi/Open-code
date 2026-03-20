# Spec 39: Runtime-First Architecture — Claw as the Execution Layer

**Status:** Specced 2026-03-20.
**Depends on:** Spec 25 (OpenCode Integration), Spec 26 (MCP Server Generation), Spec 38 (Closed-Loop Runtime)
**Referenced by future specs:** Spec 40 (claw chat — intent-routing interactive session)

---

## 1. The Problem This Spec Solves

### 1.1 The wrong framing

`AGENT.md §0` currently says:

> "Claw is N8N as code... OpenCode is the execution runtime that runs it."
> "executed with a single `opencode /AddFeaturesToRepo` command."

`claw init` tells users:

```text
4. Run: opencode run --command FindInfo "quantum computing"
```

This framing is wrong and harmful. It makes OpenCode feel **required** for basic execution. It hides the actual value of the language. It puts the OpenCode TUI on the critical path for every user.

The truth is:

- `claw` compiles `.claw` to a self-contained `generated/runtime.js`
- `runtime.js` calls the LLM API **directly** — it does not need OpenCode
- OpenCode is an **optional IDE feature** — useful for interactive chat development, not required for execution

### 1.2 What this costs users today

| Step | Current UX | Root cause |
| --- | --- | --- |
| `claw init` | Warns if OpenCode not installed | `check_opencode_installed()` in `run_init` |
| After `claw build` | Told to run `opencode run --command` | Line 217 of `claw.rs` |
| First execution | Requires `npm install` first | `package.json` dependencies |
| No OpenCode installed | Workflow looks broken | `check_opencode_installed()` prints warning |
| CI/CD | Developer wonders why they need an IDE | OpenCode-centric framing |

### 1.3 The correct framing

**Claw IS the runtime.** The `.claw` language compiles to a self-contained execution artifact (`generated/runtime.js`) that runs workflows directly against any LLM provider using only Node.js built-ins plus optional well-known packages.

OpenCode is one downstream consumer of the build output — useful for IDE-integrated chat development. It is not the execution engine.

```text
CURRENT (wrong):
  .claw → [compiler] → OpenCode config → OpenCode runtime → result

CORRECT:
  .claw → [compiler] → runtime.js → LLM API → result
                     ↓ (optional)
                     → opencode.json + MCP server → OpenCode IDE
```

---

## 2. Zero-Dependency `runtime.js`

The most impactful change: **`runtime.js` uses no npm packages.** It calls LLM APIs via raw `fetch()` — which is built into Node.js 18+.

### 2.1 Current dependency situation

| File | Package | Required for |
| --- | --- | --- |
| `runtime.js` (current mcp.rs) | `@anthropic-ai/sdk` | Anthropic agent handler |
| `mcp-server.js` | `@anthropic-ai/sdk` | Anthropic agent handler (OpenCode path) |
| `mcp-server.js` | `@modelcontextprotocol/sdk` | MCP protocol (OpenCode path only) |

### 2.2 The fix: raw fetch for all providers in `runtime.js`

`runtime.js` replaces SDK imports with inline `fetch()` calls. Both providers are supported:

**Anthropic (raw fetch):**

```javascript
async function callAnthropic(model, systemPrompt, messages, tools, maxTokens = 4096) {
  const response = await fetch("https://api.anthropic.com/v1/messages", {
    method: "POST",
    headers: {
      "x-api-key":         process.env.ANTHROPIC_API_KEY ?? "",
      "anthropic-version": "2023-06-01",
      "content-type":      "application/json",
    },
    body: JSON.stringify({
      model,
      max_tokens: maxTokens,
      system: systemPrompt,
      messages,
      tools: tools.length > 0 ? tools : undefined,
    }),
  });
  if (!response.ok) {
    const text = await response.text();
    throw { code: "E-RUN04", message: `Anthropic API error ${response.status}: ${text}` };
  }
  return response.json();
}
```

**Ollama (raw fetch — already no SDK needed):**

```javascript
async function callOllama(model, systemPrompt, messages, tools) {
  const host = process.env.OLLAMA_HOST ?? "http://localhost:11434";
  const response = await fetch(`${host}/v1/chat/completions`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      model,
      messages: [{ role: "system", content: systemPrompt }, ...messages],
      tools: tools.length > 0 ? tools : undefined,
    }),
  });
  if (!response.ok) {
    const text = await response.text();
    throw { code: "E-RUN04", message: `Ollama error ${response.status}: ${text}` };
  }
  return response.json();
}
```

### 2.3 Result: `runtime.js` requires only Node.js ≥ 18

No `npm install`. No `package.json`. No `node_modules`. A user with Node 18+ installed can run `node generated/runtime.js` immediately after `claw build`.

### 2.4 `mcp-server.js` retains SDK imports (OpenCode path)

`mcp-server.js` still uses `@anthropic-ai/sdk` and `@modelcontextprotocol/sdk`. These are only needed for the OpenCode IDE integration path. Users who want OpenCode integration run `npm install` for those packages. Users who only use `claw run` never need npm.

The generated `package.json` is updated to reflect this separation (§5.3).

---

## 3. Architecture Positioning

### 3.1 Primary execution path

```text
claw build demo.claw       # compile
claw run FindInfo --arg topic="quantum computing"   # execute
```

This is the complete workflow. Node.js must be installed. Nothing else required except an API key in env.

### 3.2 OpenCode IDE enhancement (optional)

If the user wants interactive chat development in the OpenCode IDE:

```text
npm install                 # installs @modelcontextprotocol/sdk, @anthropic-ai/sdk
opencode                    # opens IDE — reads opencode.json, starts mcp-server.js
/FindInfo                   # slash command → runs via MCP
```

This path continues to work unchanged. Nothing in this spec removes or degrades OpenCode integration. It is reframed, not removed.

### 3.3 Rule: which path gets what

| Capability | `claw run` path | OpenCode path |
| --- | --- | --- |
| Execute workflows headlessly | Yes — primary | No |
| CI/CD pipelines | Yes | No |
| Interactive chat in terminal | `claw chat` (§4) | No |
| Interactive chat in IDE | No | Yes |
| LLM tool calling | Yes — direct fetch | Yes — via MCP |
| Type-validated output | Yes | Partial (MCP boundary) |
| npm required | No | Yes |
| OpenCode installed | No | Yes |

---

## 4. `claw chat` — Terminal ChatOps Without the TUI

Adds an interactive REPL that executes workflows conversationally from the terminal. This is the "ChatOps" capability — the terminal equivalent of what the OpenCode chat window provides for users of the IDE.

### 4.1 Usage

```bash
claw chat [--workflow <name>] [--client <name>]
```

Without `--workflow`: enters a workflow selection prompt.
With `--workflow`: enters directly into argument prompting for that workflow.

### 4.2 Session example

```text
$ claw chat
Claw Chat  ·  project: my-project  ·  3 workflows available

  FindInfo    (topic: string) -> SearchResult
  Summarize   (text: string, style: string) -> Summary
  AnalyzePR   (pr_number: int, repo: string) -> PRAnalysis

> FindInfo
  topic: quantum computing

[running FindInfo...]

{
  "url": "https://arxiv.org/quantum",
  "snippet": "Quantum computing leverages quantum-mechanical phenomena...",
  "confidence_score": 0.91
}

> Summarize
  text: (paste or type, end with Ctrl+D)
  style: concise
...
```

### 4.3 How it works

`claw chat` is a thin Rust readline loop that:

1. Reads `generated/runtime.js` PLANS to discover available workflows and their `requiredArgs`.
2. Presents a workflow menu or accepts workflow name directly.
3. Prompts for each required argument interactively.
4. Spawns `node generated/runtime.js <workflow> --arg key=value ...` (same as `claw run`).
5. Parses stdout (JSON) and pretty-prints it.
6. Loops back to prompt.

`claw chat` is NOT an LLM conversation. It does not interpret natural language. It does not hold conversational context between executions. It is a structured workflow invocation shell. Natural language intent routing is deferred to Spec 40.

### 4.4 Type signature

```rust
#[derive(Debug, clap::Args)]
struct ChatArgs {
    /// Jump directly to this workflow (skips menu)
    #[arg(long)]
    workflow: Option<String>,
    /// Override client (same as claw run --client)
    #[arg(long)]
    client: Option<String>,
    /// Output raw JSON instead of pretty-printing
    #[arg(long)]
    raw: bool,
}
```

### 4.5 `runtime.js` plan introspection

`claw chat` reads plan metadata without executing a workflow. It spawns:

```bash
node generated/runtime.js --list
```

`runtime.js` adds a `--list` flag to its CLI entry:

```javascript
if (rawArgs.includes("--list")) {
  const plans = Object.values(PLANS).map(p => ({
    name:         p.name,
    requiredArgs: p.requiredArgs,
    returnType:   p.returnType,
  }));
  process.stdout.write(JSON.stringify(plans) + "\n");
  process.exit(0);
}
```

This keeps `claw chat` decoupled from the Rust codegen — it introspects the compiled artifact, not the AST.

---

## 5. Updated `claw init` and `claw build` Messaging

### 5.1 `claw init` — remove OpenCode requirement

**Remove** the call to `check_opencode_installed()` from `run_init`. OpenCode is not required.

**Updated `claw init` success message:**

```text
✓ Created example.claw
✓ Created claw.json
✓ Created scripts/search.js (stub)
✓ Created .gitignore

Next steps:
  1. Set your API key:   export ANTHROPIC_API_KEY=sk-ant-...
  2. Compile:            claw build
  3. Run:                claw run FindInfo --arg topic="quantum computing"

Optional — OpenCode IDE integration:
  npm install            (installs OpenCode MCP packages)
  opencode               (opens IDE with slash command support)
```

### 5.2 `claw build` — show `claw run` as next step

At the end of a successful `BuildLanguage::Opencode` build, print the available workflows with their run commands:

```text
✓ Built example.claw

  generated/runtime.js      ← primary runtime (no npm required)
  generated/mcp-server.js   ← OpenCode IDE integration (optional)
  opencode.json             ← OpenCode config (optional)

Workflows compiled:
  claw run FindInfo --arg topic=<value>

Tip: claw chat for interactive workflow execution
     claw dev   to watch and rebuild on change
```

If the document has no workflows, omit the "Workflows compiled" section.

### 5.3 Updated `package.json` template

The `claw init` generated `package.json` no longer lists npm packages in `dependencies`. Those are only needed for the OpenCode path.

**Before:**

```json
{
  "dependencies": {
    "@modelcontextprotocol/sdk": "^1.12.0"
  }
}
```

**After:**

```json
{
  "name": "my-claw-project",
  "type": "module",
  "scripts": {
    "build": "claw build",
    "dev":   "claw dev"
  },
  "//": "Run `npm install` only if you use the OpenCode IDE integration path.",
  "optionalDependencies": {
    "@modelcontextprotocol/sdk": "^1.12.0",
    "@anthropic-ai/sdk":         "^0.30.0"
  }
}
```

`optionalDependencies` installs silently if npm is run, but does not error if omitted. `claw run` (which uses `runtime.js`) does not need them.

### 5.4 Remove `package.json` from `claw init` output if Node tooling is absent

If Node.js is not detected on PATH, skip writing `package.json` entirely. Print:

```text
Note: Node.js not found. Install Node.js ≥ 18 to use `claw run` and `claw chat`.
      OpenCode integration also requires Node.js.
```

---

## 6. Updated Vision Statement (AGENT.md §0)

The product vision in `AGENT.md §0` must be updated to reflect the correct architecture. This is not cosmetic — the current vision statement has propagated incorrect framing into the codebase.

**Replace:**

> Claw is N8N as code. It is a statically-typed, deterministic orchestration language that compiles `.claw` source files into native OpenCode configuration. Think of it as the relationship between SQL and a database engine — Claw is the high-level typed language; OpenCode is the execution runtime that runs it.

**With:**

> **Claw is N8N as code.** It is a statically-typed, deterministic orchestration language with its own runtime. A developer writes `.claw` source once. The compiler verifies types, validates agent boundaries, and emits `generated/runtime.js` — a self-contained workflow executor that calls LLM providers directly. No OpenCode required.
>
> OpenCode is supported as an optional IDE backend: `claw build` also emits `opencode.json` and `mcp-server.js` for users who want interactive chat development in the OpenCode IDE. But it is an enhancement, not the execution engine.

**Replace the canonical use case conclusion:**

> "...executed with a single `opencode /AddFeaturesToRepo` command."

**With:**

> "...executed with `claw run AddFeaturesToRepo --arg features=features.md`, or interactively via `claw chat`."

---

## 7. Codegen Changes

### 7.1 `src/codegen/shared_js.rs` — add raw-fetch LLM loops

Two new exported functions alongside the SDK-based ones:

```rust
/// Anthropic via raw fetch — zero npm dependencies
pub fn emit_llm_loop_anthropic_fetch(
    fn_name: &str,
    system_prompt: &str,
    model: &str,
    max_steps: u32,
    temperature: f64,
    tools_js: &str,
) -> String

/// Ollama via fetch — already zero npm deps, same signature for consistency
pub fn emit_llm_loop_ollama_fetch(
    fn_name: &str,
    system_prompt: &str,
    model: &str,
    max_steps: u32,
    temperature: f64,
    tools_js: &str,
) -> String
```

The `_fetch` variants emit raw `fetch()` calls (shown in §2.2). The SDK-based variants remain in `shared_js.rs` for `mcp-server.js` which still uses `@anthropic-ai/sdk`.

### 7.2 `src/codegen/runtime.rs` (Spec 38) — use fetch variants

`emit_agent_runner` in `runtime.rs` calls `shared_js::emit_llm_loop_anthropic_fetch` instead of `emit_llm_loop_anthropic`. This is the only change to `runtime.rs` from Spec 38.

### 7.3 `src/codegen/runtime.rs` — add `--list` flag to CLI entry

In `emit_cli_entry()`, prepend:

```javascript
if (process.argv.includes("--list")) {
  const plans = Object.values(PLANS).map(p => ({
    name: p.name, requiredArgs: p.requiredArgs, returnType: p.returnType,
  }));
  process.stdout.write(JSON.stringify(plans) + "\n");
  process.exit(0);
}
```

### 7.4 `src/bin/claw.rs` changes

| Change | Location | What |
| --- | --- | --- |
| Remove `check_opencode_installed()` call | `run_init` | OpenCode is optional |
| Update success message | `run_init` | Show `claw run` not `opencode run` |
| Update `package.json` template | `run_init` | Use `optionalDependencies` |
| Update build success message | `run_compile_once` | Show `claw run <workflow>` lines |
| Add `Chat(ChatArgs)` variant | `Commands` enum | New subcommand |
| Add `run_chat` function | new | Readline loop (§4) |
| Emit Node.js check (not OpenCode) | `run_init` | Check for Node ≥ 18 |

### 7.5 No changes to `mcp-server.js` codegen

`mcp.rs` is unchanged. `mcp-server.js` continues to use `@anthropic-ai/sdk` and `@modelcontextprotocol/sdk`. The OpenCode integration path is fully preserved.

### 7.6 No new AST nodes, no new DSL syntax

This spec is CLI and codegen changes only.

---

## 8. Node.js Version Check

`claw run` and `claw chat` require Node.js ≥ 18 (for built-in `fetch()`). The Rust binary checks before spawning:

```rust
fn check_node_version(node_path: &str) -> Result<(), ClawCliError> {
    let output = Command::new(node_path)
        .arg("--version")
        .output()
        .map_err(|_| ClawCliError::Message(
            "Node.js not found. Install Node.js ≥ 18: https://nodejs.org".to_owned()
        ))?;
    let version_str = String::from_utf8_lossy(&output.stdout);
    // Parse vMAJOR.MINOR.PATCH
    let major = version_str
        .trim_start_matches('v')
        .split('.')
        .next()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0);
    if major < 18 {
        return Err(ClawCliError::Message(format!(
            "Node.js ≥ 18 required (found {}). Update at https://nodejs.org",
            version_str.trim()
        )));
    }
    Ok(())
}
```

This check runs once per `claw run` or `claw chat` invocation, before spawning the node process. It is fast (single process + exit).

---

## 9. Error and Warning Codes

| Code | Name | Trigger |
| --- | --- | --- |
| E-RT01 | NodeTooOld | Node.js < 18 detected — `fetch()` not built in |
| E-RT02 | NodeNotFound | `node` not on PATH |
| E-RT03 | AnthropicKeyMissing | Provider is `anthropic` but `ANTHROPIC_API_KEY` not set |
| E-RT04 | OllamaNotReachable | `fetch` to Ollama host failed (connection refused) |
| W-RT01 | OpenCodeOptional | `opencode` not installed — `claw run` and `claw chat` still work |

### 9.1 Error examples

```bash
# E-RT01
$ claw run FindInfo --arg topic=test
error[E-RT01]: Node.js 16 detected — claw run requires Node.js ≥ 18
  update at https://nodejs.org

# E-RT02
$ claw run FindInfo --arg topic=test
error[E-RT02]: Node.js not found
  install at https://nodejs.org

# E-RT03 (printed at runtime by runtime.js)
$ claw run FindInfo --arg topic=test
error[E-RT03]: ANTHROPIC_API_KEY not set
  export ANTHROPIC_API_KEY=sk-ant-...
  or use a local Ollama model: set provider = "local" in your .claw client

# W-RT01 (downgraded from current warning/error)
$ claw init
✓ Created example.claw
note: OpenCode IDE not installed — `claw run` works without it
      to enable IDE chat: curl -fsSL https://opencode.ai/install | bash
```

---

## 10. Edge Cases

### 10.1 `fetch()` not available (Node.js < 18, exotic runtimes)

`runtime.js` detects missing `fetch` at startup:

```javascript
if (typeof fetch === "undefined") {
  process.stderr.write(JSON.stringify({
    error: "Node.js ≥ 18 required — built-in fetch() not found",
    code: "E-RT01"
  }) + "\n");
  process.exit(1);
}
```

### 10.2 `ANTHROPIC_API_KEY` not set with cloud model

`callAnthropic` will receive a 401 from the API with a clear error message. This propagates as E-RUN04 (provider error) with the Anthropic error body included in the message.

Better: add a pre-check in `runAgent` before calling the API:

```javascript
if (agent.provider !== "local" && !process.env.ANTHROPIC_API_KEY) {
  throw { code: "E-RT03", message: "ANTHROPIC_API_KEY not set\n  export ANTHROPIC_API_KEY=sk-ant-..." };
}
```

### 10.3 `claw chat` with no compiled runtime.js

Exits with E-RUN05 (same as `claw run`) — runtime not built, run `claw build` first.

### 10.4 OpenCode integration while using `claw run` simultaneously

No conflict. `claw run` spawns its own node process. A running OpenCode instance uses `mcp-server.js`, a separate process. They don't share state.

### 10.5 User runs `npm install` in a claw project

Works fine. Installs `optionalDependencies` which enables the OpenCode integration path. Doesn't break `claw run`.

### 10.6 `claw chat --workflow DoesNotExist`

`claw chat` validates the workflow name against the `--list` output before prompting for args. If not found, prints:

```text
error: workflow "DoesNotExist" not found
available: FindInfo, Summarize, AnalyzePR
```

---

## 11. What This Spec Does NOT Cover

- Natural language intent routing in `claw chat` (map free text to workflow + args) — Spec 40
- Streaming output from `claw run` / `claw chat` — future spec
- `claw serve` HTTP wrapper (spec'd in Spec 38, not changed here)
- Adding new LLM providers beyond Anthropic and Ollama — future spec
- OpenCode removal or degradation — this spec explicitly preserves full OpenCode integration
- Windows PATH handling for `node` binary — deferred (same issue exists today)
