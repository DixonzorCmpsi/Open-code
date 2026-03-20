# Implementation Prompt: Spec 39 — Runtime-First Architecture

**Project:** Claw DSL compiler (`clawc`) in Rust — `/Users/dixon.zor/Documents/Open-code`
**Spec:** `specs/39-Runtime-First-Architecture.md`
**Prerequisites:**
- **Spec 38 fully implemented first** (`prompts/implement-spec-38.md`): `src/codegen/shared_js.rs`, `src/codegen/runtime.rs`, `Run(RunArgs)` and `Serve(ServeArgs)` in `claw.rs` must exist and tests must pass
- All tests pass: run `INSTA_UPDATE=always ~/.cargo/bin/cargo test` before starting

---

## What you are implementing

Five focused changes — no new DSL syntax, no new AST nodes:

1. **Zero-dependency `runtime.js`** — swap `@anthropic-ai/sdk` for raw `fetch()` in the runtime agent runner. `mcp-server.js` keeps the SDK.
2. **`--list` flag on `runtime.js`** — plan introspection used by `claw chat`.
3. **`claw chat`** — interactive REPL subcommand for terminal ChatOps.
4. **Fixed `claw init` DX** — remove OpenCode requirement, show `claw run` as primary next step.
5. **Fixed `claw build` DX** — success message prints `claw run <workflow>` lines for each compiled workflow.

---

## Existing codebase orientation

Read these files FIRST:

- `src/codegen/mcp.rs` lines 410–490 — `emit_agent_handler_anthropic`: the SDK-based handler to compare against
- `src/codegen/mcp.rs` lines 492–580 — `emit_agent_handler_ollama`: already uses raw fetch (model for runtime.rs)
- `src/codegen/shared_js.rs` — after Spec 38: contains `emit_llm_loop_anthropic` and `emit_llm_loop_ollama`
- `src/codegen/runtime.rs` — after Spec 38: `emit_agent_runner` calls `emit_llm_loop_anthropic`
- `src/bin/claw.rs` lines 132–220 — `run_init`: `check_opencode_installed()` on line 133, wrong message on line 217
- `src/bin/claw.rs` lines 259–309 — `run_compile_once`: the Opencode build block
- `specs/39-Runtime-First-Architecture.md` — full spec

---

## Implementation order

Work in this exact sequence. Run `cargo test` after each task group.

---

### Task 1: Add raw-fetch LLM loops to `shared_js.rs`

The existing `emit_llm_loop_anthropic` in `shared_js.rs` generates code that does:

```javascript
const Anthropic = (await import("@anthropic-ai/sdk")).default;
const client = new Anthropic();
// ...
const response = await client.messages.create({ ... });
```

Add two new functions that generate identical logic but use raw `fetch()` instead of the SDK. These are used by `runtime.rs`. The SDK variants remain unchanged for `mcp.rs`.

**1a. Add `emit_llm_loop_anthropic_fetch` to `src/codegen/shared_js.rs`:**

```rust
/// Anthropic via raw fetch — zero npm dependencies. Used by runtime.rs.
pub fn emit_llm_loop_anthropic_fetch(
    fn_name: &str,
    system_prompt: &str,
    model: &str,
    max_steps: i64,
    temperature: f64,
    tools_filter: &str,  // JS expression for tools array (same as SDK variant)
) -> String {
    format!(r#"async function {fn_name}(task) {{
  if (!process.env.ANTHROPIC_API_KEY) {{
    throw {{ code: "E-RT03", message: "ANTHROPIC_API_KEY not set\n  export ANTHROPIC_API_KEY=sk-ant-..." }};
  }}
  const agentTools = {tools_filter};
  const messages = [{{ role: "user", content: task }}];
  let steps = 0;

  while (steps < {max_steps}) {{
    steps++;
    const response = await fetch("https://api.anthropic.com/v1/messages", {{
      method: "POST",
      headers: {{
        "x-api-key":         process.env.ANTHROPIC_API_KEY,
        "anthropic-version": "2023-06-01",
        "content-type":      "application/json",
      }},
      body: JSON.stringify({{
        model:      "{model}",
        system:     "{system_prompt}",
        messages,
        tools: agentTools.length > 0 ? agentTools.map(t => ({{
          name:         t.name,
          description:  t.description,
          input_schema: t.inputSchema,
        }})) : undefined,
        max_tokens:  4096,
        temperature: {temperature},
      }}),
    }});

    if (!response.ok) {{
      const text = await response.text();
      throw {{ code: "E-RUN04", message: `Anthropic API error ${{response.status}}: ${{text}}` }};
    }}

    const data = await response.json();

    if (data.stop_reason === "end_turn") {{
      return data.content.find(b => b.type === "text")?.text ?? "";
    }}

    if (data.stop_reason === "tool_use") {{
      const toolUseBlocks = data.content.filter(b => b.type === "tool_use");
      messages.push({{ role: "assistant", content: data.content }});
      const toolResults = [];
      for (const toolUse of toolUseBlocks) {{
        let resultContent;
        try {{
          resultContent = JSON.stringify(await callTool(toolUse.name, toolUse.input));
        }} catch (e) {{
          resultContent = `Error: ${{e.message ?? String(e)}}`;
        }}
        toolResults.push({{ type: "tool_result", tool_use_id: toolUse.id, content: resultContent }});
      }}
      messages.push({{ role: "user", content: toolResults }});
      continue;
    }}

    break;
  }}

  return `Agent reached max_steps ({max_steps}) without finishing.`;
}}"#,
        fn_name = fn_name,
        tools_filter = tools_filter,
        max_steps = max_steps,
        model = model,
        system_prompt = system_prompt,
        temperature = temperature,
    )
}
```

**Key differences from the SDK variant:**
- No `import("@anthropic-ai/sdk")` — zero npm deps
- Pre-checks `ANTHROPIC_API_KEY` before calling (cleaner error message)
- Returns a `String` (the agent's text output) rather than an MCP `{ content: [{ type: "text", text }] }` response object — `runtime.js` doesn't speak MCP protocol
- Tool dispatch calls `callTool(name, input)` instead of `HANDLERS[name](input)` (runtime uses `callTool` dispatcher)

**1b. Add `emit_llm_loop_ollama_fetch` to `src/codegen/shared_js.rs`:**

The Ollama variant in `mcp.rs` already uses raw fetch. This is the same logic adapted for `runtime.js` (returns a `String`, uses `callTool`):

```rust
pub fn emit_llm_loop_ollama_fetch(
    fn_name: &str,
    system_prompt: &str,
    model: &str,
    max_steps: i64,
    temperature: f64,
    tools_filter: &str,
) -> String {
    format!(r#"async function {fn_name}(task) {{
  const host = process.env.OLLAMA_HOST ?? "http://localhost:11434";
  const agentTools = {tools_filter};
  const messages = [
    {{ role: "system", content: "{system_prompt}" }},
    {{ role: "user",   content: task }},
  ];
  let steps = 0;

  while (steps < {max_steps}) {{
    steps++;
    const res = await fetch(`${{host}}/v1/chat/completions`, {{
      method: "POST",
      headers: {{ "content-type": "application/json" }},
      body: JSON.stringify({{
        model: "{model}",
        messages,
        tools: agentTools.length > 0 ? agentTools.map(t => ({{
          type: "function",
          function: {{ name: t.name, description: t.description, parameters: t.inputSchema }},
        }})) : undefined,
        stream: false,
        temperature: {temperature},
      }}),
    }});

    if (!res.ok) {{
      const text = await res.text();
      throw {{ code: "E-RUN04", message: `Ollama error ${{res.status}}: ${{text}}\n  hint: start Ollama with \`ollama serve\`` }};
    }}

    const data = await res.json();
    const choice = data.choices?.[0];
    if (!choice) break;

    if (choice.finish_reason === "stop" || choice.finish_reason === "length") {{
      return choice.message?.content ?? "";
    }}

    if (choice.finish_reason === "tool_calls") {{
      const toolCalls = choice.message?.tool_calls ?? [];
      messages.push({{ role: "assistant", content: choice.message?.content ?? null, tool_calls: toolCalls }});
      for (const tc of toolCalls) {{
        let resultContent;
        try {{
          const tcArgs = typeof tc.function.arguments === "string"
            ? JSON.parse(tc.function.arguments)
            : tc.function.arguments;
          resultContent = JSON.stringify(await callTool(tc.function.name, tcArgs));
        }} catch (e) {{
          resultContent = `Error: ${{e.message ?? String(e)}}`;
        }}
        messages.push({{ role: "tool", tool_call_id: tc.id, content: resultContent }});
      }}
      continue;
    }}

    break;
  }}

  return `Agent reached max_steps ({max_steps}) without finishing.`;
}}"#,
        fn_name = fn_name,
        tools_filter = tools_filter,
        max_steps = max_steps,
        system_prompt = system_prompt,
        model = model,
        temperature = temperature,
    )
}
```

**After Task 1:** Run `cargo test`. No test failures — these are new functions, nothing calls them yet.

---

### Task 2: Wire fetch variants into `runtime.rs`

In `src/codegen/runtime.rs` (written in Spec 38), find `emit_agent_runner`. It currently calls `shared_js::emit_llm_loop_anthropic` and `shared_js::emit_llm_loop_ollama`. Change both calls to the `_fetch` variants:

```rust
// Before (Spec 38):
shared_js::emit_llm_loop_anthropic(fn_name, system_prompt, model, max_steps, temperature, tools_js)
shared_js::emit_llm_loop_ollama(fn_name, system_prompt, model, max_steps, temperature, tools_js)

// After (Spec 39):
shared_js::emit_llm_loop_anthropic_fetch(fn_name, system_prompt, model, max_steps, temperature, tools_js)
shared_js::emit_llm_loop_ollama_fetch(fn_name, system_prompt, model, max_steps, temperature, tools_js)
```

`mcp.rs` continues to call the SDK variants — do not change `mcp.rs`.

**After Task 2:** Run `cargo test`. Verify:

```bash
cargo test test_codegen_runtime_js_agent_uses_direct_llm -- --nocapture
```

The test asserts `!content.contains("@anthropic-ai/sdk")`. It must pass now.

Also verify that the MCP test still uses the SDK:

```bash
cargo test test_codegen_mcp_server_js -- --nocapture
```

This asserts `content.contains("@anthropic-ai/sdk")`. It must still pass.

---

### Task 3: Add `--list` flag to `runtime.js` CLI entry

In `src/codegen/runtime.rs`, find `emit_cli_entry()`. Prepend the `--list` handler before the workflow execution:

```rust
fn emit_cli_entry() -> &'static str {
    r#"
// ── CLI Entry ─────────────────────────────────────────────────────────────────

if (typeof fetch === "undefined") {
  process.stderr.write(JSON.stringify({
    error: "Node.js >= 18 required — built-in fetch() not available",
    code: "E-RT01"
  }) + "\n");
  process.exit(1);
}

function parseArgs(rawArgs) {
  const result = {};
  for (let i = 0; i < rawArgs.length; i++) {
    if (rawArgs[i] === "--arg" && i + 1 < rawArgs.length) {
      const eq = rawArgs[++i].indexOf("=");
      if (eq !== -1) result[rawArgs[i].slice(0, eq)] = rawArgs[i].slice(eq + 1);
    }
  }
  return result;
}

function exitCodeFor(code) {
  const MAP = { "E-RUN01": 1, "E-RUN02": 2, "E-RUN03": 3, "E-RUN04": 4, "E-RUN05": 5, "E-RT01": 1, "E-RT03": 4 };
  return MAP[code] ?? 1;
}

const [,, workflowName, ...rawArgs] = process.argv;

// --list: print plan metadata for claw chat introspection
if (workflowName === "--list") {
  const plans = Object.values(PLANS).map(p => ({
    name: p.name, requiredArgs: p.requiredArgs, returnType: p.returnType ?? null,
  }));
  process.stdout.write(JSON.stringify(plans) + "\n");
  process.exit(0);
}

const args = parseArgs(rawArgs);

(async () => {
  try {
    const result = await executeWorkflow(workflowName, args);
    process.stdout.write(JSON.stringify(result) + "\n");
    process.exit(0);
  } catch (err) {
    process.stderr.write(JSON.stringify({ error: err.message ?? String(err), code: err.code ?? "E-RUN99" }) + "\n");
    process.exit(exitCodeFor(err.code));
  }
})();
"#
}
```

**After Task 3:** Run `cargo test`. Add a test to `codegen_tests.rs`:

```rust
#[test]
fn test_codegen_runtime_js_list_flag() {
    let doc = parser::parse(FULL_DOC).expect("parse failed");
    let dir = tempdir().unwrap();
    codegen::generate_runtime(&doc, dir.path()).expect("codegen failed");
    let content = fs::read_to_string(dir.path().join("generated/runtime.js")).unwrap();
    assert!(content.contains("--list"), "should have --list flag handler");
    assert!(content.contains("PLANS").count() > 1 || content.contains("Object.values(PLANS)"),
        "should reference PLANS in --list handler");
    // Node >= 18 check must be present
    assert!(content.contains("fetch") && content.contains("E-RT01"),
        "should check for fetch() availability");
}
```

---

### Task 4: Fix `claw init` — remove OpenCode requirement

In `src/bin/claw.rs`, `run_init`:

**4a. Remove `check_opencode_installed()` call:**

```rust
fn run_init(args: InitArgs) -> Result<(), ClawCliError> {
    // REMOVE: check_opencode_installed();   ← delete this line
    ...
}
```

**4b. Update the `package.json` template** (around line 183):

```rust
let package_json = r#"{
  "name": "my-claw-project",
  "type": "module",
  "scripts": {
    "build": "claw build",
    "dev":   "claw dev"
  },
  "//": "Run npm install only for OpenCode IDE integration (optional).",
  "optionalDependencies": {
    "@modelcontextprotocol/sdk": "^1.12.0",
    "@anthropic-ai/sdk":         "^0.30.0"
  }
}"#;
```

**4c. Update the success message** (line 216–217):

```rust
println!("✓ Created example.claw\n✓ Created claw.json\n✓ Created scripts/search.js (stub)\n✓ Created .gitignore\n");
println!(
    "Next steps:\n  1. Set your API key:   export ANTHROPIC_API_KEY=sk-ant-...\n  2. Compile:            claw build\n  3. Run:                claw run FindInfo --arg topic=\"quantum computing\"\n\nOptional — OpenCode IDE integration:\n  npm install            (installs OpenCode MCP packages)\n  opencode               (opens IDE with slash command support)\n\nTip: Run `claw dev` to watch for changes and auto-rebuild."
);
```

**4d. Add Node.js check to `run_init`** — warn if Node < 18, but do NOT block:

After removing `check_opencode_installed()`, add:

```rust
// Check Node.js version — required for claw run
match find_node_binary() {
    Some(node) => {
        if let Err(e) = check_node_version(&node) {
            println!("note: {}", e);
        }
    }
    None => {
        println!("note: Node.js not found — install Node.js >= 18 to use `claw run` and `claw chat`");
        println!("      https://nodejs.org");
    }
}
```

**4e. Add `check_node_version` helper** (new function in `claw.rs`):

```rust
fn check_node_version(node_path: &str) -> Result<(), ClawCliError> {
    let output = Command::new(node_path)
        .arg("--version")
        .output()
        .map_err(|_| ClawCliError::Message(
            "Node.js not found. Install Node.js >= 18: https://nodejs.org".to_owned()
        ))?;
    let version_str = String::from_utf8_lossy(&output.stdout);
    let major = version_str
        .trim()
        .trim_start_matches('v')
        .split('.')
        .next()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0);
    if major < 18 {
        return Err(ClawCliError::Message(format!(
            "Node.js >= 18 required for `claw run` (found {}). Update at https://nodejs.org",
            version_str.trim()
        )));
    }
    Ok(())
}
```

**4f. Add Node version check to `run_run` (written in Spec 38):**

At the top of `run_run`, after finding the `node` binary path, add:

```rust
check_node_version(&node).map_err(|e| {
    eprintln!("error[E-RT01]: {e}");
    std::process::exit(1);
})?;
```

**After Task 4:** Run `cargo test`. The `check_opencode_installed` function still exists (it's fine to leave the function, we just stopped calling it from `run_init`). Or delete it if nothing else calls it:

```bash
grep -r "check_opencode_installed" src/
```

If nothing calls it, delete the function body too.

---

### Task 5: Fix `claw build` success message

In `run_compile_once`, at the end of the `BuildLanguage::Opencode` branch, replace the existing `println!("✓ Built ...")` with a message that shows the compiled workflows:

```rust
// After all codegen calls succeed, before the final "✓ Built" line:
let workflow_lines: Vec<String> = document.workflows.iter()
    .map(|w| {
        let args_hint = w.arguments.iter()
            .map(|a| format!("--arg {}=<{}>", a.name, type_name_hint(&a.data_type)))
            .collect::<Vec<_>>()
            .join(" ");
        if args_hint.is_empty() {
            format!("  claw run {}", w.name)
        } else {
            format!("  claw run {} {}", w.name, args_hint)
        }
    })
    .collect();

println!("✓ Built {}", source_path.display());
println!();
println!("  generated/runtime.js     ← primary runtime (no npm required)");
println!("  generated/mcp-server.js  ← OpenCode IDE integration (optional)");
if !document.workflows.is_empty() {
    println!();
    println!("Workflows compiled:");
    for line in &workflow_lines {
        println!("{}", line);
    }
}
println!();
println!("Tip: `claw chat` for interactive execution  |  `claw dev` to watch for changes");
```

Add the helper:

```rust
fn type_name_hint(dt: &crate::ast::DataType) -> &'static str {
    use crate::ast::DataType;
    match dt {
        DataType::String(_)  => "string",
        DataType::Int(_)     => "int",
        DataType::Float(_)   => "float",
        DataType::Boolean(_) => "bool",
        DataType::List(..)   => "list",
        DataType::Custom(..) => "value",
    }
}
```

**After Task 5:** Run `cargo test`. Run `cargo build --bin claw` and manually test:

```bash
echo 'workflow Hello(name: string) { return execute A.run(task: "${name}") }
agent A { system_prompt = "hi" }' > /tmp/hello.claw
~/.cargo/bin/claw build /tmp/hello.claw
```

Expected output:

```
✓ Built /tmp/hello.claw

  generated/runtime.js     ← primary runtime (no npm required)
  generated/mcp-server.js  ← OpenCode IDE integration (optional)

Workflows compiled:
  claw run Hello --arg name=<string>

Tip: `claw chat` for interactive execution  |  `claw dev` to watch for changes
```

---

### Task 6: `claw chat` subcommand

**6a. Add `Chat(ChatArgs)` to the `Commands` enum:**

```rust
#[derive(Debug, Subcommand)]
enum Commands {
    Init(InitArgs),
    Build(BuildArgs),
    Dev(DevArgs),
    Test(TestArgs),
    Run(RunArgs),
    Serve(ServeArgs),
    Chat(ChatArgs),  // NEW
}
```

**6b. Add `ChatArgs` struct:**

```rust
#[derive(Debug, clap::Args)]
struct ChatArgs {
    /// Jump directly to this workflow (skips menu)
    #[arg(long)]
    workflow: Option<String>,
    /// Override client for all agents (default: use compiled client from .claw)
    #[arg(long)]
    client: Option<String>,
    /// Print raw JSON instead of pretty-printing
    #[arg(long)]
    raw: bool,
}
```

**6c. Wire in `run()` match:**

```rust
Commands::Chat(args) => run_chat(args),
```

**6d. Implement `run_chat`:**

```rust
fn run_chat(args: ChatArgs) -> Result<(), ClawCliError> {
    let project_root = find_project_root().ok_or_else(|| {
        ClawCliError::Message("no claw.json found in current directory or any parent".to_owned())
    })?;

    let runtime_path = project_root.join("generated").join("runtime.js");
    if !runtime_path.exists() {
        return Err(ClawCliError::Message(
            "error[E-RUN05]: runtime not built — run `claw build` first".to_owned()
        ));
    }

    let node = find_node_binary().ok_or_else(|| {
        ClawCliError::Message("Node.js not found — install Node.js >= 18: https://nodejs.org".to_owned())
    })?;

    // Introspect available plans via --list
    let list_output = Command::new(&node)
        .arg(&runtime_path)
        .arg("--list")
        .output()
        .map_err(|e| ClawCliError::Message(format!("failed to introspect runtime: {e}")))?;

    let plans: Vec<serde_json::Value> = serde_json::from_slice(&list_output.stdout)
        .unwrap_or_default();

    if plans.is_empty() {
        return Err(ClawCliError::Message(
            "no workflows compiled — add a workflow block to your .claw file and run `claw build`".to_owned()
        ));
    }

    // Print header
    println!("Claw Chat  ·  {} workflow{} available",
        plans.len(), if plans.len() == 1 { "" } else { "s" });
    println!();
    for plan in &plans {
        let name = plan["name"].as_str().unwrap_or("?");
        let req_args: Vec<&str> = plan["requiredArgs"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();
        let return_type = plan["returnType"].as_str().unwrap_or("any");
        if req_args.is_empty() {
            println!("  {}  ->  {}", name, return_type);
        } else {
            println!("  {}  ({})  ->  {}", name, req_args.join(", "), return_type);
        }
    }
    println!();
    println!("Type a workflow name, then enter arguments when prompted.");
    println!("Press Ctrl+C or Ctrl+D to exit.\n");

    // REPL loop
    loop {
        // Prompt for workflow
        let workflow_name = if let Some(ref wf) = args.workflow {
            wf.clone()
        } else {
            eprint!("> ");
            let mut input = String::new();
            match std::io::stdin().read_line(&mut input) {
                Ok(0) => break,  // EOF
                Ok(_) => input.trim().to_owned(),
                Err(_) => break,
            }
        };

        if workflow_name.is_empty() { continue; }
        if workflow_name == "exit" || workflow_name == "quit" { break; }

        // Find plan
        let plan = plans.iter().find(|p| {
            p["name"].as_str().map(|n| n.eq_ignore_ascii_case(&workflow_name)).unwrap_or(false)
        });

        let plan = match plan {
            Some(p) => p,
            None => {
                let available: Vec<&str> = plans.iter()
                    .filter_map(|p| p["name"].as_str())
                    .collect();
                eprintln!("  error: workflow \"{}\" not found", workflow_name);
                eprintln!("  available: {}", available.join(", "));
                // Only continue if workflow was from flag (single shot), break if from REPL
                if args.workflow.is_some() {
                    return Err(ClawCliError::Message(format!(
                        "workflow \"{}\" not found\n  available: {}", workflow_name, available.join(", ")
                    )));
                }
                continue;
            }
        };

        // Prompt for required args
        let req_args: Vec<&str> = plan["requiredArgs"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();

        let mut run_args: Vec<String> = Vec::new();
        for arg_name in &req_args {
            eprint!("  {}: ", arg_name);
            let mut value = String::new();
            match std::io::stdin().read_line(&mut value) {
                Ok(0) => { eprintln!(); break; }
                Ok(_) => {
                    let v = value.trim().to_owned();
                    run_args.push(format!("{}={}", arg_name, v));
                }
                Err(e) => return Err(ClawCliError::Message(e.to_string())),
            }
        }

        // Build node command
        let mut cmd = Command::new(&node);
        cmd.arg(&runtime_path);
        cmd.arg(&workflow_name);
        for kv in &run_args {
            cmd.arg("--arg");
            cmd.arg(kv);
        }
        if let Some(ref client) = args.client {
            cmd.arg("--client");
            cmd.arg(client);
        }

        println!();
        println!("[running {}...]", workflow_name);

        let output = cmd.output()
            .map_err(|e| ClawCliError::Message(format!("failed to spawn node: {e}")))?;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if args.raw {
                print!("{}", stdout);
            } else {
                // Pretty-print JSON if parseable
                match serde_json::from_str::<serde_json::Value>(stdout.trim()) {
                    Ok(v) => println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default()),
                    Err(_) => print!("{}", stdout),
                }
            }
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            eprintln!("  error: {}", stderr.trim());
        }

        println!();

        // If --workflow was specified, run once and exit
        if args.workflow.is_some() {
            break;
        }
    }

    Ok(())
}
```

**After Task 6:** Run `cargo test`. Run `cargo build --bin claw`. Verify:

```bash
~/.cargo/bin/claw chat --help
```

---

### Task 7: Tests

Add to `src/codegen_tests.rs`:

**7a. Verify runtime.js has zero npm imports:**

```rust
#[test]
fn test_codegen_runtime_js_zero_npm_deps() {
    let doc = parser::parse(FULL_DOC).expect("parse failed");
    let dir = tempdir().unwrap();
    codegen::generate_runtime(&doc, dir.path()).expect("codegen failed");
    let content = fs::read_to_string(dir.path().join("generated/runtime.js")).unwrap();

    // runtime.js must NOT import any npm packages
    assert!(!content.contains("@anthropic-ai/sdk"), "runtime.js must NOT import @anthropic-ai/sdk");
    assert!(!content.contains("@modelcontextprotocol/sdk"), "runtime.js must NOT import @modelcontextprotocol/sdk");

    // Must use raw fetch for Anthropic
    assert!(content.contains("api.anthropic.com"), "should use raw Anthropic API endpoint");
    assert!(content.contains("x-api-key"), "should set x-api-key header");
    assert!(content.contains("anthropic-version"), "should set anthropic-version header");
}
```

**7b. Verify mcp-server.js still uses SDK:**

```rust
#[test]
fn test_codegen_mcp_server_still_uses_sdk() {
    let doc = parser::parse(FULL_DOC).expect("parse failed");
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("generated")).unwrap();
    codegen::generate_mcp(&doc, dir.path()).expect("codegen failed");
    let content = fs::read_to_string(dir.path().join("generated/mcp-server.js")).unwrap();

    // mcp-server.js SHOULD use the SDK — it's the OpenCode integration path
    assert!(content.contains("@anthropic-ai/sdk"), "mcp-server.js should still use @anthropic-ai/sdk");
    assert!(content.contains("@modelcontextprotocol/sdk"), "mcp-server.js should still use MCP SDK");
}
```

**7c. Verify --list flag in runtime.js:**

```rust
#[test]
fn test_codegen_runtime_js_has_list_flag() {
    let doc = parser::parse(FULL_DOC).expect("parse failed");
    let dir = tempdir().unwrap();
    codegen::generate_runtime(&doc, dir.path()).expect("codegen failed");
    let content = fs::read_to_string(dir.path().join("generated/runtime.js")).unwrap();
    assert!(content.contains("--list"), "should handle --list flag");
    assert!(content.contains("E-RT01"), "should check for fetch() availability");
}
```

---

### Task 8: Final verification

```bash
# All tests pass
INSTA_UPDATE=always ~/.cargo/bin/cargo test

# Binary builds cleanly
~/.cargo/bin/cargo build --bin claw

# claw init no longer mentions opencode as a requirement
~/.cargo/bin/claw init --force --path /tmp/test39-claw.json 2>&1 | grep -i opencode
# Expected: only optional mention, no "Install it to run compiled workflows"

# claw build shows claw run lines
cat > /tmp/test39.claw << 'EOF'
type Summary { title: string body: string }
client C { provider = "anthropic" model = "claude-haiku-4-5-20251001" }
agent Writer { client = C system_prompt = "Write." }
workflow Summarize(topic: string) -> Summary {
    let r: Summary = execute Writer.run(task: "${topic}", require_type: Summary)
    return r
}
EOF
cd /tmp && ~/.cargo/bin/claw build test39.claw
# Expected: shows "claw run Summarize --arg topic=<string>" NOT "opencode run --command"

# runtime.js has no npm imports
grep -E "@anthropic-ai|@modelcontextprotocol" /tmp/generated/runtime.js
# Expected: empty (no matches)

# runtime.js --list works
node /tmp/generated/runtime.js --list
# Expected: [{"name":"Summarize","requiredArgs":["topic"],"returnType":"Summary"}]

# claw chat help
~/.cargo/bin/claw chat --help

# Confirm mcp-server.js still has SDK (OpenCode path preserved)
grep "@anthropic-ai" /tmp/generated/mcp-server.js
# Expected: found — OpenCode path unchanged
```

---

## Invariants — never violate these

1. **All tests pass after every task group.** Run `INSTA_UPDATE=always ~/.cargo/bin/cargo test` after each.
2. **`runtime.js` has zero npm imports.** No `import` from `@anthropic-ai/sdk` or `@modelcontextprotocol/sdk`. Only Node.js built-ins and `fetch()`.
3. **`mcp-server.js` keeps the SDK.** Do NOT touch `src/codegen/mcp.rs`. The OpenCode integration path is fully preserved.
4. **`claw chat` is a structured REPL, not natural language.** It does not interpret free text as intent. It prompts for exact workflow names and named arguments. Natural language routing is Spec 40.
5. **`check_node_version` warns on `claw init`, hard-errors on `claw run`/`claw chat`.** `claw init` is informational. `claw run` and `claw chat` cannot work without Node >= 18 so they must exit with an error code.
6. **`claw chat --workflow` is single-shot.** If `--workflow` is specified, prompt for args once, run once, exit. Do not loop.
7. **OpenCode is optional in all messaging.** After this spec, no success message tells users to install or run OpenCode as a required step.
8. **`find_node_binary()` is the single source of truth for node path.** Do not hardcode `"node"` anywhere new — always call the shared helper.
9. **`emit_llm_loop_anthropic_fetch` and `emit_llm_loop_anthropic` are separate functions.** Do not merge them. `runtime.rs` uses `_fetch`. `mcp.rs` uses the SDK variant. The distinction is intentional and permanent.
10. **All `Statement` match arms include `Statement::Reason { .. }`.** Any new match you write in this spec must include it (emit an `execute_agent` step as a fallback).
