import test from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, rmSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { CheckpointStore } from "./checkpoints.ts";
import { executeWorkflow } from "./traversal.ts";
import type { CompiledDocumentFile } from "../types.ts";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..", "..");

test("end-to-end: compile → load document → execute workflow → validate result", async () => {
  const tempDir = mkdtempSync(join(tmpdir(), "openclaw-e2e-"));

  try {
    // Load the real compiled document produced by `cargo run --bin openclaw -- build`
    const documentPath = join(repoRoot, "generated", "claw", "document.json");
    const compiled: CompiledDocumentFile = JSON.parse(
      readFileSync(documentPath, "utf-8")
    );

    assert.ok(compiled.ast_hash, "compiled document must have an ast_hash");
    assert.ok(compiled.document.workflows.length > 0, "compiled document must contain workflows");

    const workflow = compiled.document.workflows[0];
    assert.equal(workflow.name, "AnalyzeCompetitors");

    // Execute via the gateway engine (no LLM keys → mock bridge)
    const checkpoints = new CheckpointStore(join(tempDir, "e2e.sqlite"));
    const result = await executeWorkflow({
      compiled,
      request: {
        workflow: "AnalyzeCompetitors",
        arguments: { company: "Acme" },
        ast_hash: compiled.ast_hash,
        session_id: "e2e_sess_1"
      },
      checkpoints,
      workspaceRoot: repoRoot
    });

    // The mock bridge should produce a SearchResult conforming to the schema
    assert.equal(typeof result, "object");
    const searchResult = result as Record<string, unknown>;
    assert.equal(typeof searchResult.url, "string");
    assert.ok((searchResult.url as string).startsWith("https://"), "url must match regex constraint");
    assert.equal(typeof searchResult.confidence_score, "number");
    assert.equal(typeof searchResult.snippet, "string");
    assert.ok(Array.isArray(searchResult.tags), "tags must be an array");

    // Verify checkpoint recorded the session as completed
    const session = await checkpoints.loadSession("e2e_sess_1");
    assert.ok(session, "session should be persisted");
    assert.equal(session?.state.status, "completed");

    // Verify idempotent replay returns the same result
    const replayed = await executeWorkflow({
      compiled,
      request: {
        workflow: "AnalyzeCompetitors",
        arguments: { company: "Acme" },
        ast_hash: compiled.ast_hash,
        session_id: "e2e_sess_1"
      },
      checkpoints,
      workspaceRoot: repoRoot
    });
    assert.deepEqual(replayed, result, "replaying a completed session returns the cached result");

    await checkpoints.close();
  } finally {
    rmSync(tempDir, { recursive: true, force: true });
  }
});

test("end-to-end: gateway rejects invalid workflow names gracefully", async () => {
  const tempDir = mkdtempSync(join(tmpdir(), "openclaw-e2e-"));

  try {
    const documentPath = join(repoRoot, "generated", "claw", "document.json");
    const compiled: CompiledDocumentFile = JSON.parse(
      readFileSync(documentPath, "utf-8")
    );

    const checkpoints = new CheckpointStore(join(tempDir, "e2e.sqlite"));
    await assert.rejects(
      executeWorkflow({
        compiled,
        request: {
          workflow: "NonExistentWorkflow",
          arguments: {},
          ast_hash: compiled.ast_hash,
          session_id: "e2e_sess_bad"
        },
        checkpoints,
        workspaceRoot: repoRoot
      }),
      (error: Error) => {
        assert.ok(error.message.includes("NonExistentWorkflow"));
        return true;
      }
    );

    await checkpoints.close();
  } finally {
    rmSync(tempDir, { recursive: true, force: true });
  }
});
