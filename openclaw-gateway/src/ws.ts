/**
 * Lightweight RFC 6455 WebSocket implementation for the Claw Gateway.
 *
 * Uses only Node.js built-in modules — no external WS library needed.
 * Supports text frames (opcode 0x01) for JSON streaming of checkpoint events.
 */

import { createHash } from "node:crypto";
import type { IncomingMessage, ServerResponse } from "node:http";
import type { Socket } from "node:net";

import { authorizeGatewayRequest } from "./auth.ts";

const WS_MAGIC_GUID = "258EAFA5-E914-47DA-95CA-5AB5A7FEEA56";

interface HandshakeFailure {
  statusCode: number;
  message: string;
}

interface ParsedFrame {
  opcode: number;
  payload: string;
}

export function computeAcceptKey(clientKey: string): string {
  return createHash("sha1")
    .update(clientKey + WS_MAGIC_GUID)
    .digest("base64");
}

export function validateHandshakeHeaders(
  headers: Record<string, string | string[] | undefined>,
  expectedApiKey?: string | null
): HandshakeFailure | null {
  const upgrade = normalizeHeader(headers, "upgrade");
  if (!upgrade || upgrade.toLowerCase() !== "websocket") {
    return { statusCode: 400, message: "Missing or invalid Upgrade header" };
  }

  const connection = normalizeHeader(headers, "connection");
  if (!connection || !connection.toLowerCase().includes("upgrade")) {
    return { statusCode: 400, message: "Missing or invalid Connection header" };
  }

  const key = normalizeHeader(headers, "sec-websocket-key");
  if (!key) {
    return { statusCode: 400, message: "Missing Sec-WebSocket-Key header" };
  }

  if (expectedApiKey) {
    const authFailure = authorizeGatewayRequest({ headers }, expectedApiKey);
    if (authFailure) {
      return { statusCode: authFailure.statusCode, message: authFailure.payload.message };
    }
  }

  return null;
}

/**
 * Upgrades an HTTP request to a WebSocket connection. Returns the raw socket
 * for bidirectional streaming, or null if the handshake fails.
 */
export function upgradeToWebSocket(
  request: IncomingMessage,
  socket: Socket,
  head: Buffer,
  expectedApiKey?: string | null
): Socket | null {
  const failure = validateHandshakeHeaders(
    request.headers as Record<string, string | string[] | undefined>,
    expectedApiKey
  );

  if (failure) {
    socket.write(
      `HTTP/1.1 ${failure.statusCode} ${failure.message}\r\n` +
      "Content-Type: text/plain\r\n" +
      `Content-Length: ${Buffer.byteLength(failure.message)}\r\n` +
      "\r\n" +
      failure.message
    );
    socket.destroy();
    return null;
  }

  const clientKey = request.headers["sec-websocket-key"] as string;
  const acceptKey = computeAcceptKey(clientKey);

  socket.write(
    "HTTP/1.1 101 Switching Protocols\r\n" +
    "Upgrade: websocket\r\n" +
    "Connection: Upgrade\r\n" +
    `Sec-WebSocket-Accept: ${acceptKey}\r\n` +
    "\r\n"
  );

  return socket;
}

/**
 * Builds a WebSocket text frame (opcode 0x01, unmasked, FIN=1).
 * Suitable for server-to-client messages (server frames are never masked).
 */
export function buildWebSocketFrame(payload: string): Buffer {
  const data = Buffer.from(payload, "utf-8");
  const length = data.length;

  let header: Buffer;
  if (length < 126) {
    header = Buffer.alloc(2);
    header[0] = 0x81; // FIN + text opcode
    header[1] = length;
  } else if (length < 65536) {
    header = Buffer.alloc(4);
    header[0] = 0x81;
    header[1] = 126;
    header.writeUInt16BE(length, 2);
  } else {
    header = Buffer.alloc(10);
    header[0] = 0x81;
    header[1] = 127;
    header.writeBigUInt64BE(BigInt(length), 2);
  }

  return Buffer.concat([header, data]);
}

/**
 * Parses a WebSocket frame (unmasked or masked).
 * Returns null if the buffer is incomplete (need more data).
 *
 * Per specs/11-WebSocket-Protocol.md Section 3.1: MUST bounds-check
 * buffer length before accessing any index.
 */
export function parseWebSocketFrame(buffer: Buffer): ParsedFrame | null {
  if (buffer.length < 2) {
    return null;
  }

  const opcode = buffer[0] & 0x0f;
  const masked = (buffer[1] & 0x80) !== 0;
  let payloadLength = buffer[1] & 0x7f;
  let offset = 2;

  if (payloadLength === 126) {
    if (buffer.length < 4) return null;
    payloadLength = buffer.readUInt16BE(2);
    offset = 4;
  } else if (payloadLength === 127) {
    if (buffer.length < 10) return null;
    payloadLength = Number(buffer.readBigUInt64BE(2));
    offset = 10;
  }

  if (masked) {
    if (buffer.length < offset + 4 + payloadLength) return null;
    const mask = buffer.subarray(offset, offset + 4);
    offset += 4;
    const payloadData = Buffer.alloc(payloadLength);
    for (let i = 0; i < payloadLength; i++) {
      payloadData[i] = buffer[offset + i] ^ mask[i % 4];
    }
    return { opcode, payload: payloadData.toString("utf-8") };
  }

  if (buffer.length < offset + payloadLength) return null;
  const payloadData = buffer.subarray(offset, offset + payloadLength);
  return { opcode, payload: payloadData.toString("utf-8") };
}

/**
 * Sends a JSON message over a WebSocket connection.
 */
export function sendJsonMessage(socket: Socket, data: unknown): void {
  const frame = buildWebSocketFrame(JSON.stringify(data));
  socket.write(frame);
}

/**
 * Sends a close frame and ends the socket after the write completes.
 *
 * Per specs/11-WebSocket-Protocol.md Section 3.3: wait for write callback
 * before calling socket.end() to prevent data loss.
 */
export function closeWebSocket(socket: Socket, code = 1000, reason = ""): void {
  const reasonBuffer = Buffer.from(reason, "utf-8");
  const payload = Buffer.alloc(2 + reasonBuffer.length);
  payload.writeUInt16BE(code, 0);
  reasonBuffer.copy(payload, 2);

  const header = Buffer.alloc(2);
  header[0] = 0x88; // FIN + close opcode
  header[1] = payload.length;

  socket.write(Buffer.concat([header, payload]), () => {
    socket.end();
  });
}

function normalizeHeader(
  headers: Record<string, string | string[] | undefined>,
  name: string
): string | null {
  for (const [key, value] of Object.entries(headers)) {
    if (key.toLowerCase() !== name.toLowerCase()) {
      continue;
    }
    if (Array.isArray(value)) {
      return value[0] ?? null;
    }
    return value ?? null;
  }
  return null;
}
