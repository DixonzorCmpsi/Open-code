import assert from "node:assert/strict";
import { spawn, type ChildProcess } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { createServer } from "node:net";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const gatewayRoot = join(repoRoot, "openclaw-gateway");

test("server rejects non-json content types for every POST endpoint", async () => {
  const instance = await startGateway();
  try {
    const cases = [
      "/workflows/execute",
      "/sessions/test-session/override",
      "/shutdown"
    ];

    for (const path of cases) {
      const response = await fetch(`http://127.0.0.1:${instance.port}${path}`, {
        method: "POST",
        headers: {
          "content-type": "text/plain"
        },
        body: "not-json"
      });

      assert.equal(response.status, 415);
      assert.deepEqual(await response.json(), {
        status: "error",
        message: "Content-Type must be application/json"
      });
    }
  } finally {
    await stopGateway(instance);
  }
});

test("server accepts application/json content types with charset parameters", async () => {
  const instance = await startGateway();
  try {
    const executeResponse = await fetch(`http://127.0.0.1:${instance.port}/workflows/execute`, {
      method: "POST",
      headers: {
        "content-type": "application/json; charset=utf-8"
      },
      body: "{}"
    });
    assert.equal(executeResponse.status, 400);

    const overrideResponse = await fetch(
      `http://127.0.0.1:${instance.port}/sessions/test-session/override`,
      {
        method: "POST",
        headers: {
          "content-type": "application/json; charset=utf-8"
        },
        body: JSON.stringify({ approved: true })
      }
    );
    assert.equal(overrideResponse.status, 202);

    const shutdownResponse = await fetch(`http://127.0.0.1:${instance.port}/shutdown`, {
      method: "POST",
      headers: {
        "content-type": "application/json; charset=utf-8"
      },
      body: "{}"
    });
    assert.equal(shutdownResponse.status, 202);

    await waitForExit(instance.child);
  } finally {
    await stopGateway(instance);
  }
});

test("server returns 429 when the per-address rate limit is exceeded", async () => {
  const instance = await startGateway({
    CLAW_RATE_LIMIT: "1"
  });
  try {
    const firstResponse = await fetch(`http://127.0.0.1:${instance.port}/missing`);
    assert.equal(firstResponse.status, 404);

    const secondResponse = await fetch(`http://127.0.0.1:${instance.port}/missing`);
    assert.equal(secondResponse.status, 429);
    assert.deepEqual(await secondResponse.json(), {
      status: "rate_limited",
      message: "Too many requests. Max 1 per second."
    });
  } finally {
    await stopGateway(instance);
  }
});

interface GatewayInstance {
  child: ChildProcess;
  port: number;
  tempDir: string;
}

async function startGateway(
  extraEnv: Record<string, string> = {}
): Promise<GatewayInstance> {
  const port = await reservePort();
  const tempDir = mkdtempSync(join(tmpdir(), "claw-gateway-server-"));
  const child = spawn("node", ["--experimental-strip-types", "src/server.ts"], {
    cwd: gatewayRoot,
    env: {
      ...process.env,
      CLAW_GATEWAY_PORT: String(port),
      CLAW_PROJECT_ROOT: repoRoot,
      CLAW_STATE_DIR: tempDir,
      CLAW_SCREENSHOT_DIR: join(tempDir, "screenshots"),
      CLAW_SANDBOX_BACKEND: "local",
      ...extraEnv
    },
    stdio: ["ignore", "pipe", "pipe"]
  });

  await waitForHealthy(port, child);

  return { child, port, tempDir };
}

async function stopGateway(instance: GatewayInstance): Promise<void> {
  try {
    if (!instance.child.killed && instance.child.exitCode === null) {
      await fetch(`http://127.0.0.1:${instance.port}/shutdown`, {
        method: "POST",
        headers: {
          "content-type": "application/json"
        },
        body: "{}"
      }).catch(() => undefined);
      await Promise.race([waitForExit(instance.child), delay(2_000)]);
      if (instance.child.exitCode === null) {
        instance.child.kill();
        await waitForExit(instance.child);
      }
    }
  } finally {
    rmSync(instance.tempDir, { recursive: true, force: true });
  }
}

async function waitForHealthy(port: number, child: ChildProcess): Promise<void> {
  const deadline = Date.now() + 10_000;
  while (Date.now() < deadline) {
    if (child.exitCode !== null) {
      throw new Error(`gateway exited before becoming healthy (exit ${child.exitCode})`);
    }

    try {
      const response = await fetch(`http://127.0.0.1:${port}/health`);
      if (response.ok) {
        return;
      }
    } catch {
      // Retry until the child is listening.
    }

    await delay(100);
  }

  throw new Error("timed out waiting for gateway health endpoint");
}

async function waitForExit(child: ChildProcess): Promise<void> {
  if (child.exitCode !== null) {
    return;
  }

  await new Promise<void>((resolvePromise, rejectPromise) => {
    child.once("exit", () => resolvePromise());
    child.once("error", rejectPromise);
  });
}

async function reservePort(): Promise<number> {
  return new Promise<number>((resolvePromise, rejectPromise) => {
    const server = createServer();
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      if (!address || typeof address === "string") {
        server.close();
        rejectPromise(new Error("failed to reserve a TCP port"));
        return;
      }

      const { port } = address;
      server.close((error) => {
        if (error) {
          rejectPromise(error);
          return;
        }
        resolvePromise(port);
      });
    });
    server.on("error", rejectPromise);
  });
}
