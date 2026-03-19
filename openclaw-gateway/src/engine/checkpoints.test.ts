import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import test from "node:test";

import { CheckpointStore } from "./checkpoints.ts";
import type { ExecutionState } from "../types.ts";

class InMemoryRedisClient {
  values = new Map<string, string>();
  lists = new Map<string, string[]>();

  async connect(): Promise<void> {}

  async quit(): Promise<void> {}

  async set(key: string, value: string): Promise<string> {
    this.values.set(key, value);
    return "OK";
  }

  async get(key: string): Promise<string | null> {
    return this.values.get(key) ?? null;
  }

  async del(key: string): Promise<number> {
    const existed = this.values.delete(key);
    return existed ? 1 : 0;
  }

  async rPush(key: string, value: string): Promise<number> {
    const list = this.lists.get(key) ?? [];
    list.push(value);
    this.lists.set(key, list);
    return list.length;
  }
}

test("sqlite checkpoint backend roundtrips sessions and human overrides", async () => {
  const tempDir = mkdtempSync(join(tmpdir(), "claw-checkpoints-"));
  try {
    const store = new CheckpointStore({
      databasePath: join(tempDir, "engine.sqlite")
    });
    const state = createState("sqlite_session");

    await store.checkpoint(state, "workflow:Hello", "workflow_started", { attempt: 1 });
    const loaded = await store.loadSession("sqlite_session");
    assert.ok(loaded);
    assert.equal(loaded?.state.workflowName, "Hello");

    await store.saveHumanOverride("sqlite_session", { decision: "continue" });
    assert.deepEqual(await store.consumeHumanOverride("sqlite_session"), { decision: "continue" });
    assert.equal(await store.consumeHumanOverride("sqlite_session"), null);
    await store.close();
  } finally {
    rmSync(tempDir, { recursive: true, force: true });
  }
});

test("redis checkpoint backend roundtrips sessions and human overrides", async () => {
  const store = new CheckpointStore({
    databasePath: join(tmpdir(), "unused.sqlite"),
    redisClient: new InMemoryRedisClient(),
    redisNamespace: "claw:test"
  });
  const state = createState("redis_session");

  await store.checkpoint(state, "workflow:Hello", "workflow_started", { attempt: 1 });
  const loaded = await store.loadSession("redis_session");
  assert.ok(loaded);
  assert.equal(loaded?.state.workflowName, "Hello");

  await store.saveHumanOverride("redis_session", { decision: "continue" });
  assert.deepEqual(await store.consumeHumanOverride("redis_session"), { decision: "continue" });
  assert.equal(await store.consumeHumanOverride("redis_session"), null);
  await store.close();
});

test("cross-instance redis handoff: instance B resumes session checkpointed by instance A", async () => {
  // Shared Redis backend simulates two gateway containers in a cluster
  const sharedRedis = new InMemoryRedisClient();

  const instanceA = new CheckpointStore({
    databasePath: join(tmpdir(), "unused-a.sqlite"),
    redisClient: sharedRedis,
    redisNamespace: "claw:cluster"
  });

  const instanceB = new CheckpointStore({
    databasePath: join(tmpdir(), "unused-b.sqlite"),
    redisClient: sharedRedis,
    redisNamespace: "claw:cluster"
  });

  // Instance A starts the workflow and checkpoints mid-execution
  const state = createState("distributed_sess");
  state.status = "running";
  state.scopes = [{ company: "Acme" }];
  state.frames = [{
    kind: "block",
    blockPath: "workflow:Analyze/body",
    nextIndex: 1,
    createdScope: false
  }];
  await instanceA.checkpoint(state, "workflow:Analyze/body/statements/0", "let_decl", {});

  // Simulate Instance A dying — close its checkpoint store
  await instanceA.close();

  // Instance B picks up the session from shared Redis
  const resumed = await instanceB.loadSession("distributed_sess");
  assert.ok(resumed, "Instance B must find the session in shared Redis");
  assert.equal(resumed!.state.workflowName, "Hello");
  assert.equal(resumed!.state.sessionId, "distributed_sess");
  assert.equal(resumed!.state.status, "running");
  assert.deepEqual(resumed!.state.scopes, [{ company: "Acme" }]);

  // Instance B advances to completion
  const completedState = resumed!.state;
  completedState.status = "completed";
  completedState.returnValue = { result: "done" };
  completedState.frames = [];
  await instanceB.checkpoint(
    completedState,
    "workflow:Analyze",
    "workflow_completed",
    completedState.returnValue
  );

  // Verify final state is readable by any instance
  const final_ = await instanceB.loadSession("distributed_sess");
  assert.equal(final_!.state.status, "completed");
  assert.deepEqual(final_!.result, { result: "done" });

  await instanceB.close();
});

test("cross-instance redis human override: instance A saves override, instance B consumes it", async () => {
  const sharedRedis = new InMemoryRedisClient();

  const instanceA = new CheckpointStore({
    databasePath: join(tmpdir(), "unused-a2.sqlite"),
    redisClient: sharedRedis,
    redisNamespace: "claw:override-test"
  });

  const instanceB = new CheckpointStore({
    databasePath: join(tmpdir(), "unused-b2.sqlite"),
    redisClient: sharedRedis,
    redisNamespace: "claw:override-test"
  });

  // Instance A saves a human override
  await instanceA.saveHumanOverride("sess_cross", {
    result: { url: "https://manual.example", text: "Human provided context" }
  });
  await instanceA.close();

  // Instance B consumes the override
  const override = await instanceB.consumeHumanOverride("sess_cross");
  assert.deepEqual(override, {
    result: { url: "https://manual.example", text: "Human provided context" }
  });

  // Override is consumed — second read returns null
  const second = await instanceB.consumeHumanOverride("sess_cross");
  assert.equal(second, null);

  await instanceB.close();
});

function createState(sessionId: string): ExecutionState {
  return {
    sessionId,
    astHash: "hash123",
    workflowName: "Hello",
    scopes: [{ name: "Alice" }],
    frames: [],
    returnValue: null,
    status: "running"
  };
}
