import assert from "node:assert/strict";
import test from "node:test";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { __testing, executeCustomTool, prePullSandboxImages, retryWithBackoff } from "./runtime.ts";

const workspaceRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..", "..");

test("module runtime resolves dotted module targets and executes exported tools", async () => {
  const result = (await executeCustomTool(
    'module("scripts.search").function("run")',
    { task: "Apple" },
    workspaceRoot
  )) as Record<string, unknown>;

  assert.equal(result.url, "https://apple.com/news");
  assert.equal(result.confidence_score, 0.95);
  assert.deepEqual(result.tags, ["search", "module-runtime"]);
});

test("docker sandbox commands are locked down for python and typescript runtimes", async () => {
  const pythonCommand = await __testing.buildSandboxCommand(
    'python("tools.analysis.runner")',
    workspaceRoot,
    "docker"
  );
  const typescriptCommand = await __testing.buildSandboxCommand(
    'typescript("scripts/search.mjs")',
    workspaceRoot,
    "docker"
  );

  assert.equal(pythonCommand.command, "docker");
  assert.ok(pythonCommand.args.includes("--network=none"));
  assert.ok(pythonCommand.args.includes("--read-only"));
  assert.ok(pythonCommand.args.includes("--cap-drop=ALL"));
  assert.ok(pythonCommand.args.includes("python:3.11-slim"));
  assert.ok(pythonCommand.args.includes("-m"));
  assert.ok(pythonCommand.args.includes("tools.analysis.runner"));

  assert.equal(typescriptCommand.command, "docker");
  assert.ok(typescriptCommand.args.includes("node:22"));
  assert.ok(typescriptCommand.args.includes("--experimental-strip-types"));
  assert.ok(
    typescriptCommand.args.some((value) => value.replaceAll("\\", "/").endsWith("/scripts/search.mjs"))
  );
});

test("sandbox execution aborts after the configured timeout", async () => {
  const originalBackend = process.env.CLAW_SANDBOX_BACKEND;
  process.env.CLAW_SANDBOX_BACKEND = "local";
  try {
    const result = executeCustomTool(
      'python("scripts/sandbox_echo.py")',
      { sleep: 30 },
      workspaceRoot,
      { timeoutMs: 500 }
    );

    await assert.rejects(result, (error: Error) => {
      assert.ok(error.message.includes("timed out"));
      return true;
    });
  } finally {
    if (originalBackend !== undefined) {
      process.env.CLAW_SANDBOX_BACKEND = originalBackend;
    } else {
      delete process.env.CLAW_SANDBOX_BACKEND;
    }
  }
});

test("transient tool failures are retried with exponential backoff", async () => {
  let attempts = 0;
  const fakeTool = async (): Promise<unknown> => {
    attempts += 1;
    if (attempts < 3) {
      throw new Error("transient network failure");
    }
    return { ok: true };
  };

  const result = await retryWithBackoff(fakeTool, { maxRetries: 3, baseDelayMs: 10 });
  assert.deepEqual(result, { ok: true });
  assert.equal(attempts, 3);
});

test("retry exhaustion surfaces the final error", async () => {
  const alwaysFails = async (): Promise<unknown> => {
    throw new Error("persistent failure");
  };

  await assert.rejects(
    retryWithBackoff(alwaysFails, { maxRetries: 2, baseDelayMs: 10 }),
    (error: Error) => {
      assert.ok(error.message.includes("persistent failure"));
      return true;
    }
  );
});

test("prePullSandboxImages is a no-op when sandbox backend is not docker", async () => {
  const originalBackend = process.env.CLAW_SANDBOX_BACKEND;
  try {
    delete process.env.CLAW_SANDBOX_BACKEND;
    // Should resolve immediately without spawning anything
    await prePullSandboxImages();
  } finally {
    if (originalBackend !== undefined) {
      process.env.CLAW_SANDBOX_BACKEND = originalBackend;
    }
  }
});
