import { createServer } from "node:http";
import { existsSync } from "node:fs";
import { appendFile, mkdir, readFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";

import { authorizeGatewayRequest, gatewayApiKeyFromEnv } from "./auth.ts";
import { CheckpointStore } from "./engine/checkpoints.ts";
import { loadCompiledDocument } from "./engine/documents.ts";
import { HumanInterventionRequiredError, SchemaDegradationError } from "./engine/errors.ts";
import { executeWorkflow } from "./engine/traversal.ts";
import { prePullSandboxImages } from "./engine/runtime.ts";
import { log } from "./logger.ts";
import { createRateLimiter } from "./rate-limiter.ts";
import { upgradeToWebSocket, sendJsonMessage, closeWebSocket, parseWebSocketFrame } from "./ws.ts";
import type { ExecutionRequest } from "./types.ts";

const projectRoot = resolveProjectRoot(process.env.CLAW_PROJECT_ROOT ?? process.cwd());
const workspaceRoot = projectRoot;
const port = Number(process.env.CLAW_GATEWAY_PORT ?? 8080);
const stateDir = process.env.CLAW_STATE_DIR ?? join(projectRoot, ".claw");
const screenshotRoot = process.env.CLAW_SCREENSHOT_DIR ?? join(stateDir, "screenshots");
const ingestorLogFile = join(stateDir, "workflow-ingestor.ndjson");
const corsOrigin = process.env.CLAW_GATEWAY_CORS_ORIGIN ?? null;
const gatewayApiKey = gatewayApiKeyFromEnv(process.env);
const configuredRateLimit = Number(process.env.CLAW_RATE_LIMIT ?? 100);
const rateLimitPerSecond =
  Number.isFinite(configuredRateLimit) && configuredRateLimit > 0 ? configuredRateLimit : 100;
let activeExecutions = 0;
let shuttingDown = false;
let shutdownPromise: Promise<void> | null = null;
let checkpointStoreClosed = false;
const openSockets = new Set<import("node:net").Socket>();
const rateLimiter = createRateLimiter(rateLimitPerSecond);

process.env.CLAW_STATE_DIR ??= stateDir;
process.env.CLAW_SCREENSHOT_DIR ??= screenshotRoot;

const checkpointStore = new CheckpointStore(
  {
    databasePath: join(stateDir, "engine.sqlite"),
    redisUrl: process.env.REDIS_URL ?? null
  }
);

const server = createServer(async (request, response) => {
  if (request.method === "OPTIONS") {
    return writeEmpty(response, 204);
  }

  if (request.method === "GET" && request.url === "/health") {
    return writeJson(response, 200, {
      status: "ok",
      workspace_root: workspaceRoot,
      state_dir: stateDir,
      ingestor_log_file: ingestorLogFile
    });
  }

  const clientKey = request.socket.remoteAddress ?? "unknown";
  if (!rateLimiter.check(clientKey)) {
    log("warn", "rate_limited", { client_key: clientKey });
    return writeJson(response, 429, {
      status: "rate_limited",
      message: `Too many requests. Max ${rateLimitPerSecond} per second.`
    });
  }

  const screenshotMatch =
    request.method === "GET" && request.url?.match(/^\/sessions\/([^/]+)\/screenshot$/);
  if (screenshotMatch) {
    const authFailure = authorizeGatewayRequest(request, gatewayApiKey);
    if (authFailure) {
      return writeJson(response, authFailure.statusCode, authFailure.payload);
    }
    return handleScreenshotRequest(decodeURIComponent(screenshotMatch[1]), response);
  }

  if (request.method === "POST" && request.url === "/shutdown") {
    const contentTypeFailure = validateJsonContentType(request);
    if (contentTypeFailure) {
      return writeJson(response, contentTypeFailure.statusCode, contentTypeFailure.payload);
    }
    const authFailure = authorizeGatewayRequest(request, gatewayApiKey);
    if (authFailure) {
      return writeJson(response, authFailure.statusCode, authFailure.payload);
    }

    writeJson(response, 202, { status: "shutting_down" });
    void gracefulShutdown("shutdown endpoint");
    return;
  }

  if (shuttingDown) {
    return writeJson(response, 503, {
      status: "shutting_down",
      message: "Gateway is draining and not accepting new work"
    });
  }

  if (request.method === "POST" && request.url === "/workflows/execute") {
    const contentTypeFailure = validateJsonContentType(request);
    if (contentTypeFailure) {
      return writeJson(response, contentTypeFailure.statusCode, contentTypeFailure.payload);
    }
    const authFailure = authorizeGatewayRequest(request, gatewayApiKey);
    if (authFailure) {
      return writeJson(response, authFailure.statusCode, authFailure.payload);
    }
    return handleWorkflowExecution(request, response);
  }

  const overrideMatch = request.method === "POST" && request.url?.match(/^\/sessions\/([^/]+)\/override$/);
  if (overrideMatch) {
    const contentTypeFailure = validateJsonContentType(request);
    if (contentTypeFailure) {
      return writeJson(response, contentTypeFailure.statusCode, contentTypeFailure.payload);
    }
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
if ((process.env.CLAW_SANDBOX_BACKEND ?? "local") === "local") {
  log("warn", "sandbox_backend_local", {
    message: "Sandbox backend is 'local' - custom tools run without isolation. Do not use in production."
  });
}

server.on("upgrade", (request, socket, head) => {
  if (shuttingDown) {
    socket.write("HTTP/1.1 503 Service Unavailable\r\n\r\n");
    socket.destroy();
    return;
  }

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

server.on("connection", (socket) => {
  openSockets.add(socket);
  socket.on("close", () => {
    openSockets.delete(socket);
  });
});

server.listen(port, "127.0.0.1", () => {
  log("info", "server_started", { port });
});

server.on("close", () => {
  void closeCheckpointStore();
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
    activeExecutions += 1;
    const compiled = await loadCompiledDocument(payload.ast_hash, workspaceRoot, streamingCheckpoints);
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
    activeExecutions = Math.max(0, activeExecutions - 1);
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
  const startedAt = Date.now();
  let payload: ExecutionRequest | null = null;
  try {
    const raw = JSON.parse(await readBody(request));
    payload = validateExecutionRequest(raw);
    activeExecutions += 1;
    try {
      await logWorkflowRequest(payload);
      log("info", "workflow_received", {
        session_id: payload.session_id,
        workflow: payload.workflow,
        ast_hash: payload.ast_hash
      });

      const compiled = await loadCompiledDocument(payload.ast_hash, workspaceRoot, checkpointStore);
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
      log("info", "workflow_completed", {
        session_id: payload.session_id,
        duration_ms: Date.now() - startedAt
      });
    } finally {
      activeExecutions = Math.max(0, activeExecutions - 1);
    }
  } catch (error) {
    if (error instanceof HumanInterventionRequiredError) {
      log("warn", "human_intervention", {
        session_id: error.event.session_id,
        reason: error.event.reason
      });
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

    log("error", "workflow_failed", {
      session_id: payload?.session_id ?? "unknown",
      error: error instanceof Error ? error.message : "Unknown gateway failure"
    });
    writeJson(response, 500, {
      status: "error",
      message: error instanceof Error ? error.message : "Unknown gateway failure"
    });
  }
}

async function handleScreenshotRequest(
  sessionId: string,
  response: import("node:http").ServerResponse
): Promise<void> {
  try {
    const screenshot = await readFile(sessionScreenshotPath(sessionId));
    writeBinary(response, 200, screenshot, "image/png");
  } catch {
    writeJson(response, 404, {
      status: "not_found",
      message: `No screenshot available for session ${sessionId}`
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

function validateJsonContentType(
  request: import("node:http").IncomingMessage
): {
  statusCode: number;
  payload: { status: string; message: string };
} | null {
  const rawContentType = request.headers["content-type"];
  const contentType = (Array.isArray(rawContentType) ? rawContentType[0] : rawContentType)
    ?.split(";")[0]
    .trim()
    .toLowerCase();

  if (contentType !== "application/json") {
    return {
      statusCode: 415,
      payload: {
        status: "error",
        message: "Content-Type must be application/json"
      }
    };
  }

  return null;
}

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
  "x-content-type-options": "nosniff",
  "x-frame-options": "DENY"
};

function securityHeaders(contentType: string): Record<string, string> {
  const headers: Record<string, string> = {
    ...SECURITY_HEADERS,
    "content-type": contentType
  };

  if (corsOrigin) {
    headers["access-control-allow-origin"] = corsOrigin;
    headers["access-control-allow-methods"] = "GET, POST, OPTIONS";
    headers["access-control-allow-headers"] = "Content-Type, x-claw-key, Authorization";
    if (corsOrigin !== "*") {
      headers.vary = "Origin";
    }
  }

  return headers;
}

function writeJson(
  response: import("node:http").ServerResponse,
  statusCode: number,
  payload: unknown
): void {
  response.writeHead(statusCode, securityHeaders("application/json"));
  response.end(JSON.stringify(payload));
}

function writeBinary(
  response: import("node:http").ServerResponse,
  statusCode: number,
  payload: Buffer,
  contentType: string
): void {
  response.writeHead(statusCode, securityHeaders(contentType));
  response.end(payload);
}

function writeEmpty(response: import("node:http").ServerResponse, statusCode: number): void {
  response.writeHead(statusCode, securityHeaders("application/json"));
  response.end();
}

function resolveProjectRoot(startPath: string): string {
  let current = resolve(startPath);

  while (true) {
    if (existsSync(join(current, "claw.json"))) {
      return current;
    }

    const parent = dirname(current);
    if (parent === current) {
      return resolve(startPath);
    }
    current = parent;
  }
}

function sessionScreenshotPath(sessionId: string): string {
  return join(screenshotRoot, sessionId, "captcha.png");
}

async function gracefulShutdown(reason: string): Promise<void> {
  if (shutdownPromise) {
    return shutdownPromise;
  }

  shuttingDown = true;
  log("info", "shutdown_started", {
    signal: reason,
    active_sessions: activeExecutions
  });

  shutdownPromise = (async () => {
    server.close();

    const drained = await waitForDrain(30_000);
    if (!drained) {
      log("warn", "shutdown_timeout", {
        active_sessions: activeExecutions
      });
    }

    for (const socket of openSockets) {
      socket.destroy();
    }

    await closeCheckpointStore();
    log("info", "shutdown_complete", { drained });
    process.exit(0);
  })();

  return shutdownPromise;
}

async function waitForDrain(timeoutMs: number): Promise<boolean> {
  const deadline = Date.now() + timeoutMs;

  while (Date.now() < deadline) {
    if (activeExecutions === 0) {
      return true;
    }
    await new Promise((resolvePromise) => setTimeout(resolvePromise, 100));
  }

  return activeExecutions === 0;
}

async function closeCheckpointStore(): Promise<void> {
  if (checkpointStoreClosed) {
    return;
  }
  checkpointStoreClosed = true;
  rateLimiter.close();
  await checkpointStore.close();
}

process.on("SIGINT", () => {
  void gracefulShutdown("SIGINT");
});

process.on("SIGTERM", () => {
  void gracefulShutdown("SIGTERM");
});
