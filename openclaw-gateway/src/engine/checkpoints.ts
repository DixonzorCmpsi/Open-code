import { mkdirSync } from "node:fs";
import { dirname } from "node:path";
import { DatabaseSync } from "node:sqlite";

import type { ExecutionState, HumanInterventionEvent } from "../types.ts";

interface SessionRow {
  session_id: string;
  ast_hash: string;
  workflow_name: string;
  status: string;
  state_json: string;
  result_json: string | null;
}

interface SessionRecord extends SessionRow {}

interface RedisLikeClient {
  connect?(): Promise<void>;
  quit?(): Promise<void>;
  set(key: string, value: string): Promise<unknown>;
  get(key: string): Promise<string | null>;
  del(key: string): Promise<number>;
  rPush(key: string, value: string): Promise<number>;
  lRange?(key: string, start: number, stop: number): Promise<string[]>;
}

interface CheckpointStoreOptions {
  databasePath: string;
  redisUrl?: string | null;
  redisNamespace?: string;
  redisClient?: RedisLikeClient;
}

interface CheckpointBackend {
  checkpoint(
    state: ExecutionState,
    nodePath: string,
    eventType: string,
    payload: unknown
  ): Promise<void>;
  loadSession(sessionId: string): Promise<{ state: ExecutionState; result: unknown | null } | null>;
  loadHistoricalEvents(sessionId: string): Promise<Map<string, unknown>>;
  saveAstDocument(astHash: string, documentJson: string): Promise<void>;
  loadAstDocument(astHash: string): Promise<string | null>;
  saveHumanOverride(sessionId: string, payload: unknown): Promise<void>;
  consumeHumanOverride(sessionId: string): Promise<unknown | null>;
  close(): Promise<void>;
}

export class CheckpointStore {
  backend: CheckpointBackend;

  constructor(options: string | CheckpointStoreOptions) {
    const resolvedOptions =
      typeof options === "string" ? { databasePath: options } : options;

    this.backend =
      resolvedOptions.redisClient || resolvedOptions.redisUrl
        ? new RedisCheckpointBackend(
            resolvedOptions.redisClient ??
              createRedisClient(resolvedOptions.redisUrl!),
            resolvedOptions.redisNamespace ?? "claw"
          )
        : new SqliteCheckpointBackend(resolvedOptions.databasePath);
  }

  async checkpoint(
    state: ExecutionState,
    nodePath: string,
    eventType: string,
    payload: unknown = {}
  ): Promise<void> {
    await this.backend.checkpoint(state, nodePath, eventType, payload);
  }

  async loadSession(sessionId: string): Promise<{ state: ExecutionState; result: unknown | null } | null> {
    return this.backend.loadSession(sessionId);
  }

  async loadHistoricalEvents(sessionId: string): Promise<Map<string, unknown>> {
    return this.backend.loadHistoricalEvents(sessionId);
  }

  async saveAstDocument(astHash: string, documentJson: string): Promise<void> {
    await this.backend.saveAstDocument(astHash, documentJson);
  }

  async loadAstDocument(astHash: string): Promise<string | null> {
    return this.backend.loadAstDocument(astHash);
  }

  async saveHumanOverride(sessionId: string, payload: unknown): Promise<void> {
    await this.backend.saveHumanOverride(sessionId, payload);
  }

  async consumeHumanOverride(sessionId: string): Promise<unknown | null> {
    return this.backend.consumeHumanOverride(sessionId);
  }

  async emitHumanIntervention(state: ExecutionState, event: HumanInterventionEvent): Promise<void> {
    const nodePath =
      typeof event.metadata.node_path === "string" ? event.metadata.node_path : event.session_id;
    await this.checkpoint(state, nodePath, "human_intervention_required", event);
  }

  async close(): Promise<void> {
    await this.backend.close();
  }
}

class SqliteCheckpointBackend implements CheckpointBackend {
  database: DatabaseSync;

  constructor(databasePath: string) {
    mkdirSync(dirname(databasePath), { recursive: true });
    this.database = new DatabaseSync(databasePath);
    this.database.exec(`
      CREATE TABLE IF NOT EXISTS sessions (
        session_id TEXT PRIMARY KEY,
        ast_hash TEXT NOT NULL,
        workflow_name TEXT NOT NULL,
        status TEXT NOT NULL,
        state_json TEXT NOT NULL,
        result_json TEXT,
        updated_at TEXT NOT NULL
      );

      CREATE TABLE IF NOT EXISTS session_events (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        session_id TEXT NOT NULL,
        event_type TEXT NOT NULL,
        node_path TEXT NOT NULL,
        status TEXT NOT NULL,
        payload_json TEXT NOT NULL,
        created_at TEXT NOT NULL
      );

      CREATE TABLE IF NOT EXISTS human_overrides (
        session_id TEXT PRIMARY KEY,
        payload_json TEXT NOT NULL,
        updated_at TEXT NOT NULL
      );

      CREATE TABLE IF NOT EXISTS ast_registry (
        ast_hash TEXT PRIMARY KEY,
        document_json TEXT NOT NULL,
        created_at TEXT NOT NULL
      );
    `);
  }

  async checkpoint(
    state: ExecutionState,
    nodePath: string,
    eventType: string,
    payload: unknown = {}
  ): Promise<void> {
    const timestamp = new Date().toISOString();
    const resultJson = state.returnValue === null ? null : JSON.stringify(state.returnValue);
    this.database
      .prepare(
        `
          INSERT INTO sessions (session_id, ast_hash, workflow_name, status, state_json, result_json, updated_at)
          VALUES (?, ?, ?, ?, ?, ?, ?)
          ON CONFLICT(session_id) DO UPDATE SET
            ast_hash = excluded.ast_hash,
            workflow_name = excluded.workflow_name,
            status = excluded.status,
            state_json = excluded.state_json,
            result_json = excluded.result_json,
            updated_at = excluded.updated_at
        `
      )
      .run(
        state.sessionId,
        state.astHash,
        state.workflowName,
        state.status,
        JSON.stringify(state),
        resultJson,
        timestamp
      );

    this.database
      .prepare(
        `
          INSERT INTO session_events (session_id, event_type, node_path, status, payload_json, created_at)
          VALUES (?, ?, ?, ?, ?, ?)
        `
      )
      .run(
        state.sessionId,
        eventType,
        nodePath,
        state.status,
        JSON.stringify(payload),
        timestamp
      );
  }

  async loadSession(sessionId: string): Promise<{ state: ExecutionState; result: unknown | null } | null> {
    const row = this.database
      .prepare(
        `
          SELECT session_id, ast_hash, workflow_name, status, state_json, result_json
          FROM sessions
          WHERE session_id = ?
        `
      )
      .get(sessionId) as SessionRow | undefined;

    if (!row) {
      return null;
    }

    return {
      state: JSON.parse(row.state_json) as ExecutionState,
      result: row.result_json ? JSON.parse(row.result_json) : null
    };
  }

  async loadHistoricalEvents(sessionId: string): Promise<Map<string, unknown>> {
    const rows = this.database
      .prepare(`SELECT node_path, payload_json FROM session_events WHERE session_id = ? AND event_type != 'human_intervention_required'`)
      .all(sessionId) as Array<{ node_path: string; payload_json: string }>;
    
    const eventsMap = new Map<string, unknown>();
    for (const row of rows) {
      eventsMap.set(row.node_path, JSON.parse(row.payload_json));
    }
    return eventsMap;
  }

  async saveAstDocument(astHash: string, documentJson: string): Promise<void> {
    this.database
      .prepare(
        `
          INSERT INTO ast_registry (ast_hash, document_json, created_at)
          VALUES (?, ?, ?)
          ON CONFLICT(ast_hash) DO NOTHING
        `
      )
      .run(astHash, documentJson, new Date().toISOString());
  }

  async loadAstDocument(astHash: string): Promise<string | null> {
    const row = this.database
      .prepare(`SELECT document_json FROM ast_registry WHERE ast_hash = ?`)
      .get(astHash) as { document_json: string } | undefined;
    return row ? row.document_json : null;
  }

  async saveHumanOverride(sessionId: string, payload: unknown): Promise<void> {
    this.database
      .prepare(
        `
          INSERT INTO human_overrides (session_id, payload_json, updated_at)
          VALUES (?, ?, ?)
          ON CONFLICT(session_id) DO UPDATE SET
            payload_json = excluded.payload_json,
            updated_at = excluded.updated_at
        `
      )
      .run(sessionId, JSON.stringify(payload), new Date().toISOString());
  }

  async consumeHumanOverride(sessionId: string): Promise<unknown | null> {
    const row = this.database
      .prepare(`SELECT payload_json FROM human_overrides WHERE session_id = ?`)
      .get(sessionId) as { payload_json: string } | undefined;
    if (!row) {
      return null;
    }

    this.database.prepare(`DELETE FROM human_overrides WHERE session_id = ?`).run(sessionId);
    return JSON.parse(row.payload_json);
  }

  async close(): Promise<void> {
    this.database.close();
  }
}

class RedisCheckpointBackend implements CheckpointBackend {
  clientPromise: Promise<RedisLikeClient>;
  namespace: string;

  constructor(clientOrPromise: RedisLikeClient | Promise<RedisLikeClient>, namespace: string) {
    this.clientPromise = Promise.resolve(clientOrPromise);
    this.namespace = namespace;
  }

  async checkpoint(
    state: ExecutionState,
    nodePath: string,
    eventType: string,
    payload: unknown = {}
  ): Promise<void> {
    const client = await this.clientPromise;
    const timestamp = new Date().toISOString();
    const sessionRecord: SessionRecord = {
      session_id: state.sessionId,
      ast_hash: state.astHash,
      workflow_name: state.workflowName,
      status: state.status,
      state_json: JSON.stringify(state),
      result_json: state.returnValue === null ? null : JSON.stringify(state.returnValue)
    };

    await client.set(this.sessionKey(state.sessionId), JSON.stringify(sessionRecord));
    await client.rPush(
      this.eventsKey(state.sessionId),
      JSON.stringify({
        session_id: state.sessionId,
        event_type: eventType,
        node_path: nodePath,
        status: state.status,
        payload_json: JSON.stringify(payload),
        created_at: timestamp
      })
    );
  }

  async loadSession(sessionId: string): Promise<{ state: ExecutionState; result: unknown | null } | null> {
    const client = await this.clientPromise;
    const rawRecord = await client.get(this.sessionKey(sessionId));
    if (!rawRecord) {
      return null;
    }

    const record = JSON.parse(rawRecord) as SessionRecord;
    return {
      state: JSON.parse(record.state_json) as ExecutionState,
      result: record.result_json ? JSON.parse(record.result_json) : null
    };
  }

  async loadHistoricalEvents(sessionId: string): Promise<Map<string, unknown>> {
    const client = await this.clientPromise;
    const events = (client.lRange ? await client.lRange(this.eventsKey(sessionId), 0, -1) : []) as string[];
    const eventsMap = new Map<string, unknown>();
    for (const eventStr of events) {
      const event = JSON.parse(eventStr) as { node_path: string; payload_json: string; event_type: string };
      if (event.event_type !== "human_intervention_required") {
        eventsMap.set(event.node_path, JSON.parse(event.payload_json));
      }
    }
    return eventsMap;
  }

  async saveAstDocument(astHash: string, documentJson: string): Promise<void> {
    const client = await this.clientPromise;
    await client.set(this.astKey(astHash), documentJson);
  }

  async loadAstDocument(astHash: string): Promise<string | null> {
    const client = await this.clientPromise;
    return client.get(this.astKey(astHash));
  }

  async saveHumanOverride(sessionId: string, payload: unknown): Promise<void> {
    const client = await this.clientPromise;
    await client.set(this.overrideKey(sessionId), JSON.stringify(payload));
  }

  async consumeHumanOverride(sessionId: string): Promise<unknown | null> {
    const client = await this.clientPromise;
    const key = this.overrideKey(sessionId);
    const payload = await client.get(key);
    if (!payload) {
      return null;
    }

    await client.del(key);
    return JSON.parse(payload);
  }

  async close(): Promise<void> {
    const client = await this.clientPromise;
    await client.quit?.();
  }

  private sessionKey(sessionId: string): string {
    return `${this.namespace}:session:${sessionId}`;
  }

  private eventsKey(sessionId: string): string {
    return `${this.namespace}:events:${sessionId}`;
  }

  private overrideKey(sessionId: string): string {
    return `${this.namespace}:override:${sessionId}`;
  }

  private astKey(astHash: string): string {
    return `${this.namespace}:ast:${astHash}`;
  }
}

async function createRedisClient(redisUrl: string): Promise<RedisLikeClient> {
  const { createClient } = await import("redis");
  const client = createClient({ url: redisUrl });
  client.on("error", (error) => {
    console.error("[claw-gateway] redis client error", error);
  });
  await client.connect();
  return client as RedisLikeClient;
}
