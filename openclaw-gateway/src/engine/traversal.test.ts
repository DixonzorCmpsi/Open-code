import test from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync } from "node:fs";
import { rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { DatabaseSync } from "node:sqlite";

import { CheckpointStore } from "./checkpoints.ts";
import { executeWorkflow } from "./traversal.ts";
import type { CompiledDocumentFile, ExecutionState } from "../types.ts";

function span(start = 0, end = 0) {
  return { start, end };
}

function spanned(expr: Record<string, unknown>, start = 0, end = 0) {
  return { expr, span: span(start, end) };
}

function workflowDocument(workflow: Record<string, unknown>): CompiledDocumentFile {
  return {
    ast_hash: "test-hash",
    document: {
      imports: [],
      types: [],
      clients: [],
      tools: [],
      agents: [],
      workflows: [workflow],
      listeners: [],
      tests: [],
      mocks: [],
      span: span()
    }
  };
}

test("engine executes a simple workflow and checkpoints completion", async () => {
  const tempDir = mkdtempSync(join(tmpdir(), "claw-engine-"));
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
  const tempDir = mkdtempSync(join(tmpdir(), "claw-engine-"));
  const databasePath = join(tempDir, "engine.sqlite");
  const checkpoints = new CheckpointStore(databasePath);
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

    const database = new DatabaseSync(databasePath);
    const nestedSession = database
      .prepare("SELECT session_id FROM sessions WHERE workflow_name = ?")
      .get("Inner") as { session_id: string } | undefined;
    database.close();

    assert.ok(nestedSession);
    assert.match(
      nestedSession.session_id,
      /^sess_nested:Inner:[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i
    );
  } finally {
    await checkpoints.close();
    rmSync(tempDir, { recursive: true, force: true });
  }
});

test("engine resumes a waiting browser session after manual override", async () => {
  const tempDir = mkdtempSync(join(tmpdir(), "claw-engine-"));
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
    // Allow Windows SQLite WAL to flush before destroying the temp folder
    await new Promise((r) => setTimeout(r, 250));
  } finally {
    try {
      rmSync(tempDir, { recursive: true, force: true, maxRetries: 5 });
    } catch {
      // ignore
    }
  }
});

test("engine skips catch bodies when try blocks succeed", async () => {
  const tempDir = mkdtempSync(join(tmpdir(), "claw-engine-"));
  const checkpoints = new CheckpointStore(join(tempDir, "engine.sqlite"));
  try {
    const compiled = workflowDocument({
      name: "TrySuccess",
      arguments: [],
      return_type: { String: span() },
      body: {
        statements: [
          {
            LetDecl: {
              name: "result",
              explicit_type: { String: span() },
              value: spanned({ StringLiteral: "try-succeeded" }),
              span: span()
            }
          },
          {
            TryCatch: {
              try_body: {
                statements: [{ Expression: spanned({ Identifier: "result" }) }],
                span: span()
              },
              catch_name: "error",
              catch_type: { Custom: ["AgentExecutionError", span()] },
              catch_body: {
                statements: [
                  {
                    LetDecl: {
                      name: "result",
                      explicit_type: { String: span() },
                      value: spanned({ StringLiteral: "catch-ran" }),
                      span: span()
                    }
                  }
                ],
                span: span()
              },
              span: span()
            }
          },
          {
            Return: {
              value: spanned({ Identifier: "result" }),
              span: span()
            }
          }
        ],
        span: span()
      },
      span: span()
    });

    const result = await executeWorkflow({
      compiled,
      request: {
        workflow: "TrySuccess",
        arguments: {},
        ast_hash: "test-hash",
        session_id: "sess_try_success"
      },
      checkpoints,
      workspaceRoot: tempDir
    });

    assert.equal(result, "try-succeeded");
  } finally {
    await checkpoints.close();
    rmSync(tempDir, { recursive: true, force: true });
  }
});

test("engine executes catch bodies with bound errors when try blocks fail", async () => {
  const tempDir = mkdtempSync(join(tmpdir(), "claw-engine-"));
  const checkpoints = new CheckpointStore(join(tempDir, "engine.sqlite"));
  try {
    const compiled = workflowDocument({
      name: "TryFailure",
      arguments: [],
      return_type: { String: span() },
      body: {
        statements: [
          {
            LetDecl: {
              name: "result",
              explicit_type: { String: span() },
              value: spanned({ StringLiteral: "not-caught" }),
              span: span()
            }
          },
          {
            TryCatch: {
              try_body: {
                statements: [{ Expression: spanned({ Identifier: "missing" }) }],
                span: span()
              },
              catch_name: "error",
              catch_type: { Custom: ["AgentExecutionError", span()] },
              catch_body: {
                statements: [
                  {
                    LetDecl: {
                      name: "result",
                      explicit_type: { String: span() },
                      value: spanned({
                        MemberAccess: [spanned({ Identifier: "error" }), "message"]
                      }),
                      span: span()
                    }
                  }
                ],
                span: span()
              },
              span: span()
            }
          },
          {
            Return: {
              value: spanned({ Identifier: "result" }),
              span: span()
            }
          }
        ],
        span: span()
      },
      span: span()
    });

    const result = await executeWorkflow({
      compiled,
      request: {
        workflow: "TryFailure",
        arguments: {},
        ast_hash: "test-hash",
        session_id: "sess_try_failure"
      },
      checkpoints,
      workspaceRoot: tempDir
    });

    assert.equal(result, "Unknown variable missing");
  } finally {
    await checkpoints.close();
    rmSync(tempDir, { recursive: true, force: true });
  }
});

test("engine continues outer loops from inside nested if blocks", async () => {
  const tempDir = mkdtempSync(join(tmpdir(), "claw-engine-"));
  const checkpoints = new CheckpointStore(join(tempDir, "engine.sqlite"));
  try {
    const compiled = workflowDocument({
      name: "ContinueLoop",
      arguments: [],
      return_type: { List: [{ String: span() }, span()] },
      body: {
        statements: [
          {
            LetDecl: {
              name: "items",
              explicit_type: { List: [{ String: span() }, span()] },
              value: spanned({
                ArrayLiteral: [
                  spanned({ StringLiteral: "seed" })
                ]
              }),
              span: span()
            }
          },
          {
            ForLoop: {
              item_name: "item",
              iterator: spanned({
                ArrayLiteral: [
                  spanned({ StringLiteral: "a" }),
                  spanned({ StringLiteral: "skip" }),
                  spanned({ StringLiteral: "b" })
                ]
              }),
              body: {
                statements: [
                  {
                    IfCond: {
                      condition: spanned({
                        BinaryOp: {
                          left: spanned({ Identifier: "item" }),
                          op: "Equal",
                          right: spanned({ StringLiteral: "skip" })
                        }
                      }),
                      if_body: {
                        statements: [{ Continue: span() }],
                        span: span()
                      },
                      else_body: null,
                      span: span()
                    }
                  },
                  {
                    Expression: spanned({
                      MethodCall: [spanned({ Identifier: "items" }), "append", [spanned({ Identifier: "item" })]]
                    })
                  }
                ],
                span: span()
              },
              span: span()
            }
          },
          {
            Return: {
              value: spanned({ Identifier: "items" }),
              span: span()
            }
          }
        ],
        span: span()
      },
      span: span()
    });

    const result = await executeWorkflow({
      compiled,
      request: {
        workflow: "ContinueLoop",
        arguments: {},
        ast_hash: "test-hash",
        session_id: "sess_continue"
      },
      checkpoints,
      workspaceRoot: tempDir
    });

    assert.deepEqual(result, ["seed", "a", "b"]);
  } finally {
    await checkpoints.close();
    rmSync(tempDir, { recursive: true, force: true });
  }
});

test("engine breaks out of loops entirely", async () => {
  const tempDir = mkdtempSync(join(tmpdir(), "claw-engine-"));
  const checkpoints = new CheckpointStore(join(tempDir, "engine.sqlite"));
  try {
    const compiled = workflowDocument({
      name: "BreakLoop",
      arguments: [],
      return_type: { List: [{ String: span() }, span()] },
      body: {
        statements: [
          {
            LetDecl: {
              name: "items",
              explicit_type: { List: [{ String: span() }, span()] },
              value: spanned({ ArrayLiteral: [] }),
              span: span()
            }
          },
          {
            ForLoop: {
              item_name: "item",
              iterator: spanned({
                ArrayLiteral: [
                  spanned({ StringLiteral: "a" }),
                  spanned({ StringLiteral: "stop" }),
                  spanned({ StringLiteral: "b" })
                ]
              }),
              body: {
                statements: [
                  {
                    IfCond: {
                      condition: spanned({
                        BinaryOp: {
                          left: spanned({ Identifier: "item" }),
                          op: "Equal",
                          right: spanned({ StringLiteral: "stop" })
                        }
                      }),
                      if_body: {
                        statements: [{ Break: span() }],
                        span: span()
                      },
                      else_body: null,
                      span: span()
                    }
                  },
                  {
                    Expression: spanned({
                      MethodCall: [spanned({ Identifier: "items" }), "append", [spanned({ Identifier: "item" })]]
                    })
                  }
                ],
                span: span()
              },
              span: span()
            }
          },
          {
            Return: {
              value: spanned({ Identifier: "items" }),
              span: span()
            }
          }
        ],
        span: span()
      },
      span: span()
    });

    const result = await executeWorkflow({
      compiled,
      request: {
        workflow: "BreakLoop",
        arguments: {},
        ast_hash: "test-hash",
        session_id: "sess_break"
      },
      checkpoints,
      workspaceRoot: tempDir
    });

    assert.deepEqual(result, ["a"]);
  } finally {
    await checkpoints.close();
    rmSync(tempDir, { recursive: true, force: true });
  }
});

test("engine allows true assertions to pass", async () => {
  const tempDir = mkdtempSync(join(tmpdir(), "claw-engine-"));
  const checkpoints = new CheckpointStore(join(tempDir, "engine.sqlite"));
  try {
    const compiled = workflowDocument({
      name: "AssertPasses",
      arguments: [],
      return_type: { String: span() },
      body: {
        statements: [
          {
            Assert: {
              condition: spanned({ BoolLiteral: true }),
              message: "should not fail",
              span: span()
            }
          },
          {
            Return: {
              value: spanned({ StringLiteral: "ok" }),
              span: span()
            }
          }
        ],
        span: span()
      },
      span: span()
    });

    const result = await executeWorkflow({
      compiled,
      request: {
        workflow: "AssertPasses",
        arguments: {},
        ast_hash: "test-hash",
        session_id: "sess_assert_true"
      },
      checkpoints,
      workspaceRoot: tempDir
    });

    assert.equal(result, "ok");
  } finally {
    await checkpoints.close();
    rmSync(tempDir, { recursive: true, force: true });
  }
});

test("engine throws assertion errors with the provided message", async () => {
  const tempDir = mkdtempSync(join(tmpdir(), "claw-engine-"));
  const checkpoints = new CheckpointStore(join(tempDir, "engine.sqlite"));
  try {
    const compiled = workflowDocument({
      name: "AssertFails",
      arguments: [],
      return_type: null,
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
    });

    await assert.rejects(
      executeWorkflow({
        compiled,
        request: {
          workflow: "AssertFails",
          arguments: {},
          ast_hash: "test-hash",
          session_id: "sess_assert_false"
        },
        checkpoints,
        workspaceRoot: tempDir
      }),
      (error: Error) => {
        assert.equal(error.name, "AssertionError");
        assert.equal(error.message, "must pass");
        return true;
      }
    );
  } finally {
    await checkpoints.close();
    rmSync(tempDir, { recursive: true, force: true });
  }
});

test("engine resolves env() call expressions at runtime", async () => {
  const tempDir = mkdtempSync(join(tmpdir(), "claw-engine-"));
  const checkpoints = new CheckpointStore(join(tempDir, "engine.sqlite"));
  const originalValue = process.env.CLAW_TEST_ENV_VALUE;
  process.env.CLAW_TEST_ENV_VALUE = "resolved-from-env";
  try {
    const compiled = workflowDocument({
      name: "ReadEnv",
      arguments: [],
      return_type: { String: span() },
      body: {
        statements: [
          {
            Return: {
              value: spanned({
                Call: ["env", [spanned({ StringLiteral: "CLAW_TEST_ENV_VALUE" })]]
              }),
              span: span()
            }
          }
        ],
        span: span()
      },
      span: span()
    });

    const result = await executeWorkflow({
      compiled,
      request: {
        workflow: "ReadEnv",
        arguments: {},
        ast_hash: "test-hash",
        session_id: "sess_env_value"
      },
      checkpoints,
      workspaceRoot: tempDir
    });

    assert.equal(result, "resolved-from-env");
  } finally {
    if (originalValue === undefined) {
      delete process.env.CLAW_TEST_ENV_VALUE;
    } else {
      process.env.CLAW_TEST_ENV_VALUE = originalValue;
    }
    await checkpoints.close();
    rmSync(tempDir, { recursive: true, force: true });
  }
});

test("engine errors when client env() bindings reference missing variables", async () => {
  const tempDir = mkdtempSync(join(tmpdir(), "claw-engine-"));
  const checkpoints = new CheckpointStore(join(tempDir, "engine.sqlite"));
  const originalEndpoint = process.env.CLAW_MISSING_ENDPOINT;
  delete process.env.CLAW_MISSING_ENDPOINT;
  try {
    const compiled: CompiledDocumentFile = {
      ast_hash: "hash-client-env",
      document: {
        imports: [],
        types: [
          {
            name: "Greeting",
            fields: [
              {
                name: "message",
                data_type: { String: span() },
                constraints: [],
                span: span()
              }
            ],
            span: span()
          }
        ],
        clients: [
          {
            name: "SecureClient",
            provider: "openai",
            model: "gpt-4o",
            retries: null,
            timeout_ms: null,
            endpoint: spanned({
              Call: ["env", [spanned({ StringLiteral: "CLAW_MISSING_ENDPOINT" })]]
            }),
            api_key: null,
            span: span()
          }
        ],
        tools: [],
        agents: [
          {
            name: "Greeter",
            extends: null,
            client: "SecureClient",
            system_prompt: null,
            tools: [],
            settings: { entries: [], span: span() },
            span: span()
          }
        ],
        workflows: [
          {
            name: "Hello",
            arguments: [],
            return_type: { Custom: ["Greeting", span()] },
            body: {
              statements: [
                {
                  LetDecl: {
                    name: "result",
                    explicit_type: { Custom: ["Greeting", span()] },
                    value: spanned({
                      ExecuteRun: {
                        agent_name: "Greeter",
                        kwargs: [],
                        require_type: { Custom: ["Greeting", span()] }
                      }
                    }),
                    span: span()
                  }
                },
                {
                  Return: {
                    value: spanned({ Identifier: "result" }),
                    span: span()
                  }
                }
              ],
              span: span()
            },
            span: span()
          }
        ],
        listeners: [],
        tests: [],
        mocks: [],
        span: span()
      }
    };

    await assert.rejects(
      executeWorkflow({
        compiled,
        request: {
          workflow: "Hello",
          arguments: {},
          ast_hash: "hash-client-env",
          session_id: "sess_client_env"
        },
        checkpoints,
        workspaceRoot: tempDir
      }),
      (error: Error) => {
        assert.equal(
          error.message,
          "Environment variable CLAW_MISSING_ENDPOINT is not set (required by client SecureClient)"
        );
        return true;
      }
    );
  } finally {
    if (originalEndpoint !== undefined) {
      process.env.CLAW_MISSING_ENDPOINT = originalEndpoint;
    }
    await checkpoints.close();
    rmSync(tempDir, { recursive: true, force: true });
  }
});

test("engine mock registry intercepts agent execution", async () => {
  const tempDir = mkdtempSync(join(tmpdir(), "claw-engine-"));
  const checkpoints = new CheckpointStore(join(tempDir, "engine.sqlite"));
  try {
    const compiled: CompiledDocumentFile = {
      ast_hash: "hash-mock-hit",
      document: {
        imports: [],
        types: [
          {
            name: "Greeting",
            fields: [
              {
                name: "message",
                data_type: { String: span() },
                constraints: [],
                span: span()
              }
            ],
            span: span()
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
            settings: { entries: [], span: span() },
            span: span()
          }
        ],
        workflows: [
          {
            name: "Hello",
            arguments: [],
            return_type: { Custom: ["Greeting", span()] },
            body: {
              statements: [
                {
                  LetDecl: {
                    name: "result",
                    explicit_type: { Custom: ["Greeting", span()] },
                    value: spanned({
                      ExecuteRun: {
                        agent_name: "Greeter",
                        kwargs: [],
                        require_type: { Custom: ["Greeting", span()] }
                      }
                    }),
                    span: span()
                  }
                },
                {
                  Return: {
                    value: spanned({ Identifier: "result" }),
                    span: span()
                  }
                }
              ],
              span: span()
            },
            span: span()
          }
        ],
        listeners: [],
        tests: [],
        mocks: [
          {
            target_agent: "Greeter",
            output: [["message", spanned({ StringLiteral: "mocked" })]],
            span: span()
          }
        ],
        span: span()
      }
    };

    const result = await executeWorkflow({
      compiled,
      request: {
        workflow: "Hello",
        arguments: {},
        ast_hash: "hash-mock-hit",
        session_id: "sess_mock_hit"
      },
      checkpoints,
      workspaceRoot: tempDir
    });

    assert.deepEqual(result, { message: "mocked" });
  } finally {
    await checkpoints.close();
    rmSync(tempDir, { recursive: true, force: true });
  }
});

test("engine mock registry leaves unmocked agents alone", async () => {
  const tempDir = mkdtempSync(join(tmpdir(), "claw-engine-"));
  const checkpoints = new CheckpointStore(join(tempDir, "engine.sqlite"));
  try {
    const compiled: CompiledDocumentFile = {
      ast_hash: "hash-mock-miss",
      document: {
        imports: [],
        types: [
          {
            name: "Greeting",
            fields: [
              {
                name: "message",
                data_type: { String: span() },
                constraints: [],
                span: span()
              }
            ],
            span: span()
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
            settings: { entries: [], span: span() },
            span: span()
          }
        ],
        workflows: [
          {
            name: "Hello",
            arguments: [],
            return_type: { Custom: ["Greeting", span()] },
            body: {
              statements: [
                {
                  LetDecl: {
                    name: "result",
                    explicit_type: { Custom: ["Greeting", span()] },
                    value: spanned({
                      ExecuteRun: {
                        agent_name: "Greeter",
                        kwargs: [],
                        require_type: { Custom: ["Greeting", span()] }
                      }
                    }),
                    span: span()
                  }
                },
                {
                  Return: {
                    value: spanned({ Identifier: "result" }),
                    span: span()
                  }
                }
              ],
              span: span()
            },
            span: span()
          }
        ],
        listeners: [],
        tests: [],
        mocks: [
          {
            target_agent: "SomeoneElse",
            output: [["message", spanned({ StringLiteral: "mocked" })]],
            span: span()
          }
        ],
        span: span()
      }
    };

    const result = await executeWorkflow({
      compiled,
      request: {
        workflow: "Hello",
        arguments: {},
        ast_hash: "hash-mock-miss",
        session_id: "sess_mock_miss"
      },
      checkpoints,
      workspaceRoot: tempDir
    });

    assert.deepEqual(result, { message: "message-claw" });
  } finally {
    await checkpoints.close();
    rmSync(tempDir, { recursive: true, force: true });
  }
});

test("engine mock registry uses the last mock declared for an agent", async () => {
  const tempDir = mkdtempSync(join(tmpdir(), "claw-engine-"));
  const checkpoints = new CheckpointStore(join(tempDir, "engine.sqlite"));
  try {
    const compiled: CompiledDocumentFile = {
      ast_hash: "hash-mock-last",
      document: {
        imports: [],
        types: [
          {
            name: "Greeting",
            fields: [
              {
                name: "message",
                data_type: { String: span() },
                constraints: [],
                span: span()
              }
            ],
            span: span()
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
            settings: { entries: [], span: span() },
            span: span()
          }
        ],
        workflows: [
          {
            name: "Hello",
            arguments: [],
            return_type: { Custom: ["Greeting", span()] },
            body: {
              statements: [
                {
                  LetDecl: {
                    name: "result",
                    explicit_type: { Custom: ["Greeting", span()] },
                    value: spanned({
                      ExecuteRun: {
                        agent_name: "Greeter",
                        kwargs: [],
                        require_type: { Custom: ["Greeting", span()] }
                      }
                    }),
                    span: span()
                  }
                },
                {
                  Return: {
                    value: spanned({ Identifier: "result" }),
                    span: span()
                  }
                }
              ],
              span: span()
            },
            span: span()
          }
        ],
        listeners: [],
        tests: [],
        mocks: [
          {
            target_agent: "Greeter",
            output: [["message", spanned({ StringLiteral: "first" })]],
            span: span()
          },
          {
            target_agent: "Greeter",
            output: [["message", spanned({ StringLiteral: "second" })]],
            span: span()
          }
        ],
        span: span()
      }
    };

    const result = await executeWorkflow({
      compiled,
      request: {
        workflow: "Hello",
        arguments: {},
        ast_hash: "hash-mock-last",
        session_id: "sess_mock_last"
      },
      checkpoints,
      workspaceRoot: tempDir
    });

    assert.deepEqual(result, { message: "second" });
  } finally {
    await checkpoints.close();
    rmSync(tempDir, { recursive: true, force: true });
  }
});
