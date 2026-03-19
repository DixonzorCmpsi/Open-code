# Spec 29: Codebase Cleanup

**Goal:** Remove all dead code and stale artifacts from the pre-OpenCode architecture. The custom `openclaw-gateway` has been retired. OpenCode is now the execution OS. This spec defines exactly what to delete, archive, and rename.

---

## Background

The project pivoted from a custom `openclaw-gateway` TypeScript runtime to OpenCode as the execution OS (specs 25–27). That migration left behind ~50+ dead files: the gateway source tree, stale specs, migration scripts, build artifacts, and orphaned directories. This cleanup brings the repo in line with the current architecture.

---

## Tier 1 — Delete Immediately (no information loss)

These are empty dirs, build artifacts, one-off scripts, and task prompts. Delete unconditionally.

### Directories
| Path | Reason |
|------|--------|
| `openclaw/` | Empty placeholder from pre-migration |
| `openclaw-gateway/` | Entire retired gateway codebase (~17 TS files + deps) |
| `test-project/` | Empty orphaned test workspace |

### Root files
| Path | Reason |
|------|--------|
| `nextpromt.txt` | Task prompt for dead gateway code |
| `process_icon.py` | One-off dev script with hardcoded Windows paths |
| `pipeline.claw` | Empty 1-byte file |
| `test_output.txt` | Build artifact |
| `clippy_output.json` | Rust lint artifact (247 KB) |
| `claw.json` | Empty `{}` config, unused |

### Scripts (migration artifacts)
| Path | Reason |
|------|--------|
| `scripts/rename_openclaw.js` | One-off rename migration script, done |
| `scripts/rename_openclaw.cjs` | Same |
| `scripts/detect_renames.py` | Audit script for openclaw→claw rename, done |
| `scripts/results.txt` | Output from above audit |

---

## Tier 2 — Archive Specs (move to `specs/archive/`)

These specs are explicitly superseded. Keep them accessible in git history but remove them from the active spec list so they don't confuse implementors.

| Path | Reason |
|------|--------|
| `specs/07-OpenClaw-OS.md` | Custom gateway OS spec — superseded by spec/25 |
| `specs/11-WebSocket-Protocol.md` | Gateway WebSocket protocol — gateway retired |
| `specs/16-Phase6-Gateway-Hardening.md` | Gateway production hardening — gateway retired |
| `specs/22-Gateway-State-Resumption-Implementation.md` | Gateway checkpoint/resume impl — gateway retired |
| `specs/23-GAN-Audit-OS-Kernel.md` | GAN audit of gateway kernel refactor — irrelevant |
| `specs/24-GAN-Audit-Sandbox-Containers.md` | GAN audit of gateway Docker sandbox — irrelevant |

Move with: `git mv specs/<file> specs/archive/<file>`

---

## Tier 3 — Rename (no delete, just fix the name)

### `packages/openclaw-sdk/` → `packages/claw-sdk/`

The directory is named with the old `openclaw` brand. Rename and update the reference in root `package.json`:

```json
// package.json — change:
"@claw/sdk": "file:packages/openclaw-sdk"
// to:
"@claw/sdk": "file:packages/claw-sdk"
```

Use `git mv packages/openclaw-sdk packages/claw-sdk`.

---

## Tier 4 — Update (rewrite stale content, keep the file)

### `PRODUCTION.md`
- **Remove**: All Docker gateway deployment instructions (§2, §3 — Redis checkpointing, sandbox backends, gateway API keys)
- **Replace with**: OpenCode deployment model — `cargo install clawc`, `claw build`, `opencode` CLI, `LOCAL_ENDPOINT` for local models

### `QUICKSTART.md`
- **Fix**: `git clone` URL uses `openclaw` — change to `claw`
- **Verify**: All `claw init` / `claw build` / `claw dev` commands work against the current binary

### `.env.example`
- **Remove**: Gateway-specific vars (`CLAW_GATEWAY_API_KEY`, `CLAW_SANDBOX_BACKEND`, `REDIS_URL`, etc.)
- **Keep/Add**: `LOCAL_ENDPOINT`, `CLAW_LOCAL_MODEL`, `ANTHROPIC_API_KEY`
- **Template** should match the actual `.env` structure

### `specs/19-Binary-Distribution.md`
- **Remove**: §4.2 "Gateway version sync" — the gateway no longer has a binary
- **Keep**: Compiler (`clawc`) binary distribution via GitHub Releases + npm wrapper

### `specs/18-BAML-Integration-Layer.md`
- **Assess**: If BAML codegen is deferred, move to `specs/archive/`. If still planned, rewrite to describe BAML emission from `clawc` without any gateway execution context.

---

## Tier 5 — Investigate Before Acting

Do not delete these without verifying they're unused.

| Path | What to check |
|------|--------------|
| `npm-cli/` | Is `@claw/cli` npm distribution planned? If yes keep, if deferred delete |
| `packages/zod/` | Is this vendored locally or can it be replaced with `npm install zod`? |
| `scripts/sandbox_echo.py` / `.ts` | Are these referenced by any test? |
| `scripts/youtube_scraper.py` | Used by `python-testing/`? |
| `scripts/test-request.js`, `search.mjs`, `ts-smoke-test.mjs` | Referenced by CI or README? |
| `scripts/install.sh` | Is this the CI install script or dead? |
| `python-testing/` | Do tests assume a running gateway? Need to update for OpenCode. |
| `python-sdk/` | Does codegen still target this SDK? Verify it works with OpenCode output. |

---

## What NOT to Touch

| Path | Reason |
|------|--------|
| `opencode/` | This IS the execution OS (git submodule — OpenCode source) |
| `src/` | Active Rust compiler source |
| `specs/25-OpenCode-Integration.md` | Active spec |
| `specs/26-MCP-Server-Generation.md` | Active spec |
| `specs/27-GAN-Audit-OpenCode-Migration.md` | Active spec |
| `specs/28-Use-Cases.md` | Active spec |
| `specs/29-Codebase-Cleanup.md` | This file |
| `example.claw` | Active demo file |
| `test_e2e.sh` | Active smoke test |
| `AGENT.md` | Core operational guidelines |
| `.env` | Live config |
| `Cargo.toml` | Active build config |

---

## Execution Order

```bash
# 1. Tier 1 deletes
rm -rf openclaw/ openclaw-gateway/ test-project/
rm nextpromt.txt process_icon.py pipeline.claw test_output.txt clippy_output.json claw.json
rm scripts/rename_openclaw.js scripts/rename_openclaw.cjs scripts/detect_renames.py scripts/results.txt

# 2. Tier 2 spec archives
git mv specs/07-OpenClaw-OS.md specs/archive/
git mv specs/11-WebSocket-Protocol.md specs/archive/
git mv specs/16-Phase6-Gateway-Hardening.md specs/archive/
git mv specs/22-Gateway-State-Resumption-Implementation.md specs/archive/
git mv specs/23-GAN-Audit-OS-Kernel.md specs/archive/
git mv specs/24-GAN-Audit-Sandbox-Containers.md specs/archive/

# 3. Tier 3 rename
git mv packages/openclaw-sdk packages/claw-sdk
# then update package.json reference

# 4. Tier 4 updates — manual edits to PRODUCTION.md, QUICKSTART.md, .env.example, specs/19, specs/18

# 5. Tier 5 — investigate each before deleting
```

---

## Success Criteria

- `git grep -r "openclaw-gateway"` returns 0 results
- `git grep -r "openclaw"` returns 0 results outside of `specs/archive/` and git history
- `cargo build --bin claw` still passes
- `bash test_e2e.sh` still passes
- Active spec list (specs/ root, excluding archive/) contains only specs 01–06, 08–10, 12–15, 17, 19–21, 25–29
- No `.opencode/agents/` directory generated by codegen
- `opencode/` submodule intact and unchanged
