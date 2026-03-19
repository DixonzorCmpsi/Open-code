# Phase 6B: Gateway Hardening

This spec covers remaining gateway features from specs 07, 11, 12, 13. Focus: graceful shutdown, visual stability, production WebSocket, rate limiting, structured logging.

**Prerequisite:** Read `specs/07-OpenClaw-OS.md`, `specs/11-WebSocket-Protocol.md`, `specs/12-Security-Model.md`, `specs/13-Visual-Intelligence.md`.

---

## 0. Goals & Non-Goals

### Goals (MUST do)
- Implement graceful SIGTERM/SIGINT shutdown with 30s drain and active session tracking (try/finally around every workflow execution)
- Fix the `Date.now()` security violation in nested workflow session IDs → `crypto.randomUUID()`
- Implement `waitForStability()` — Strategy A (networkidle + 1.5s delay) is the DEFAULT for all screenshots; Strategy B (raw pixel diff via `sharp`) is ONLY used when `locateByVision()` is called
- Create `ws-production.ts` using the `ws` library alongside the existing hand-rolled `ws.ts` — selectable via `CLAW_WS_PROVIDER` env var (default: `"builtin"`)
- Implement token bucket rate limiter with periodic cleanup (60s interval, 5min TTL, 10k max buckets) keyed by `request.socket.remoteAddress`
- Validate `Content-Type: application/json` on POST (split on `;`, trim, lowercase compare)
- Implement structured ndjson logger to replace all `console.log`/`console.error` calls

### Non-Goals (MUST NOT do)
- Do NOT implement request-body streaming or chunked transfer encoding
- Do NOT implement per-user rate limiting (Phase 7 — requires auth-aware key derivation)
- Do NOT trust `X-Forwarded-For` headers for rate limiting (proxy-aware rate limiting is Phase 7)
- Do NOT implement CORS preflight (`OPTIONS`) handling beyond basic headers (Phase 7 — needs configurable origins)
- Do NOT implement HTTP/2 or HTTP/3 support
- Do NOT implement TLS termination in the gateway (use a reverse proxy in production)
- Do NOT change the REST API contract (`/workflows/execute`, `/sessions/{id}/override`, `/health`) — only add headers and validation
- Do NOT make `sharp` a required dependency — it is optional; Strategy B falls back to Strategy A when unavailable

---

## 1. Graceful Shutdown (SIGTERM/SIGINT)

### Design

The gateway must track active workflow sessions and drain them on shutdown.

```typescript
const activeSessions = new Set<string>();
const DRAIN_TIMEOUT_MS = 30_000;

function setupGracefulShutdown(): void {
  let shuttingDown = false;

  const shutdown = (signal: string) => {
    if (shuttingDown) return;
    shuttingDown = true;
    log("info", "shutdown_started", { signal, active_sessions: activeSessions.size });

    // 1. Stop accepting new connections
    server.close(() => {
      log("info", "server_closed", { signal });
    });

    // 2. If no active sessions, exit immediately
    if (activeSessions.size === 0) {
      void checkpointStore.close().then(() => { process.exitCode = 0; });
      return;
    }

    // 3. Wait for active sessions to complete (with timeout)
    const drainTimer = setTimeout(async () => {
      log("warn", "drain_timeout", { remaining: activeSessions.size });
      // Checkpoint remaining sessions as interrupted
      // (the checkpoint store records their current state)
      await checkpointStore.close();
      process.exitCode = 0;
    }, DRAIN_TIMEOUT_MS);

    // 4. Check periodically if all sessions finished
    const checkDrain = setInterval(async () => {
      if (activeSessions.size === 0) {
        clearInterval(checkDrain);
        clearTimeout(drainTimer);
        await checkpointStore.close();
        log("info", "shutdown_complete", { drained: true });
        process.exitCode = 0;
      }
    }, 500);
  };

  process.on("SIGTERM", () => shutdown("SIGTERM"));
  process.on("SIGINT", () => shutdown("SIGINT"));
}
```

### Fix: Nested Workflow Session ID (Security Violation)

The existing `traversal.ts:266` uses `Date.now()` for nested workflow session IDs:
```typescript
session_id: `${state.sessionId}:${workflowName}:${Date.now()}`
```
This violates `specs/12-Security-Model.md` Section 4. Replace with:
```typescript
session_id: `${state.sessionId}:${workflowName}:${crypto.randomUUID()}`
```
Add `import { randomUUID } from "node:crypto"` at the top of `traversal.ts`.

### Session Tracking Integration

In `handleWorkflowExecution` and `handleWebSocketExecution`:
```typescript
activeSessions.add(payload.session_id);
try {
  // ... execute workflow ...
} finally {
  activeSessions.delete(payload.session_id);
}
```

### TDD Tests

1. **`test_graceful_shutdown_exits_when_no_active_sessions`** — Call shutdown with empty activeSessions. Verify checkpointStore.close() is called and process exits 0.
2. **`test_graceful_shutdown_waits_for_active_sessions`** — Add a session to activeSessions, call shutdown, verify it waits. Remove the session, verify exit occurs.
3. **`test_graceful_shutdown_force_exits_after_timeout`** — Add a session that never completes. Verify process exits after DRAIN_TIMEOUT_MS.

---

## 2. Visual Stability Wait

### Design Decision

**Do NOT hand-roll pixel comparison on PNG buffers.** PNGs are compressed — byte-level comparison is meaningless.

Instead, use one of two strategies:

### Strategy A: Playwright-Native Stability (Recommended)

Use Playwright's built-in `page.waitForLoadState("networkidle")` combined with a configurable delay:

```typescript
const VISUAL_STABILITY_DELAY_MS = 1_500;

async function waitForStability(page: BrowserPage): Promise<void> {
  try {
    // Wait for network to idle (no requests for 500ms)
    await page.waitForLoadState?.("networkidle");
  } catch {
    // Fallback: some pages never reach networkidle
  }
  // Additional delay for CSS animations / JS rendering
  await new Promise((resolve) => setTimeout(resolve, VISUAL_STABILITY_DELAY_MS));
}
```

**Rationale:** Playwright's `networkidle` is the industry standard for waiting until a page is "done loading." The additional 1.5s delay catches CSS transitions and JS-driven animations. This is how Percy.io, Chromatic, and other visual testing tools approach stability.

### Strategy B: Frame Comparison (For Vision-Critical Paths Only)

When `waitForStability` is used before a vision LLM call (not just logging screenshots), use Playwright's `screenshot()` with `type: "png"` and compare raw pixel data via `sharp`:

```typescript
import sharp from "sharp";

const STABILITY_FRAMES = 3;
const STABILITY_THRESHOLD = 0.001; // 0.1% of pixels changed
const STABILITY_TIMEOUT_MS = 4_000;

async function waitForVisualStability(page: BrowserPage): Promise<void> {
  const deadline = Date.now() + STABILITY_TIMEOUT_MS;
  let stableCount = 0;
  let lastPixels: Buffer | null = null;

  while (stableCount < STABILITY_FRAMES && Date.now() < deadline) {
    // fullPage: false for stability checks (viewport-only is faster and sufficient)
    // Note: Spec 13 uses fullPage: true for AUDIT screenshots; this is for stability ONLY
    const screenshot = await page.screenshot({ fullPage: false, type: "png" });
    const currentPixels = await sharp(screenshot).raw().toBuffer();

    if (lastPixels && lastPixels.length === currentPixels.length) {
      const delta = pixelDelta(lastPixels, currentPixels);
      if (delta < STABILITY_THRESHOLD) {
        stableCount++;
      } else {
        stableCount = 0;
      }
    }

    lastPixels = currentPixels;
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
}

function pixelDelta(a: Buffer, b: Buffer): number {
  // a and b are RAW pixel buffers (RGBA), NOT compressed PNGs
  let diffPixels = 0;
  const threshold = 10; // Allow 10-level per-channel variance
  for (let i = 0; i < a.length; i += 4) {
    if (Math.abs(a[i] - b[i]) > threshold ||       // R
        Math.abs(a[i + 1] - b[i + 1]) > threshold || // G
        Math.abs(a[i + 2] - b[i + 2]) > threshold || // B
        Math.abs(a[i + 3] - b[i + 3]) > threshold) { // A (alpha — catches opacity animations)
      diffPixels++;
    }
  }
  return diffPixels / (a.length / 4);
}
```

**Key difference from the previous spec:** The `pixelDelta` function operates on **raw RGBA pixel buffers** (decoded via `sharp`), NOT on compressed PNG buffers. Comparing compressed PNGs byte-by-byte is meaningless.

### Integration

- **`search()` and `navigate()`:** Use Strategy A (networkidle + delay). Fast and reliable.
- **`locateByVision()`:** Use Strategy B (frame comparison). More precise, required before sending to vision LLM.
- **Fallback (no Playwright):** `waitForStability` is a no-op. Return immediately.

### TDD Tests

1. **`test_waitForStability_resolves_after_networkidle`** — Mock page with `waitForLoadState` that resolves. Verify function completes in < 2s.
2. **`test_waitForStability_fallback_on_timeout`** — Mock page where `waitForLoadState` throws. Verify function resolves after the delay fallback.
3. **`test_pixelDelta_detects_change_in_raw_buffers`** — Create two raw RGBA buffers that differ in 10% of pixels. Assert `pixelDelta > 0.05`.
4. **`test_pixelDelta_identical_buffers_return_zero`** — Identical raw buffers → `pixelDelta === 0`.

### Dependencies

- `sharp` is an optional dependency (installed via `npm install sharp`). If unavailable, Strategy B falls back to Strategy A.
- `sharp` is NOT required for basic gateway operation — only for vision-critical screenshot analysis.

---

## 3. Production WebSocket (ws Library)

### Migration Architecture

Create `ws-production.ts` alongside the existing `ws.ts`:

```typescript
// ws-production.ts — drop-in replacement using the audited ws library
import { WebSocketServer } from "ws";

export function createProductionWebSocketHandler(
  server: import("node:http").Server,
  options: { gatewayApiKey: string | null }
) {
  const wss = new WebSocketServer({ noServer: true });

  server.on("upgrade", (request, socket, head) => {
    // Auth check
    const authFailure = authorizeGatewayRequest(request, options.gatewayApiKey);
    if (authFailure) {
      socket.write(`HTTP/1.1 ${authFailure.statusCode} ${authFailure.payload.message}\r\n\r\n`);
      socket.destroy();
      return;
    }

    wss.handleUpgrade(request, socket, head, (ws) => {
      wss.emit("connection", ws, request);
    });
  });

  wss.on("connection", (ws) => {
    ws.on("message", (data) => {
      const payload = JSON.parse(data.toString());
      handleWebSocketExecution(ws, payload).catch((error) => {
        ws.send(JSON.stringify({ type: "error", message: error.message }));
        ws.close(1011, "Internal error");
      });
    });
  });
}
```

### Provider Selection

```typescript
const WS_PROVIDER = process.env.CLAW_WS_PROVIDER ?? "builtin";

if (WS_PROVIDER === "production") {
  const { createProductionWebSocketHandler } = await import("./ws-production.ts");
  createProductionWebSocketHandler(server, { gatewayApiKey });
} else {
  // Use existing hand-rolled ws.ts handler
  server.on("upgrade", ...);
}
```

### TDD Tests

All existing WebSocket tests must pass with both providers. Add:
1. **`test_production_ws_handles_fragmented_frames`** — Send a large message that the `ws` library fragments. Verify reassembly.
2. **`test_production_ws_rejects_oversized_frames`** — Send >1MB frame. Verify connection closes with code 1009.

### Migration Timeline

- Phase 6: Implement `ws-production.ts`, keep `ws.ts` as default
- v1.0: Switch default to `ws-production.ts`
- v1.1: Remove hand-rolled `ws.ts`

---

## 4. Rate Limiting (Token Bucket with LRU Eviction)

### Design

```typescript
interface RateLimiter {
  check(key: string): boolean;
}

const DEFAULT_MAX_PER_SECOND = 100;
const MAX_BUCKETS = 10_000;
const BUCKET_TTL_MS = 5 * 60 * 1000; // 5 minutes

function createRateLimiter(maxPerSecond = DEFAULT_MAX_PER_SECOND): RateLimiter {
  const buckets = new Map<string, { tokens: number; lastRefill: number }>();

  // Periodic cleanup to prevent unbounded memory growth
  const cleanupInterval = setInterval(() => {
    const now = Date.now();
    for (const [key, bucket] of buckets) {
      if (now - bucket.lastRefill > BUCKET_TTL_MS) {
        buckets.delete(key);
      }
    }
    // Hard cap: evict oldest entries if over limit
    if (buckets.size > MAX_BUCKETS) {
      const entries = [...buckets.entries()].sort((a, b) => a[1].lastRefill - b[1].lastRefill);
      for (let i = 0; i < entries.length - MAX_BUCKETS; i++) {
        buckets.delete(entries[i][0]);
      }
    }
  }, 60_000);
  cleanupInterval.unref(); // Don't prevent process exit

  return {
    check(key: string): boolean {
      const now = Date.now();
      let bucket = buckets.get(key);
      if (!bucket) {
        bucket = { tokens: maxPerSecond, lastRefill: now };
        buckets.set(key, bucket);
      }
      const elapsed = (now - bucket.lastRefill) / 1000;
      bucket.tokens = Math.min(maxPerSecond, bucket.tokens + elapsed * maxPerSecond);
      bucket.lastRefill = now;
      if (bucket.tokens < 1) return false;
      bucket.tokens -= 1;
      return true;
    }
  };
}
```

### Integration

```typescript
const rateLimiter = createRateLimiter(
  Number(process.env.CLAW_RATE_LIMIT ?? 100)
);

// In HTTP handler:
const clientKey = request.socket.remoteAddress ?? "unknown";
if (!rateLimiter.check(clientKey)) {
  return writeJson(response, 429, {
    status: "rate_limited",
    message: "Too many requests. Max 100 per second."
  });
}
```

### TDD Tests

1. **`test_rate_limiter_allows_under_limit`** — Check 50 times in a row → all return true.
2. **`test_rate_limiter_blocks_over_limit`** — Check 150 times in 0ms → some return false.
3. **`test_rate_limiter_refills_over_time`** — Exhaust tokens, wait 1 second, check again → tokens refilled.
4. **`test_rate_limiter_cleanup_removes_stale_entries`** — Insert 5 entries, advance time by 6 minutes, trigger cleanup, verify entries removed.

---

## 5. Content-Type Validation

### Implementation

```typescript
if (request.method === "POST") {
  const contentType = request.headers["content-type"]?.split(";")[0].trim().toLowerCase();
  if (contentType !== "application/json") {
    return writeJson(response, 415, {
      status: "error",
      message: "Content-Type must be application/json"
    });
  }
}
```

**Note:** Split on `;` to handle `application/json; charset=utf-8` correctly. Trim and lowercase for robustness.

### TDD Test

1. **`test_rejects_non_json_content_type`** — Send POST with `Content-Type: text/plain` → HTTP 415.
2. **`test_accepts_json_with_charset`** — Send POST with `Content-Type: application/json; charset=utf-8` → accepted.

---

## 6. Structured Logging (ndjson)

### Logger Module (`openclaw-gateway/src/logger.ts`)

```typescript
type LogLevel = "error" | "warn" | "info" | "debug";

const PRIORITY: Record<LogLevel, number> = { error: 0, warn: 1, info: 2, debug: 3 };
const configuredLevel: LogLevel = (process.env.CLAW_LOG_LEVEL as LogLevel) ?? "info";
const jsonFormat = process.env.CLAW_LOG_FORMAT === "json";

export function log(level: LogLevel, event: string, data?: Record<string, unknown>): void {
  if (PRIORITY[level] > PRIORITY[configuredLevel]) return;

  if (jsonFormat) {
    const entry = { timestamp: new Date().toISOString(), level, event, ...data };
    process.stderr.write(JSON.stringify(entry) + "\n");
  } else {
    const prefix = `[claw-gateway]`;
    const message = data ? `${event} ${JSON.stringify(data)}` : event;
    if (level === "error") {
      console.error(`${prefix} ERROR: ${message}`);
    } else if (level === "warn") {
      console.error(`${prefix} WARN: ${message}`);
    } else {
      console.log(`${prefix} ${message}`);
    }
  }
}
```

### Log Events

| Event | Level | Fields |
|-------|-------|--------|
| `server_started` | info | `port` |
| `workflow_received` | info | `session_id`, `workflow`, `ast_hash` |
| `workflow_completed` | info | `session_id`, `duration_ms` |
| `workflow_failed` | error | `session_id`, `error` |
| `human_intervention` | warn | `session_id`, `reason` |
| `checkpoint` | debug | `session_id`, `node_path`, `event_type` |
| `shutdown_started` | info | `signal`, `active_sessions` |
| `shutdown_complete` | info | `drained` |
| `rate_limited` | warn | `client_key` |

### TDD Tests

1. **`test_logger_json_format`** — Set `CLAW_LOG_FORMAT=json`, call `log("info", "test", { x: 1 })`, capture stderr, assert valid JSON with correct fields.
2. **`test_logger_respects_log_level`** — Set `CLAW_LOG_LEVEL=warn`, call `log("debug", ...)`, assert nothing written.
