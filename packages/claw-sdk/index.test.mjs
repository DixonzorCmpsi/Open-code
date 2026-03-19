import assert from "node:assert/strict";
import test from "node:test";

import {
  AgentExecutionError,
  ClawClient,
  ClawExecutionError
} from "./index.js";

test("ClawClient forwards api keys to the gateway", async () => {
  const originalFetch = globalThis.fetch;
  try {
    globalThis.fetch = async (url, options) => {
      assert.equal(url, "http://127.0.0.1:8080/workflows/execute");
      assert.equal(options.headers["x-claw-key"], "prod_secret");
      return {
        ok: true,
        async json() {
          return {
            status: "success",
            result: { ok: true }
          };
        }
      };
    };

    const client = new ClawClient({
      endpoint: "http://127.0.0.1:8080",
      api_key: "prod_secret"
    });
    const result = await client.executeWorkflow({
      workflowName: "AnalyzeCompetitors",
      arguments: { company: "Apple" },
      astHash: "hash123"
    });

    assert.deepEqual(result, { ok: true });
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("ClawClient raises resumable execution errors with the session id", async () => {
  const originalFetch = globalThis.fetch;
  try {
    globalThis.fetch = async () => ({
      ok: false,
      async json() {
        return {
          status: "forbidden",
          message: "Invalid Claw API key",
          session_id: "sess_recover"
        };
      }
    });

    const client = new ClawClient();
    await assert.rejects(
      () =>
        client.executeWorkflow({
          workflowName: "AnalyzeCompetitors",
          arguments: { company: "Apple" },
          astHash: "hash123",
          resumeSessionId: "sess_recover"
        }),
      (error) => {
        assert.ok(error instanceof ClawExecutionError);
        assert.ok(error instanceof AgentExecutionError);
        assert.equal(error.sessionId, "sess_recover");
        assert.equal(error.status, "forbidden");
        return true;
      }
    );
  } finally {
    globalThis.fetch = originalFetch;
  }
});
