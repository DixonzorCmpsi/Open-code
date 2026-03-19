import { randomUUID } from "node:crypto";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

import { CheckpointStore } from "./checkpoints.ts";
import { AssertionError } from "./errors.ts";
import { executeWorkflow } from "./traversal.ts";
import type { Block, CompiledDocumentFile, TestDecl, WorkflowDecl } from "../types.ts";

export interface TestManifest {
  compiled: CompiledDocumentFile;
  tests: TestDecl[];
}

export interface TestResult {
  name: string;
  status: "pass" | "fail";
  duration_ms: number;
  error?: string;
  node_path?: string;
}

interface RunTestsOptions {
  timeoutMs?: number;
  workspaceRoot?: string;
}

export async function runTests(
  manifest: TestManifest,
  options: RunTestsOptions = {}
): Promise<TestResult[]> {
  const timeoutMs = options.timeoutMs ?? Number(process.env.CLAW_TEST_TIMEOUT_MS ?? 30_000);
  const workspaceRoot = options.workspaceRoot ?? process.env.CLAW_PROJECT_ROOT ?? process.cwd();
  const selectedTests = manifest.tests.length > 0 ? manifest.tests : manifest.compiled.document.tests;
  const results: TestResult[] = [];

  for (let index = 0; index < selectedTests.length; index += 1) {
    const testDecl = selectedTests[index]!;
    const startedAt = Date.now();
    const checkpoints = new CheckpointStore({ databasePath: ":memory:" });

    try {
      const compiled = buildTestDocument(manifest.compiled, testDecl, index);
      const workflowName = syntheticWorkflowName(index);
      await withTimeout(
        executeWorkflow({
          compiled,
          request: {
            workflow: workflowName,
            arguments: {},
            ast_hash: manifest.compiled.ast_hash,
            session_id: `test:${testDecl.name}:${randomUUID()}`
          },
          checkpoints,
          workspaceRoot
        }),
        timeoutMs
      );

      results.push({
        name: testDecl.name,
        status: "pass",
        duration_ms: Date.now() - startedAt
      });
    } catch (error) {
      results.push({
        name: testDecl.name,
        status: "fail",
        duration_ms: Date.now() - startedAt,
        error: error instanceof Error ? error.message : String(error),
        node_path: normalizeNodePath(index, testDecl.name, error)
      });
    } finally {
      await checkpoints.close();
    }
  }

  return results;
}

async function main(): Promise<void> {
  const input = await readStdin();
  const manifest = JSON.parse(input) as TestManifest;
  const startedAt = Date.now();
  const results = await runTests(manifest);

  for (const result of results) {
    process.stdout.write(`${JSON.stringify(result)}\n`);
  }

  const passed = results.filter((result) => result.status === "pass").length;
  const failed = results.length - passed;
  process.stdout.write(
    `${JSON.stringify({
      summary: true,
      passed,
      failed,
      total_ms: Date.now() - startedAt
    })}\n`
  );

  process.exit(failed === 0 ? 0 : 1);
}

function buildTestDocument(
  compiled: CompiledDocumentFile,
  testDecl: TestDecl,
  index: number
): CompiledDocumentFile {
  const syntheticWorkflow: WorkflowDecl = {
    name: syntheticWorkflowName(index),
    arguments: [],
    return_type: null,
    body: cloneBlock(testDecl.body),
    span: testDecl.span
  };

  return {
    ast_hash: compiled.ast_hash,
    document: {
      ...compiled.document,
      workflows: [...compiled.document.workflows, syntheticWorkflow]
    }
  };
}

function syntheticWorkflowName(index: number): string {
  return `__claw_test_${index}`;
}

function cloneBlock(block: Block): Block {
  return {
    statements: [...block.statements],
    span: block.span
  };
}

function normalizeNodePath(index: number, testName: string, error: unknown): string | undefined {
  if (!(error instanceof AssertionError) && !(error instanceof Error && "nodePath" in error)) {
    return undefined;
  }

  const rawNodePath =
    error instanceof AssertionError
      ? error.nodePath
      : typeof (error as { nodePath?: unknown }).nodePath === "string"
        ? (error as { nodePath: string }).nodePath
        : undefined;
  if (!rawNodePath) {
    return undefined;
  }

  return rawNodePath.replace(
    `workflow:${syntheticWorkflowName(index)}/body`,
    `test:${testName}/body`
  );
}

async function readStdin(): Promise<string> {
  const chunks: Buffer[] = [];
  for await (const chunk of process.stdin) {
    chunks.push(typeof chunk === "string" ? Buffer.from(chunk) : chunk);
  }
  return Buffer.concat(chunks).toString("utf8");
}

async function withTimeout<T>(promise: Promise<T>, timeoutMs: number): Promise<T> {
  return await Promise.race([
    promise,
    new Promise<T>((_, reject) => {
      const timer = setTimeout(() => {
        reject(new Error(`Test timed out after ${timeoutMs}ms`));
      }, timeoutMs);
      timer.unref?.();
    })
  ]);
}

if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) {
  void main().catch((error) => {
    process.stderr.write(
      `${JSON.stringify({
        status: "error",
        message: error instanceof Error ? error.message : String(error)
      })}\n`
    );
    process.exit(1);
  });
}
