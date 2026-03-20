# Spec 30: First Real Test — Using Claw in a Separate Repo

**Goal:** Get `claw` installed globally and usable in any project directory as a standalone `.claw` file, backed by Ollama running locally.

---

## Current State (verified)

| Item | Status |
|------|--------|
| `claw` binary | Built at `target/debug/claw` — works |
| `claw init` | Scaffolds correctly |
| `claw build` | Compiles and generates correct output |
| `opencode.json` | Correct schema (`agents.coder.model`, `mcpServers`, `contextPaths`) |
| `.opencode/commands/FindInfo.md` | Correct (`$TOPIC`, `agent_Researcher`) |
| `generated/mcp-server.js` | Generated correctly |
| `opencode` | Installed at `~/.opencode/bin/opencode` |
| Ollama | Running at `http://localhost:11434` with `qwen2.5-coder:7b` |
| `cargo` | Available at `~/.cargo/bin/cargo` |
| `node` / `npm` | **NOT installed** — blocker |
| `claw` in PATH | **NOT in PATH** — blocker |

---

## Step 1 — Install Node.js

The MCP server (`generated/mcp-server.js`) requires Node.js. Install via Homebrew:

```bash
brew install node
```

Verify:
```bash
node --version   # expect v20+
npm --version
```

---

## Step 2 — Install `claw` in PATH

Two options — pick one:

### Option A: Copy debug binary (fastest, no rebuild needed)
```bash
cp /Users/dixon.zor/Documents/Open-code/target/debug/claw /usr/local/bin/claw
chmod +x /usr/local/bin/claw
```

### Option B: `cargo install` (builds release binary, stays in sync with source)
```bash
cd /Users/dixon.zor/Documents/Open-code
~/.cargo/bin/cargo install --path . --bin claw
```

This installs to `~/.cargo/bin/claw`. Ensure `~/.cargo/bin` is in PATH:
```bash
echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> ~/.zshrc
source ~/.zshrc
```

Verify:
```bash
claw --version   # expect: clawc 0.1.0
```

---

## Step 3 — Create the Test Repo

In a completely separate directory (not inside `Open-code/`):

```bash
mkdir ~/claw-demo && cd ~/claw-demo
claw init
```

This scaffolds:
```
claw-demo/
  example.claw        ← the .claw program
  claw.json           ← compiler config
  package.json        ← node deps
  scripts/search.js   ← stub tool implementation
  .gitignore
```

---

## Step 4 — Write a Real `.claw` File

Replace `example.claw` with a real use case using local Ollama. The scaffolded file uses `claude-4-sonnet` — swap it for the local model:

```claw
// demo.claw — local Ollama demo
// Run: opencode /Summarize "explain transformers in 2 sentences"

type Summary {
    title: string
    body: string
    confidence: float
}

tool WebSearch(query: string) -> Summary {
    invoke: module("scripts/search").function("run")
}

client LocalQwen {
    provider = "local"
    model = "local.qwen2.5-coder:7b"
}

agent Writer {
    client = LocalQwen
    system_prompt = "You are a concise technical writer. Return a Summary with title, body, and confidence score between 0 and 1."
    tools = [WebSearch]
    settings = {
        max_steps: 3,
        temperature: 0.2
    }
}

workflow Summarize(topic: string) -> Summary {
    let result: Summary = execute Writer.run(
        task: "Write a concise summary about: ${topic}",
        require_type: Summary
    )
    return result
}
```

Save as `demo.claw`. Update `claw.json` to point to it:
```json
{
  "build": {
    "source": "demo.claw",
    "language": "opencode"
  }
}
```

---

## Step 5 — Build

```bash
cd ~/claw-demo
claw build
```

Expected output:
```
✓ Built demo.claw
```

Verify generated files:
```bash
cat opencode.json
# Must have: agents.coder.model = "local.qwen2.5-coder:7b"
# Must have: mcpServers.claw-tools.type = "stdio"

cat .opencode/commands/Summarize.md
# Must contain: $TOPIC
# Must contain: agent_Writer

cat generated/mcp-server.js | grep -E "agent_Writer|WebSearch"
# Must find both
```

---

## Step 6 — Install MCP Dependencies

```bash
cd ~/claw-demo
npm install
```

This installs `@modelcontextprotocol/sdk` (listed in the scaffolded `package.json`).

---

## Step 7 — Run

Set the local model endpoint and run a workflow:

```bash
cd ~/claw-demo
export LOCAL_ENDPOINT=http://localhost:11434
opencode /Summarize "explain transformers in 2 sentences"
```

OpenCode will:
1. Read `opencode.json` → use `local.qwen2.5-coder:7b` via Ollama
2. Load `generated/mcp-server.js` as an MCP server
3. Load `.opencode/commands/Summarize.md` as the `/Summarize` command
4. Execute the workflow via the `agent_Writer` MCP tool
5. Return a `Summary` object

---

## Step 8 — Verify MCP Server Directly

Before running full OpenCode, verify the MCP server starts and lists tools:

```bash
cd ~/claw-demo
node -e "
import('@modelcontextprotocol/sdk/client/index.js').then(async ({ Client }) => {
  const { StdioClientTransport } = await import('@modelcontextprotocol/sdk/client/stdio.js');
  const t = new StdioClientTransport({ command: 'node', args: ['generated/mcp-server.js'] });
  const c = new Client({ name: 'test', version: '1.0.0' });
  await c.connect(t);
  const { tools } = await c.listTools();
  console.log(tools.map(t => t.name));
  await c.close();
});
"
```

Expected output: `[ 'WebSearch', 'agent_Writer' ]`

---

## Success Criteria

| Check | Expected |
|-------|---------|
| `claw --version` | `clawc 0.1.0` |
| `node --version` | `v20+` |
| `claw build` in new repo | `✓ Built demo.claw` |
| `opencode.json` model field | `local.qwen2.5-coder:7b` |
| `Summarize.md` | contains `$TOPIC` and `agent_Writer` |
| MCP tool list | `WebSearch`, `agent_Writer` |
| `opencode /Summarize "..."` | exits 0, returns structured output |

---

## What Gemini Needs to Implement

The compiler itself already works. Gemini's job for this spec is **environment setup only**:

1. Install Node.js (`brew install node`)
2. Install `claw` binary to PATH (Option A or B above)
3. Create `~/claw-demo/`, run `claw init`, replace `example.claw` with `demo.claw` above
4. Run `claw build`, `npm install`
5. Run the MCP server verification (Step 8)
6. Report results of each step

Gemini should NOT modify any source files in `Open-code/`. This is purely environment + first-run validation.

---

## Known Limitations at This Stage

- The stub `scripts/search.js` returns fake data — the `WebSearch` tool is not real yet. The workflow will still run end-to-end using Ollama, but search results are hardcoded.
- `OPENCODE_CONTEXT` env var is not read by OpenCode — the agent system prompt is written to a temp file but not loaded by OpenCode at this point. The agent runs with its default context only. This is an MVP limitation tracked in spec/25 §2.3.
- `claw test` command lists tests but does not execute them yet (mocked runner).
