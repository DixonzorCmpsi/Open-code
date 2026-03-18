import test from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync } from "node:fs";
import { rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";

import { CheckpointStore } from "./checkpoints.ts";
import { executeWorkflow } from "./traversal.ts";
import type { CompiledDocumentFile, ExecutionState } from "../types.ts";

test("engine executes a simple workflow and checkpoints completion", async () => {
  const tempDir = mkdtempSync(join(tmpdir(), "openclaw-engine-"));
  try {
    const compiled: CompiledDocumentFile = {
      ast_hash: "hash123",
      document: {
        imports: [],
        types: [
          {
            name: "Greeting",
            fields: [
              {
                name: "message",
                data_type: { String: { start: 0, end: 0 } },
                constraints: [],
                span: { start: 0, end: 0 }
              }
            ],
            span: { start: 0, end: 0 }
          }
        ],
        clients: [],
        tools: [],
        agents: [
          {
            name: "Greeter",
            extends: null,
            client: null,
            system_prompt: null,
            tools: [],
            settings: { entries: [], span: { start: 0, end: 0 } },
            span: { start: 0, end: 0 }
          }
        ],
        workflows: [
          {
            name: "Hello",
            arguments: [
              {
                name: "name",
                data_type: { String: { start: 0, end: 0 } },
                constraints: [],
                span: { start: 0, end: 0 }
              }
            ],
            return_type: { Custom: ["Greeting", { start: 0, end: 0 }] },
            body: {
              statements: [
                {
                  LetDecl: {
                    name: "result",
                    explicit_type: { Custom: ["Greeting", { start: 0, end: 0 }] },
                    value: {
                      ExecuteRun: {
                        agent_name: "Greeter",
                        kwargs: [["task", { Identifier: "name" }]],
                        require_type: { Custom: ["Greeting", { start: 0, end: 0 }] }
                      }
                    },
                    span: { start: 0, end: 0 }
                  }
                },
                {
                  Return: {
                    value: { Identifier: "result" },
                    span: { start: 0, end: 0 }
                  }
                }
              ],
              span: { start: 0, end: 0 }
            },
            span: { start: 0, end: 0 }
          }
        ],
        listeners: [],
        tests: [],
        mocks: [],
        span: { start: 0, end: 0 }
      }
    };

    const checkpoints = new CheckpointStore(join(tempDir, "engine.sqlite"));
    const result = await executeWorkflow({
      compiled,
      request: {
        workflow: "Hello",
        arguments: { name: "Alice" },
        ast_hash: "hash123",
        session_id: "sess_1"
      },
      checkpoints,
      workspaceRoot: tempDir
    });

    assert.equal(typeof result, "object");
    assert.equal((result as { message: string }).message, "message-alice");

    const session = await checkpoints.loadSession("sess_1");
    assert.ok(session);
    assert.equal(session?.state.status, "completed");
    await checkpoints.close();
  } finally {
    rmSync(tempDir, { recursive: true, force: true });
  }
});

test("engine executes nested workflow calls via the Call expression", async () => {
  const tempDir = mkdtempSync(join(tmpdir(), "openclaw-engine-"));
  try {
    const compiled: CompiledDocumentFile = {
      ast_hash: "hash-nested",
      document: {
        imports: [],
        types: [
          {
            name: "Label",
            fields: [
              {
                name: "text",
                data_type: { String: { start: 0, end: 0 } },
                constraints: [],
                span: { start: 0, end: 0 }
              }
            ],
            span: { start: 0, end: 0 }
          }
        ],
        clients: [],
        tools: [],
        agents: [
          {
            name: "Labeler",
            extends: null,
            client: null,
            system_prompt: null,
            tools: [],
            settings: { entries: [], span: { start: 0, end: 0 } },
            span: { start: 0, end: 0 }
          }
        ],
        workflows: [
          {
            name: "Inner",
            arguments: [
              {
                name: "input",
                data_type: { String: { start: 0, end: 0 } },
                constraints: [],
                span: { start: 0, end: 0 }
              }
            ],
            return_type: { Custom: ["Label", { start: 0, end: 0 }] },
            body: {
              statements: [
                {
                  LetDecl: {
                    name: "label",
                    explicit_type: { Custom: ["Label", { start: 0, end: 0 }] },
                    value: {
                      ExecuteRun: {
                        agent_name: "Labeler",
                        kwargs: [["task", { Identifier: "input" }]],
                        require_type: { Custom: ["Label", { start: 0, end: 0 }] }
                      }
                    },
                    span: { start: 0, end: 0 }
                  }
                },
                {
                  Return: {
                    value: { Identifier: "label" },
                    span: { start: 0, end: 0 }
                  }
                }
              ],
              span: { start: 0, end: 0 }
            },
            span: { start: 0, end: 0 }
          },
          {
            name: "Outer",
            arguments: [
              {
                name: "name",
                data_type: { String: { start: 0, end: 0 } },
                constraints: [],
                span: { start: 0, end: 0 }
              }
            ],
            return_type: { Custom: ["Label", { start: 0, end: 0 }] },
            body: {
              statements: [
                {
                  LetDecl: {
                    name: "result",
                    explicit_type: { Custom: ["Label", { start: 0, end: 0 }] },
                    value: {
                      Call: ["Inner", [{ Identifier: "name" }]]
                    },
                    span: { start: 0, end: 0 }
                  }
                },
                {
                  Return: {
                    value: { Identifier: "result" },
                    span: { start: 0, end: 0 }
                  }
                }
              ],
              span: { start: 0, end: 0 }
            },
            span: { start: 0, end: 0 }
          }
        ],
        listeners: [],
        tests: [],
        mocks: [],
        span: { start: 0, end: 0 }
      }
    };

    const checkpoints = new CheckpointStore(join(tempDir, "engine.sqlite"));
    const result = await executeWorkflow({
      compiled,
      request: {
        workflow: "Outer",
        arguments: { name: "world" },
        ast_hash: "hash-nested",
        session_id: "sess_nested"
      },
      checkpoints,
      workspaceRoot: tempDir
    });

    assert.equal(typeof result, "object");
    assert.equal((result as { text: string }).text, "text-world");
    await checkpoints.close();
  } finally {
    rmSync(tempDir, { recursive: true, force: true });
  }
});

test("engine resumes a waiting browser session after manual override", async () => {
  const tempDir = mkdtempSync(join(tmpdir(), "openclaw-engine-"));
  try {
    const compiled: CompiledDocumentFile = {
      ast_hash: "hash-browser",
      document: {
        imports: [],
        types: [
          {
            name: "BrowserResult",
            fields: [
              {
                name: "url",
                data_type: { String: { start: 0, end: 0 } },
                constraints: [],
                span: { start: 0, end: 0 }
              },
              {
                name: "text",
                data_type: { String: { start: 0, end: 0 } },
                constraints: [],
                span: { start: 0, end: 0 }
              }
            ],
            span: { start: 0, end: 0 }
          }
        ],
        clients: [],
        tools: [],
        agents: [
          {
            name: "Navigator",
            extends: null,
            client: null,
            system_prompt: null,
            tools: ["Browser.navigate"],
            settings: { entries: [], span: { start: 0, end: 0 } },
            span: { start: 0, end: 0 }
          }
        ],
        workflows: [
          {
            name: "VisitPage",
            arguments: [
              {
                name: "url",
                data_type: { String: { start: 0, end: 0 } },
                constraints: [],
                span: { start: 0, end: 0 }
              }
            ],
            return_type: { Custom: ["BrowserResult", { start: 0, end: 0 }] },
            body: {
              statements: [
                {
                  LetDecl: {
                    name: "page",
                    explicit_type: { Custom: ["BrowserResult", { start: 0, end: 0 }] },
                    value: {
                      ExecuteRun: {
                        agent_name: "Navigator",
                        kwargs: [["url", { Identifier: "url" }]],
                        require_type: { Custom: ["BrowserResult", { start: 0, end: 0 }] }
                      }
                    },
                    span: { start: 0, end: 0 }
                  }
                },
                {
                  Return: {
                    value: { Identifier: "page" },
                    span: { start: 0, end: 0 }
                  }
                }
              ],
              span: { start: 0, end: 0 }
            },
            span: { start: 0, end: 0 }
          }
        ],
        listeners: [],
        tests: [],
        mocks: [],
        span: { start: 0, end: 0 }
      }
    };

    const checkpoints = new CheckpointStore(join(tempDir, "engine.sqlite"));
    const waitingState: ExecutionState = {
      sessionId: "sess_resume",
      astHash: "hash-browser",
      workflowName: "VisitPage",
      scopes: [{ url: "https://example.com" }],
      frames: [
        {
          kind: "block",
          blockPath: "workflow:VisitPage/body",
          nextIndex: 0,
          createdScope: false
        }
      ],
      returnValue: null,
      status: "waiting_human"
    };

    await checkpoints.checkpoint(
      waitingState,
      "workflow:VisitPage/body/statements/0",
      "human_intervention_required",
      {}
    );
    await checkpoints.saveHumanOverride("sess_resume", {
      result: {
        url: "https://manual.example",
        text: "Manual browser context"
      }
    });

    const result = await executeWorkflow({
      compiled,
      request: {
        workflow: "VisitPage",
        arguments: { url: "https://example.com" },
        ast_hash: "hash-browser",
        session_id: "sess_resume"
      },
      checkpoints,
      workspaceRoot: tempDir
    });

    assert.deepEqual(result, {
      url: "https://manual.example",
      text: "Manual browser context"
    });

    const session = await checkpoints.loadSession("sess_resume");
    assert.ok(session);
    assert.equal(session?.state.status, "completed");
    await checkpoints.close();
  } finally {
    rmSync(tempDir, { recursive: true, force: true });
  }
});
