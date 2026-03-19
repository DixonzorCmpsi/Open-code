/**
 * Production WebSocket handler using the audited `ws` npm library.
 *
 * Selectable at runtime via CLAW_WS_PROVIDER=production (see server.ts).
 * Falls back to the hand-rolled RFC 6455 implementation when the env var
 * is unset or set to "builtin".
 *
 * Spec reference: specs/16-Phase-6B-Gateway-Hardening.md
 */

import { WebSocketServer, WebSocket } from "ws";
import type { Server, IncomingMessage } from "node:http";
import { authorizeGatewayRequest } from "./auth.ts";

interface ProductionWsOptions {
  gatewayApiKey: string | null;
  onMessage: (ws: WebSocket, payload: string, sessionId: string) => Promise<void>;
  isShuttingDown: () => boolean;
}

export function createProductionWebSocketHandler(
  server: Server,
  options: ProductionWsOptions
): void {
  const wss = new WebSocketServer({ noServer: true });

  server.on("upgrade", (request: IncomingMessage, socket, head) => {
    // Reject during shutdown
    if (options.isShuttingDown()) {
      socket.write("HTTP/1.1 503 Service Unavailable\r\n\r\n");
      socket.destroy();
      return;
    }

    // Only handle the streaming endpoint
    if (request.url !== "/workflows/stream") {
      socket.write("HTTP/1.1 404 Not Found\r\n\r\n");
      socket.destroy();
      return;
    }

    // Auth check using the project's existing authorizeGatewayRequest
    if (options.gatewayApiKey) {
      const authFailure = authorizeGatewayRequest(request, options.gatewayApiKey);
      if (authFailure) {
        socket.write(`HTTP/1.1 ${authFailure.statusCode} ${authFailure.payload.message}\r\n\r\n`);
        socket.destroy();
        return;
      }
    }

    wss.handleUpgrade(request, socket, head, (ws) => {
      wss.emit("connection", ws, request);
    });
  });

  wss.on("connection", (ws: WebSocket) => {
    ws.on("message", async (data) => {
      try {
        const payload = typeof data === "string" ? data : data.toString("utf8");
        const parsed = JSON.parse(payload);
        const sessionId = parsed.session_id ?? "unknown";
        await options.onMessage(ws, payload, sessionId);
      } catch (error) {
        ws.send(JSON.stringify({
          type: "error",
          message: error instanceof Error ? error.message : "Unknown error"
        }));
      }
    });

    ws.on("error", () => {
      // Silently handle connection errors — the close event will fire next
    });
  });

  // Graceful shutdown: close all open clients on SIGTERM
  process.once("SIGTERM", () => {
    wss.clients.forEach((client) => {
      if (client.readyState === WebSocket.OPEN) {
        client.close(1001, "Server shutting down");
      }
    });
    wss.close();
  });
}
