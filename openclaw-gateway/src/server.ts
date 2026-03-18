import { createServer } from "node:http";
import { appendFile, mkdir } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { authorizeGatewayRequest, gatewayApiKeyFromEnv } from "./auth.ts";
import { CheckpointStore } from "./engine/checkpoints.ts";
import { loadCompiledDocument } from "./engine/documents.ts";
import { HumanInterventionRequiredError, SchemaDegradationError } from "./engine/errors.ts";
import { executeWorkflow } from "./engine/traversal.ts";
import { prePullSandboxImages } from "./engine/runtime.ts";
import { upgradeToWebSocket, sendJsonMessage, closeWebSocket, parseWebSocketFrame } from "./ws.ts";
import type { ExecutionRequest } from "./types.ts";

const gatewayRoot = dirname(fileURLToPath(import.meta.url));
const workspaceRoot = join(gatewayRoot, "..", "..");
const port = Number(process.env.OPENCLAW_GATEWAY_PORT ?? 8080);
const stateDir = join(workspaceRoot, "openclaw-gateway", ".openclaw");
const ingestorLogFile = join(stateDir, "workflow-ingestor.ndjson");
const gatewayApiKey = gatewayApiKeyFromEnv(process.env);
const checkpointStore = new CheckpointStore(
  {
    databasePath: join(stateDir, "engine.sqlite"),
    redisUrl: process.env.REDIS_URL ?? null
  }
);

const server = createServer(async (request, response) => {
  if (request.method === "GET" && request.url === "/health") {
    return writeJson(response, 200, {
      status: "ok",
      workspace_root: workspaceRoot,
      ingestor_log_file: ingestorLogFile
    });
  }

  if (request.method === "POST" && request.url === "/workflows/execute") {
    const authFailure = authorizeGatewayRequest(request, gatewayApiKey);
    if (authFailure) {
      return writeJson(response, authFailure.statusCode, authFailure.payload);
    }
    return handleWorkflowExecution(request, response);
  }

  const overrideMatch = request.method === "POST" && request.url?.match(/^\/sessions\/([^/]+)\/override$/);
  if (overrideMatch) {
    const authFailure = authorizeGatewayRequest(request, gatewayApiKey);
    if (authFailure) {
      return writeJson(response, authFailure.statusCode, authFailure.payload);
    }
    const sessionId = decodeURIComponent(overrideMatch[1]);
    const payload = JSON.parse(await readBody(request));
    await checkpointStore.saveHumanOverride(sessionId, payload);
    return writeJson(response, 202, {
      session_id: sessionId,
      status: "override_saved"
    });
  }

  return writeJson(response, 404, { status: "error", message: "Not found" });
});

// Pre-pull Docker images in the background at startup
void prePullSandboxImages();

server.on("upgrade", (request, socket, head) => {
  if (request.url !== "/workflows/stream") {
    socket.write("HTTP/1.1 404 Not Found\r\n\r\n");
    socket.destroy();
    return;
  }

  const ws = upgradeToWebSocket(request, socket, head, gatewayApiKey);
  if (!ws) {
    return;
  }

  let buffer = Buffer.alloc(0);
  ws.on("data", (chunk: Buffer) => {
    buffer = Buffer.concat([buffer, chunk]);

    const frame = parseWebSocketFrame(buffer);
    if (!frame) {
      // Incomplete frame — wait for more data
      return;
    }
    buffer = Buffer.alloc(0);

    // Close frame
    if (frame.opcode === 0x08) {
      closeWebSocket(ws);
      return;
    }

    // Ping → pong
    if (frame.opcode === 0x09) {
      const pong = Buffer.alloc(2);
      pong[0] = 0x8a; // FIN + pong
      pong[1] = 0;
      ws.write(pong);
      return;
    }

    // Text frame: expect JSON ExecutionRequest
    if (frame.opcode === 0x01) {
      handleWebSocketExecution(ws, frame.payload).catch((error) => {
        sendJsonMessage(ws, {
          type: "error",
          message: error instanceof Error ? error.message : "Unknown error"
        });
        closeWebSocket(ws, 1011, "Internal error");
      });
    }
  });

  ws.on("error", () => {
    ws.destroy();
  });
});

server.listen(port, "127.0.0.1", () => {
  console.log(`[openclaw-gateway] listening on http://127.0.0.1:${port}`);
});

server.on("close", () => {
  void checkpointStore.close();
});

async function handleWebSocketExecution(socket: import("node:net").Socket, rawPayload: string): Promise<void> {
  const raw = JSON.parse(rawPayload);
  const payload = validateExecutionRequest(raw);

  sendJsonMessage(socket, { type: "ack", session_id: payload.session_id });

  // Wrap checkpoint store to stream events over WebSocket
  const streamingCheckpoints = new CheckpointStore({
    databasePath: join(stateDir, "engine.sqlite"),
    redisUrl: process.env.REDIS_URL ?? null
  });
  const originalCheckpoint = streamingCheckpoints.checkpoint.bind(streamingCheckpoints);
  streamingCheckpoints.checkpoint = async (state, nodePath, eventType, eventPayload) => {
    await originalCheckpoint(state, nodePath, eventType, eventPayload);
    sendJsonMessage(socket, {
      type: "checkpoint",
      session_id: state.sessionId,
      node_path: nodePath,
      event_type: eventType,
      status: state.status
    });
  };

  try {
    const compiled = await loadCompiledDocument(payload.ast_hash, workspaceRoot);
    const result = await executeWorkflow({
      compiled,
      request: payload,
      checkpoints: streamingCheckpoints,
      workspaceRoot
    });

    sendJsonMessage(socket, {
      type: "result",
      session_id: payload.session_id,
      status: "success",
      result
    });
  } catch (error) {
    if (error instanceof HumanInterventionRequiredError) {
      sendJsonMessage(socket, {
        type: "human_intervention",
        session_id: error.event.session_id,
        event: error.event
      });
      return;
    }

    sendJsonMessage(socket, {
      type: "error",
      session_id: payload.session_id,
      message: error instanceof Error ? error.message : "Execution failed"
    });
  } finally {
    await streamingCheckpoints.close();
    closeWebSocket(socket, 1000, "completed");
  }
}

function validateExecutionRequest(raw: unknown): ExecutionRequest {
  if (typeof raw !== "object" || raw === null) {
    throw new ValidationError("Request body must be a JSON object");
  }

  const body = raw as Record<string, unknown>;

  if (typeof body.workflow !== "string" || body.workflow.length === 0) {
    throw new ValidationError("'workflow' must be a non-empty string");
  }
  if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(body.workflow)) {
    throw new ValidationError("'workflow' contains invalid characters");
  }
  if (typeof body.ast_hash !== "string" || body.ast_hash.length === 0) {
    throw new ValidationError("'ast_hash' must be a non-empty string");
  }
  if (typeof body.session_id !== "string" || body.session_id.length === 0) {
    throw new ValidationError("'session_id' must be a non-empty string");
  }
  if (typeof body.arguments !== "object" || body.arguments === null || Array.isArray(body.arguments)) {
    throw new ValidationError("'arguments' must be a JSON object");
  }

  return {
    workflow: body.workflow,
    ast_hash: body.ast_hash,
    session_id: body.session_id,
    arguments: body.arguments as Record<string, unknown>
  };
}

class ValidationError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "ValidationError";
  }
}

async function handleWorkflowExecution(
  request: import("node:http").IncomingMessage,
  response: import("node:http").ServerResponse
): Promise<void> {
  try {
    const raw = JSON.parse(await readBody(request));
    const payload = validateExecutionRequest(raw);
    await logWorkflowRequest(payload);
    console.log(
      `[Workflow Ingestor] workflow=${payload.workflow} ast_hash=${payload.ast_hash} session_id=${payload.session_id}`
    );

    const compiled = await loadCompiledDocument(payload.ast_hash, workspaceRoot);
    const result = await executeWorkflow({
      compiled,
      request: payload,
      checkpoints: checkpointStore,
      workspaceRoot
    });

    writeJson(response, 200, {
      session_id: payload.session_id,
      status: "success",
      result
    });
  } catch (error) {
    if (error instanceof HumanInterventionRequiredError) {
      return writeJson(response, 409, {
        session_id: error.event.session_id,
        status: "human_intervention_required",
        event: error.event
      });
    }

    if (error instanceof SchemaDegradationError) {
      return writeJson(response, 422, {
        status: "schema_degradation",
        message: error.message,
        payload: error.payload
      });
    }

    if (error instanceof ValidationError || error instanceof SyntaxError) {
      return writeJson(response, 400, {
        status: "validation_error",
        message: error.message
      });
    }

    console.error("[openclaw-gateway] execution failed", error);
    writeJson(response, 500, {
      status: "error",
      message: error instanceof Error ? error.message : "Unknown gateway failure"
    });
  }
}

async function logWorkflowRequest(payload: ExecutionRequest): Promise<void> {
  await mkdir(stateDir, { recursive: true });
  await appendFile(
    ingestorLogFile,
    `${JSON.stringify({ received_at: new Date().toISOString(), ...payload })}\n`
  );
}

const MAX_REQUEST_BODY_SIZE = 1_048_576;

function readBody(request: import("node:http").IncomingMessage): Promise<string> {
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

const SECURITY_HEADERS = {
  "content-type": "application/json",
  "x-content-type-options": "nosniff",
  "x-frame-options": "DENY"
};

function writeJson(
  response: import("node:http").ServerResponse,
  statusCode: number,
  payload: unknown
): void {
  response.writeHead(statusCode, SECURITY_HEADERS);
  response.end(JSON.stringify(payload));
}
