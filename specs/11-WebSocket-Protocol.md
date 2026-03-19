# Claw WebSocket Streaming Protocol

This document defines the bidirectional WebSocket protocol used by the Claw Gateway to stream workflow execution events to connected clients in real time.

---

## 1. Protocol Decision

| Phase | Implementation | Rationale |
|-------|---------------|-----------|
| Prototype / MVP | Hand-rolled RFC 6455 using Node.js built-ins | Zero external dependencies, <200 lines |
| Production (v1.0+) | MUST migrate to audited `ws` library | Proper fragmentation, extensions, backpressure, security audit trail |

**Note:** The hand-rolled implementation is acceptable for development and testing. Before any production deployment, the gateway MUST switch to the `ws` npm package (or equivalent audited library) to avoid edge cases in frame parsing, masking, and connection management.

**Production TLS architecture:** TLS termination is handled by a reverse proxy (nginx, Caddy, AWS ALB, Cloudflare, etc.). The gateway itself serves plain `ws://` and `http://` on a local port; the reverse proxy upgrades and forwards secure `wss://` traffic. Running the gateway directly on a public port without TLS termination is not supported.

---

## 2. Connection Endpoint

```
ws://host:port/workflows/stream
wss://host:port/workflows/stream  (production with TLS)
```

### 2.1 Handshake

Standard RFC 6455 handshake with Claw extensions:

**Required Headers (Client → Server):**
- `Upgrade: websocket`
- `Connection: Upgrade`
- `Sec-WebSocket-Key: <base64-encoded 16-byte nonce>`
- `Sec-WebSocket-Version: 13`
- `Sec-WebSocket-Protocol: claw.v1`

**Authentication (one of):**
- `x-claw-key: <api-key>`
- `Authorization: Bearer <api-key>`

Authentication follows the rules in `specs/12-Security-Model.md` Section 2.

**Response (Server → Client):**
- `HTTP/1.1 101 Switching Protocols`
- `Sec-WebSocket-Accept: <computed-accept-key>`
- `Sec-WebSocket-Protocol: claw.v1`

### 2.2 Connection States

```
CONNECTING → OPEN → EXECUTING → DRAINING → CLOSED
```

| State | Description |
|-------|-------------|
| CONNECTING | TCP established, handshake in progress |
| OPEN | Handshake complete, waiting for execute message |
| EXECUTING | Workflow running, checkpoint events streaming |
| DRAINING | Result/error sent, waiting for close acknowledgment |
| CLOSED | Connection terminated |

---

## 3. Frame Safety Requirements

### 3.1 Bounds Checking

**MUST:** The frame parser MUST validate buffer length before accessing any index. If the buffer is too small to contain the expected frame structure, the parser MUST return a "need more data" signal (not throw or crash).

```typescript
// CORRECT: Check bounds first
if (buffer.length < 2) return null; // Need more data
const opcode = buffer[0] & 0x0f;
let payloadLength = buffer[1] & 0x7f;

if (payloadLength === 126 && buffer.length < 4) return null;
if (payloadLength === 127 && buffer.length < 10) return null;
```

### 3.2 Maximum Frame Size

- Maximum payload size: 1 MB (1_048_576 bytes)
- Frames exceeding this limit: close connection with code 1009 (Message Too Big)

### 3.3 Close Frame Handling

**MUST:** Wait for `socket.write()` callback before calling `socket.end()` to prevent data loss.

```typescript
export function closeWebSocket(socket: Socket, code = 1000, reason = ""): void {
  const frame = buildCloseFrame(code, reason);
  socket.write(frame, () => {
    socket.end();
  });
}
```

---

## 4. Message Types (Server → Client)

All messages are JSON-encoded text frames (opcode 0x01).

### 4.1 `ack` — Execution Acknowledged

Sent immediately after receiving a valid execute message.

```json
{
  "type": "ack",
  "session_id": "req_a1b2c3d4-e5f6-7890-abcd-ef1234567890"
}
```

### 4.2 `checkpoint` — Execution Progress

Sent after every AST node execution is committed to the checkpoint store.

```json
{
  "type": "checkpoint",
  "session_id": "req_...",
  "node_path": "workflow:AnalyzeCompetitors/body/statements/0",
  "event_type": "let_decl",
  "status": "running"
}
```

### 4.3 `human_intervention` — Manual Action Required

Sent when the execution engine encounters a CAPTCHA or other human-blocking condition.

```json
{
  "type": "human_intervention",
  "session_id": "req_...",
  "event": {
    "type": "HumanInterventionRequired",
    "session_id": "req_...",
    "reason": "CAPTCHA detected in browser automation",
    "metadata": {
      "url": "https://example.com",
      "screenshot_url": "/sessions/req_.../screenshot"
    }
  }
}
```

The screenshot is retrieved over authenticated HTTP from `GET /sessions/{session_id}/screenshot`. Filesystem paths MUST NOT be exposed to clients because the gateway and SDK consumer may be running on different machines.

### 4.4 `result` — Execution Complete

Sent when the workflow finishes successfully. The server enters DRAINING state after this message.

```json
{
  "type": "result",
  "session_id": "req_...",
  "status": "success",
  "result": { ... }
}
```

### 4.5 `error` — Execution Failed

Sent on unrecoverable execution failure. The server enters DRAINING state.

```json
{
  "type": "error",
  "session_id": "req_...",
  "message": "Unknown variable company_name"
}
```

---

## 5. Message Types (Client → Server)

### 5.1 `execute` — Start Workflow

```json
{
  "workflow": "AnalyzeCompetitors",
  "arguments": { "company": "Apple" },
  "ast_hash": "b1b262a9c819...",
  "session_id": "req_a1b2c3d4-e5f6-7890-abcd-ef1234567890"
}
```

### 5.2 `cancel` — Cancel Execution (Future)

Reserved for future implementation. Not yet supported.

```json
{
  "type": "cancel",
  "session_id": "req_..."
}
```

---

## 6. Ordering & Delivery Guarantees

- Messages within a single session are strictly ordered (server sends them sequentially as the traversal engine processes AST nodes).
- Checkpoint events increase monotonically per `node_path`.
- There is NO at-least-once delivery guarantee. If the WebSocket connection drops, the client must reconnect and resume via the checkpoint store (REST `POST /workflows/execute` with the same `session_id`).

---

## 7. Reconnection

If a WebSocket connection drops mid-execution:

1. The workflow continues executing on the gateway (it is not tied to the connection).
2. The client reconnects and sends a new `execute` message with the same `session_id`.
3. If the session is still running, the gateway streams remaining checkpoint events.
4. If the session completed while disconnected, the gateway sends the `result` immediately.
5. Session state is durable in the checkpoint store (SQLite or Redis).

---

## 8. Ping/Pong Keepalive

- Server responds to client Ping frames (opcode 0x09) with Pong frames (opcode 0x0A).
- Server MAY send Ping frames every 30 seconds to detect dead connections.
- If no Pong is received within 10 seconds of a Ping, the server SHOULD close the connection.

---

## 9. Removed: X-Claw-Protocol Header

The previously proposed `X-Claw-Protocol` semver header has been **removed** from the protocol. Version negotiation is handled via the standard `Sec-WebSocket-Protocol: claw.v1` header during the handshake.

**Rationale:** Custom version headers on WebSocket connections add complexity without value. The standard subprotocol mechanism is sufficient and understood by all WebSocket libraries and proxies.
