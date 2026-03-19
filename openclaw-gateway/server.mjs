import { createServer } from "node:http";
import { appendFile, mkdir } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const port = Number(process.env.CLAW_GATEWAY_PORT ?? 8080);
const rootDir = dirname(fileURLToPath(import.meta.url));
const stateDir = join(rootDir, ".claw");
const logFile = join(stateDir, "workflow-ingestor.ndjson");

const workflowResults = {
  AnalyzeCompetitors: {
    url: "https://apple.com/news",
    confidence_score: 0.95,
    snippet: "Apple releases new XR headset.",
    tags: ["hardware", "xr"]
  }
};

const server = createServer(async (request, response) => {
  if (request.method === "GET" && request.url === "/health") {
    return writeJson(response, 200, { status: "ok", log_file: logFile });
  }

  if (request.method !== "POST" || request.url !== "/workflows/execute") {
    return writeJson(response, 404, { status: "error", message: "Not found" });
  }

  try {
    const body = await readBody(request);
    const payload = JSON.parse(body);
    const entry = {
      received_at: new Date().toISOString(),
      ...payload
    };

    await mkdir(stateDir, { recursive: true });
    await appendFile(logFile, `${JSON.stringify(entry)}\n`);
    console.log(
      `[Workflow Ingestor] workflow=${payload.workflow} ast_hash=${payload.ast_hash} session_id=${payload.session_id}`
    );

    const result = workflowResults[payload.workflow] ?? {
      workflow: payload.workflow,
      arguments: payload.arguments
    };

    return writeJson(response, 200, {
      session_id: payload.session_id,
      status: "success",
      result
    });
  } catch (error) {
    console.error("[Workflow Ingestor] request failed", error);
    return writeJson(response, 500, {
      status: "error",
      message: error instanceof Error ? error.message : "Unknown gateway error"
    });
  }
});

server.listen(port, () => {
  console.log(`[claw-gateway] listening on http://127.0.0.1:${port}`);
  console.log(`[claw-gateway] log file ${logFile}`);
});

function readBody(request) {
  return new Promise((resolve, reject) => {
    let body = "";
    request.setEncoding("utf8");
    request.on("data", (chunk) => {
      body += chunk;
    });
    request.on("end", () => resolve(body));
    request.on("error", reject);
  });
}

function writeJson(response, statusCode, payload) {
  response.writeHead(statusCode, {
    "content-type": "application/json"
  });
  response.end(JSON.stringify(payload));
}
