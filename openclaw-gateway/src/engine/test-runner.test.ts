import assert from "node:assert/strict";
import test from "node:test";

import { runTests } from "./test-runner.ts";
import type { CompiledDocumentFile, TestDecl } from "../types.ts";

function span(start = 0, end = 0) {
  return { start, end };
}

function spanned(expr: Record<string, unknown>) {
  return { expr, span: span() };
}

function compiledDocument(tests: TestDecl[]): CompiledDocumentFile {
  return {
    ast_hash: "test-hash",
    document: {
      imports: [],
      types: [],
      clients: [],
      tools: [],
      agents: [],
      workflows: [],
      listeners: [],
      tests,
      mocks: [],
      span: span()
    }
  };
}

test("test runner reports passing test blocks as pass", async () => {
  const results = await runTests({
    compiled: compiledDocument([
      {
        name: "asserts true",
        body: {
          statements: [
            {
              Assert: {
                condition: spanned({ BoolLiteral: true }),
                message: "should pass",
                span: span()
              }
            }
          ],
          span: span()
        },
        span: span()
      }
    ]),
    tests: []
  });

  assert.equal(results[0]?.status, "pass");
});

test("test runner reports assertion failures as fail with node paths", async () => {
  const results = await runTests({
    compiled: compiledDocument([
      {
        name: "asserts false",
        body: {
          statements: [
            {
              Assert: {
                condition: spanned({ BoolLiteral: false }),
                message: "must pass",
                span: span()
              }
            }
          ],
          span: span()
        },
        span: span()
      }
    ]),
    tests: []
  });

  assert.equal(results[0]?.status, "fail");
  assert.equal(results[0]?.error, "must pass");
  assert.equal(results[0]?.node_path, "test:asserts false/body/statements/0");
});
