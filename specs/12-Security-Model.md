# OpenClaw Security Model

This document defines the security invariants that apply to every layer of the OpenClaw toolchain: the `clawc` compiler, the Gateway OS, the generated SDKs, and the client libraries. All other specs reference this document for security requirements.

---

## 1. Threat Model

OpenClaw has three trust boundaries:

| Boundary | Untrusted Input | Component |
|----------|----------------|-----------|
| **Compiler** | Raw `.claw` source text | `clawc` (Rust) |
| **Gateway** | HTTP/WebSocket requests, LLM responses | `openclaw-gateway` (TypeScript) |
| **Sandbox** | Custom tool execution output, file system access | Docker containers, local subprocesses |

**Assumption:** The `.claw` source file is developer-authored but may contain adversarial numeric literals, deeply nested structures, or pathological grammar constructs. The compiler MUST NOT crash on any input.

**Assumption:** Gateway HTTP/WebSocket endpoints are internet-facing. All request data is untrusted.

**Assumption:** Custom tools (`python(...)`, `typescript(...)`) execute arbitrary user code. The sandbox MUST prevent host access.

---

## 2. Authentication & Authorization

### 2.1 API Key Comparison

**MUST:** Use constant-time comparison for all secret validation.

```typescript
import { timingSafeEqual } from "node:crypto";

function compareApiKeys(provided: string, expected: string): boolean {
  const a = Buffer.from(provided);
  const b = Buffer.from(expected);
  if (a.length !== b.length) return false;
  return timingSafeEqual(a, b);
}
```

**NEVER:** Use `===` or `!==` for API key comparison. String equality is vulnerable to timing attacks where an attacker measures response time differences to extract the key character-by-character.

### 2.2 Key Sources

- The gateway API key environment variable name is driven by `gateway.api_key_env` from `claw.json`. The default name is `CLAW_GATEWAY_API_KEY`.
- `GATEWAY_AUTH_KEY` is a deprecated fallback for backward compatibility only.
- Precedence when both are set: the variable named by `gateway.api_key_env` wins over `GATEWAY_AUTH_KEY`.
- Keys MUST NOT be logged, committed, or included in error messages.
- Both `x-claw-key` header and `Authorization: Bearer <key>` are accepted.

### 2.3 Auth Bypass

- When no API key environment variable is set, authentication is disabled (local development mode).
- The `/health` endpoint is always unauthenticated.

---

## 3. Request Hardening

### 3.1 Request Body Size Limit

**MUST:** Enforce `MAX_REQUEST_BODY_SIZE = 1_048_576` bytes (1 MB) on all HTTP POST endpoints.

```typescript
const MAX_REQUEST_BODY_SIZE = 1_048_576;

function readBody(request: IncomingMessage): Promise<string> {
  return new Promise((resolve, reject) => {
    let body = "";
    let size = 0;
    request.setEncoding("utf8");
    request.on("data", (chunk) => {
      size += Buffer.byteLength(chunk);
      if (size > MAX_REQUEST_BODY_SIZE) {
        request.destroy();
        reject(new Error("Request body exceeds 1MB limit"));
        return;
      }
      body += chunk;
    });
    request.on("end", () => resolve(body));
    request.on("error", reject);
  });
}
```

**Rationale:** Without a size limit, an attacker can send a multi-gigabyte POST and exhaust gateway memory.

### 3.2 HTTP Security Headers

All HTTP responses MUST include:

```
X-Content-Type-Options: nosniff
X-Frame-Options: DENY
Content-Type: application/json
```

Production deployments SHOULD additionally include:
```
Strict-Transport-Security: max-age=31536000; includeSubDomains
Access-Control-Allow-Origin: <configured-origin from gateway.cors_origin>
Access-Control-Allow-Methods: GET, POST, OPTIONS
Access-Control-Allow-Headers: Content-Type, x-claw-key, Authorization
```

### 3.3 JSON Parsing Safety

- Parse JSON inside a try/catch. Return HTTP 400 with `status: "validation_error"` on malformed JSON.
- Validate all required fields before processing. Never pass raw `JSON.parse()` output directly into the execution engine.

---

## 4. Session ID Generation

**MUST:** Use `crypto.randomUUID()` (Node.js) or `uuid.uuid4()` (Python) for all session identifiers.

**NEVER:** Use `Date.now()`, `Math.random()`, or any timestamp-based ID generation.

**Rationale:** `Date.now()` produces predictable millisecond-precision IDs. An attacker can guess session IDs and resume or override someone else's workflow via `POST /sessions/{id}/override`.

```typescript
// Correct
const sessionId = resumeSessionId ?? `req_${crypto.randomUUID()}`;

// WRONG - predictable
const sessionId = resumeSessionId ?? `req_${Date.now()}`;
```

```python
# Correct
import uuid
session_id = resume_session_id or f"req_{uuid.uuid4()}"

# WRONG - predictable
session_id = resume_session_id or f"req_{int(time.time() * 1000)}"
```

---

## 5. Path Traversal Prevention

### 5.1 Symlink Resolution

**MUST:** Resolve all tool file paths with `fs.realpath()` before use, then verify the resolved path remains within the workspace root.

```typescript
import { realpath } from "node:fs/promises";

async function resolveToolPath(target: string, workspaceRoot: string): Promise<string> {
  const candidate = path.resolve(workspaceRoot, target);
  const real = await realpath(candidate);
  const relative = path.relative(workspaceRoot, real);
  if (relative.startsWith("..") || path.isAbsolute(relative)) {
    throw new Error(`Tool target resolves outside workspace: ${target}`);
  }
  return real;
}
```

**Rationale:** Without `realpath()`, a symlink inside the workspace can point to `/etc/passwd` or other sensitive files. The `path.relative()` check alone is insufficient because it operates on the symlink path, not the real target.

**Platform note:** On Windows, modern Node.js `fs.realpath()` resolves directory junctions as well as symlinks. This repository requires Node.js `22.6+`, which is sufficient for consistent junction handling. UNC paths (`\\\\server\\share`) are absolute and MUST fail the containment check.

### 5.2 Docker Mount Safety

- Workspace is mounted read-only: `-v ${workspaceRoot}:/workspace:ro`
- Sandbox temp directory is the only writable mount
- No host network access: `--network=none`

On Windows, Docker bind mounts depend on Docker Desktop's WSL2 integration. Workspaces on non-`C:` drives or UNC network paths are not guaranteed to mount correctly. For reliable Docker sandboxing on Windows, running the gateway inside WSL2 is strongly recommended.

---

## 6. Sandbox Security

### 6.1 Docker Container Flags (MANDATORY)

Every Docker sandbox execution MUST include:

| Flag | Purpose |
|------|---------|
| `--rm` | Remove container after exit |
| `--network=none` | No network access |
| `--read-only` | Read-only root filesystem |
| `--cap-drop=ALL` | Drop all Linux capabilities |
| `--security-opt=no-new-privileges` | Prevent privilege escalation |
| `--pids-limit=64` | Limit process count (prevent fork bombs) |
| `--memory=256m` | Memory limit |
| `--cpus=1` | CPU limit |
| `--user=65532:65532` | Run as unprivileged user |

### 6.2 Timeout Enforcement

- Default timeout: `DEFAULT_SANDBOX_TIMEOUT_MS = 30_000` (30 seconds)
- On timeout: send `SIGKILL` to child process, clean up temp directories
- Timeout is configurable per-tool via execution options

### 6.3 Exit Code Mapping

| Exit Code | Meaning | OpenClaw Error |
|-----------|---------|----------------|
| 0 | Success | — |
| 1 | General error | `ToolExecutionError` |
| 137 | OOM Kill (SIGKILL) | `SandboxOOMError` |
| 139 | Segmentation fault | `SandboxCrashError` |
| 143 | SIGTERM | `SandboxTimeoutError` |

### 6.4 Local Sandbox Mode

When `sandbox_backend` is `"local"`, custom tools execute as direct child subprocesses of the gateway with the same filesystem and network permissions as the gateway process. This mode provides **no isolation**, **no filesystem sandbox**, and **no network sandbox**.

Local mode is acceptable only for local development where every tool is fully trusted. It MUST NOT be used in production, staging, CI environments that run untrusted code, or any deployment that accepts user-provided tools.

The gateway MUST print a visible startup warning when running in local mode:

```text
[WARN] Sandbox backend is 'local' - custom tools run without isolation. Do not use in production.
```

---

## 7. Compiler Security

### 7.1 No Panics on User Input

**MUST:** The `clawc` parser MUST NOT use `.expect()` or `.unwrap()` on any code path reachable from user-provided `.claw` source text. Use `Result<T, CompilerError>` propagation exclusively.

**Exception:** `.expect()` is permitted ONLY with a `// SAFETY:` comment that mathematically proves the branch is unreachable (e.g., after a regex match that guarantees parse success).

### 7.2 Numeric Literal Safety

Integer and float parsing MUST handle overflow gracefully:
- Integers exceeding `i64::MAX` → `CompilerError::ParseError` with span
- Floats parsing to infinity or NaN → `CompilerError::ParseError` with span

### 7.3 Recursion Depth

- Parser recursion depth MUST be bounded (default: 256 nesting levels)
- Deeply nested `.claw` expressions beyond the limit → `CompilerError::ParseError("Maximum nesting depth exceeded")`
