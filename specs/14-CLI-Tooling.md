# OpenClaw CLI Tooling

This document specifies the `openclaw` CLI binary, its commands, configuration system, and the `claw-lsp` language server.

---

## 1. Command Overview

The `openclaw` binary is built in Rust and distributed as a standalone executable alongside `clawc`.

| Command | Purpose |
|---------|---------|
| `claw init` | Scaffold a new OpenClaw project |
| `claw build` | Compile `.claw` source to SDK files |
| `claw dev` | Hot-reload development server (watch + gateway) |
| `claw test` | Run `.claw` test blocks with mock injection (see `specs/17-Phase6-Test-Runner-And-Mocks.md`) |

---

## 2. `claw init`

**Usage:** `claw init [--path claw.json] [--force]`

**Behavior:**
1. Detect the `.claw` entry file (prefer `example.claw`, fall back to `src/pipeline.claw`)
2. Generate `claw.json` configuration file with sensible defaults
3. If `--force` is not set and the file already exists, exit with an error

**Generated `claw.json` structure:**

```json
{
  "gateway": {
    "url": "http://127.0.0.1:8080",
    "api_key_env": "CLAW_GATEWAY_API_KEY",
    "executable": null,
    "cors_origin": null
  },
  "build": {
    "source": "example.claw",
    "language": "ts",
    "output_dir": "generated/claw"
  },
  "runtimes": {
    "sandbox_backend": "docker",
    "python_image": "python:3.11-slim",
    "node_image": "node:22"
  },
  "llm_providers": [
    { "name": "openai", "api_key_env": "OPENAI_API_KEY", "default_model": "gpt-4o" },
    { "name": "anthropic", "api_key_env": "ANTHROPIC_API_KEY", "default_model": "claude-sonnet-4-6" }
  ]
}
```

These model identifiers are only defaults. Implementations MUST verify provider model availability before release. Provider references:
- OpenAI models: https://platform.openai.com/docs/models
- Anthropic models: https://platform.claude.com/docs/en/about-claude/models/overview

---

## 3. `claw build`

**Usage:** `claw build [source.claw] [--lang ts|python] [--watch] [--config claw.json]`

**Behavior:**
1. Load `claw.json` if no source argument is provided
2. Read `.claw` source file
3. Run the full `clawc` pipeline: parse → semantic analysis → IR lowering → code generation
4. Write output files to `generated/claw/`:
   - `index.ts` (TypeScript SDK) or `__init__.py` (Python SDK)
   - `document.json` (compiled AST for gateway)
   - `documents/{ast_hash}.json` (hash-addressed copy)

**Watch Mode (`--watch`):**
- Monitor the `.claw` source file and `claw.json` for changes
- On file change, re-run the full build pipeline
- Print `rebuilt {path}` on success, print error with line/column context on failure
- If the source path changes in config, update the file watcher
- The Rust CLI implementation MUST use the `notify` crate's cross-platform watcher with a short coalescing window to avoid duplicate rebuilds from single save events. Raw Node.js `fs.watch()` / `fs.watchFile()` are prohibited for this CLI implementation.

**Exit Codes:**

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Parse error (malformed `.claw` syntax) |
| 2 | Semantic error (type mismatch, undefined reference) |
| 3 | Code generation error (template failure) |
| 4 | I/O error (file not found, permission denied) |

**Error Formatting:**
Errors MUST include file path, line number, column number, the source line, and a caret pointing to the exact error location:

```
error: undefined tool reference 'FakeTool'
 --> example.claw:15:22
  |
15 |     tools = [WebSearch, FakeTool]
  |                         ^^^^^^^^
```

---

## 4. `claw dev`

**Usage:** `claw dev [--config claw.json] [--port 8080]`

**Behavior:**
1. Load `claw.json`
2. Run an initial `claw build` (fail fast if the `.claw` file has errors)
3. Resolve and start the `openclaw-gateway` child process on the specified port
4. Enter watch mode on the `.claw` source file
5. On file change, rebuild the SDK (gateway does NOT restart — it loads documents dynamically by `ast_hash`)
6. On `SIGTERM` or `SIGINT` (Ctrl+C), request graceful gateway shutdown and exit cleanly

**Console Output:**
```
[dev] built generated/claw/index.ts
[dev] starting gateway on port 8080
[dev] watching example.claw for changes (ctrl+c to stop)
[dev] rebuilt generated/claw/index.ts
```

**Graceful Shutdown:**
- On signal: first call authenticated `POST /shutdown` on the local gateway
- Wait up to 5 seconds for child to exit
- If child doesn't exit, send `SIGKILL`
- Exit with code 0

**Gateway Resolution Order:**
1. `gateway.executable` from `claw.json` if set
2. `node_modules/.bin/openclaw-gateway` relative to the project root
3. `openclaw-gateway` on `$PATH`
4. Monorepo development fallback: `node --experimental-strip-types openclaw-gateway/src/server.ts` when the source tree is present locally

On Windows, the `.cmd` suffix is used automatically for steps 2 and 3 when present.

---

## 5. Configuration (`claw.json`)

| Field | Type | Description |
|-------|------|-------------|
| `gateway.url` | string | Gateway endpoint URL |
| `gateway.api_key_env` | string | Environment variable name for API key |
| `gateway.executable` | string \| null | Explicit gateway executable path or command override |
| `gateway.cors_origin` | string \| null | Allowed CORS origin. `null` disables CORS headers, `"*"` is development-only, specific origins are recommended for production |
| `build.source` | string | Path to `.claw` source file |
| `build.language` | `"ts"` or `"python"` | SDK target language |
| `build.output_dir` | string | Output directory for generated files |
| `runtimes.sandbox_backend` | `"docker"` or `"local"` | Sandbox execution mode |
| `runtimes.python_image` | string | Docker image for Python sandboxes |
| `runtimes.node_image` | string | Docker image for Node.js sandboxes |
| `llm_providers[].name` | string | Provider name (`openai`, `anthropic`, `custom`) |
| `llm_providers[].api_key_env` | string | Environment variable for API key |
| `llm_providers[].default_model` | string | Default model identifier |

The config file is read by both the Rust CLI and the TypeScript gateway.

---

## 6. Language Server (`claw-lsp`)

The `claw-lsp` binary provides IDE support for `.claw` files via the Language Server Protocol.

**Capabilities:**

| Feature | Implementation |
|---------|---------------|
| Diagnostics | Reuses `clawc` parser and semantic analyzer to report errors in real time |
| Completion | Suggests keywords (`agent`, `workflow`, `tool`, `type`, `client`, `execute`, `return`, `for`, `if`, `let`) and document symbols (defined agents, types, tools) |
| Semantic Tokens | Highlights `.claw` keywords for syntax coloring |

**Architecture:**
- Built with `tower-lsp` in Rust
- On document open/change: re-parse the entire document, run semantic analysis, publish diagnostics
- Completion items are rebuilt on every change from the current AST

**Requirement:** Any change to the `clawc` parser or semantic analyzer MUST be reflected in `claw-lsp`. The LSP reuses the same `parser::parse()` and `semantic::analyze()` functions — no duplication.

---

## 7. Structured Logging

When the `--json` flag is set (future), CLI output uses newline-delimited JSON (ndjson):

```json
{"level":"info","event":"build_complete","path":"generated/claw/index.ts","duration_ms":42}
{"level":"error","event":"parse_error","file":"example.claw","line":15,"column":22,"message":"undefined tool reference 'FakeTool'"}
```

Log levels are controlled by `CLAW_LOG_LEVEL` environment variable: `error`, `warn`, `info`, `debug`. Default: `info`.
