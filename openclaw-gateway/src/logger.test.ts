import assert from "node:assert/strict";
import test from "node:test";

import { log } from "./logger.ts";

test("logger emits ndjson to stderr when json format is enabled", () => {
  const originalFormat = process.env.CLAW_LOG_FORMAT;
  const originalLevel = process.env.CLAW_LOG_LEVEL;
  const originalWrite = process.stderr.write.bind(process.stderr);
  let output = "";

  process.env.CLAW_LOG_FORMAT = "json";
  process.env.CLAW_LOG_LEVEL = "info";
  process.stderr.write = ((chunk: string | Uint8Array) => {
    output += typeof chunk === "string" ? chunk : Buffer.from(chunk).toString("utf8");
    return true;
  }) as typeof process.stderr.write;

  try {
    log("info", "test_event", { x: 1 });
  } finally {
    process.stderr.write = originalWrite;
    if (originalFormat === undefined) {
      delete process.env.CLAW_LOG_FORMAT;
    } else {
      process.env.CLAW_LOG_FORMAT = originalFormat;
    }
    if (originalLevel === undefined) {
      delete process.env.CLAW_LOG_LEVEL;
    } else {
      process.env.CLAW_LOG_LEVEL = originalLevel;
    }
  }

  const entry = JSON.parse(output.trim()) as Record<string, unknown>;
  assert.equal(entry.level, "info");
  assert.equal(entry.event, "test_event");
  assert.equal(entry.x, 1);
  assert.equal(typeof entry.timestamp, "string");
});

test("logger respects the configured minimum log level", () => {
  const originalFormat = process.env.CLAW_LOG_FORMAT;
  const originalLevel = process.env.CLAW_LOG_LEVEL;
  const originalLog = console.log;
  const originalError = console.error;
  const writes: string[] = [];

  delete process.env.CLAW_LOG_FORMAT;
  process.env.CLAW_LOG_LEVEL = "warn";
  console.log = (...args: unknown[]) => {
    writes.push(args.join(" "));
  };
  console.error = (...args: unknown[]) => {
    writes.push(args.join(" "));
  };

  try {
    log("debug", "suppressed_event", { ok: false });
  } finally {
    console.log = originalLog;
    console.error = originalError;
    if (originalFormat === undefined) {
      delete process.env.CLAW_LOG_FORMAT;
    } else {
      process.env.CLAW_LOG_FORMAT = originalFormat;
    }
    if (originalLevel === undefined) {
      delete process.env.CLAW_LOG_LEVEL;
    } else {
      process.env.CLAW_LOG_LEVEL = originalLevel;
    }
  }

  assert.deepEqual(writes, []);
});
