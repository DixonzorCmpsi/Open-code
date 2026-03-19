import test from "node:test";
import assert from "node:assert/strict";

import {
  parseWebSocketFrame,
  buildWebSocketFrame,
  validateHandshakeHeaders,
  computeAcceptKey
} from "./ws.ts";

test("computeAcceptKey produces correct RFC 6455 Sec-WebSocket-Accept value", () => {
  const key = "dGhlIHNhbXBsZSBub25jZQ==";
  const accept = computeAcceptKey(key);
  // SHA-1(key + "258EAFA5-E914-47DA-95CA-5AB5A7FEEA56") base64-encoded
  assert.equal(accept, "8r8BWQfvAtFBhy0OVa94E5hm4dA=");
});

test("validateHandshakeHeaders rejects missing upgrade header", () => {
  const result = validateHandshakeHeaders({});
  assert.ok(result !== null);
  assert.equal(result!.statusCode, 400);
});

test("validateHandshakeHeaders rejects missing sec-websocket-key", () => {
  const result = validateHandshakeHeaders({
    upgrade: "websocket",
    connection: "upgrade"
  });
  assert.ok(result !== null);
  assert.equal(result!.statusCode, 400);
});

test("validateHandshakeHeaders accepts valid upgrade headers", () => {
  const result = validateHandshakeHeaders({
    upgrade: "websocket",
    connection: "Upgrade",
    "sec-websocket-key": "dGhlIHNhbXBsZSBub25jZQ==",
    "sec-websocket-version": "13"
  });
  assert.equal(result, null);
});

test("validateHandshakeHeaders enforces api key when configured", () => {
  const result = validateHandshakeHeaders(
    {
      upgrade: "websocket",
      connection: "Upgrade",
      "sec-websocket-key": "dGhlIHNhbXBsZSBub25jZQ==",
      "sec-websocket-version": "13"
    },
    "prod_secret"
  );
  assert.ok(result !== null);
  assert.equal(result!.statusCode, 401);

  const validResult = validateHandshakeHeaders(
    {
      upgrade: "websocket",
      connection: "Upgrade",
      "sec-websocket-key": "dGhlIHNhbXBsZSBub25jZQ==",
      "sec-websocket-version": "13",
      "x-claw-key": "prod_secret"
    },
    "prod_secret"
  );
  assert.equal(validResult, null);
});

test("buildWebSocketFrame and parseWebSocketFrame roundtrip text messages", () => {
  const message = JSON.stringify({ type: "checkpoint", data: { status: "running" } });
  const frame = buildWebSocketFrame(message);
  assert.ok(Buffer.isBuffer(frame));

  const parsed = parseWebSocketFrame(frame);
  assert.ok(parsed !== null, "complete frame should parse successfully");
  assert.equal(parsed!.opcode, 0x01);
  assert.equal(parsed!.payload, message);
});

test("parseWebSocketFrame returns null on incomplete buffer (no crash)", () => {
  // Per specs/11-WebSocket-Protocol.md Section 3.1: bounds-check before accessing
  assert.equal(parseWebSocketFrame(Buffer.alloc(0)), null);
  assert.equal(parseWebSocketFrame(Buffer.alloc(1)), null);
  // 126-length frame needs at least 4 bytes of header
  const twoByteHeader = Buffer.from([0x81, 126]);
  assert.equal(parseWebSocketFrame(twoByteHeader), null);
  // 127-length frame needs at least 10 bytes of header
  const eightByteHeader = Buffer.from([0x81, 127, 0, 0, 0, 0, 0, 0]);
  assert.equal(parseWebSocketFrame(eightByteHeader), null);
});

test("buildWebSocketFrame handles payloads up to 64KB with 16-bit length encoding", () => {
  const largePayload = "x".repeat(300);
  const frame = buildWebSocketFrame(largePayload);
  const parsed = parseWebSocketFrame(frame);
  assert.ok(parsed !== null);
  assert.equal(parsed!.payload, largePayload);
});
