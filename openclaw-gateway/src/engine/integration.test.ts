import test from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
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
  const tempDir = mkdtempSync(join(tmpdir(), "claw-e2e-"));
  let checkpoints: CheckpointStore | null = null;

  try {
    ensureCompiledDocument();

    // Load the real compiled document produced by `cargo run --bin claw -- build`
    const documentPath = join(repoRoot, "generated", "claw", "document.json");
    const compiled: CompiledDocumentFile = JSON.parse(
      readFileSync(documentPath, "utf-8")
    );

    assert.ok(compiled.ast_hash, "compiled document must have an ast_hash");
    assert.ok(compiled.document.workflows.length > 0, "compiled document must contain workflows");

    const workflow = compiled.document.workflows[0];
    assert.equal(workflow.name, "AnalyzeCompetitors");

    // Execute via the gateway engine (no LLM keys → mock bridge)
    checkpoints = new CheckpointStore(join(tempDir, "e2e.sqlite"));
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

  } finally {
    await checkpoints?.close();
    await new Promise((resolvePromise) => setTimeout(resolvePromise, 25));
    rmSync(tempDir, { recursive: true, force: true });
  }
});

test("end-to-end: gateway rejects invalid workflow names gracefully", async () => {
  const tempDir = mkdtempSync(join(tmpdir(), "claw-e2e-"));
  let checkpoints: CheckpointStore | null = null;

  try {
    ensureCompiledDocument();

    const documentPath = join(repoRoot, "generated", "claw", "document.json");
    const compiled: CompiledDocumentFile = JSON.parse(
      readFileSync(documentPath, "utf-8")
    );

    checkpoints = new CheckpointStore(join(tempDir, "e2e.sqlite"));
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

  } finally {
    await checkpoints?.close();
    await new Promise((resolvePromise) => setTimeout(resolvePromise, 25));
    rmSync(tempDir, { recursive: true, force: true });
  }
});

function ensureCompiledDocument(): void {
  const documentPath = join(repoRoot, "generated", "claw", "document.json");

  try {
    readFileSync(documentPath, "utf-8");
    return;
  } catch {
    // Build the generated gateway document on demand for clean checkouts.
  }

  const build = spawnSync(
    "cargo",
    ["run", "--quiet", "--bin", "claw", "--", "build", "example.claw"],
    {
      cwd: repoRoot,
      stdio: "inherit"
    }
  );

  assert.equal(build.status, 0, "expected cargo to build generated/claw/document.json");
}
