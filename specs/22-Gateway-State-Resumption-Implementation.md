# Phase 8 Implementation Spec: Gateway State Resumption & OS Kernel Updates

This specification details the structural TypeScript refactoring required within `openclaw-gateway/src` to securely implement the findings of GAN Audits 20 and 21.

---

## 1. Immutable Cache Replays (`traversal.ts` & `checkpoints.ts`)
**Goal:** Prevent non-deterministic loop drift and protect LLM executions that crashed mid-flight. Currently, `ExecutionState.frames.nextIndex` skips *completed* statements, but it does NOT protect in-flight nested LLM calls that crash mid-expression.

**Implementation Plan:**
1. **Node Hash ID**: Modify `type ExecutionFrame` to inherently track a unique `{statementPath}`.
2. **Read-Through Caching**: Modify `evaluateExpr` in `traversal.ts`. Before executing an `ExecuteRun` or `BinaryOp`, the executor MUST perform a `checkpoints.readHistoricalEvent(state.sessionId, statementPath)`.
3. **Cache Hit Bypass**: If the Database contains a completed payload for that exact `statementPath` in the `session_events` table, `evaluateExpr` immediately returns parsed `payload_json` and bypasses the AI completely.
4. **Database Additions**: Add `readHistoricalEvent(sessionId, nodePath)` to `SqliteCheckpointBackend` and `RedisCheckpointBackend`.

---

## 2. AST Hash Verification & Drain Mode (`server.ts` & `documents.ts`)
**Goal:** Prevent an old suspended workflow from resuming on a newly deployed `.claw` AST that has a mutated schema.

**Implementation Plan:**
1. **Strict Hash Validation**: Modify `handleWorkflowExecution` in `server.ts`. When `checkpoints.loadSession` returns an existing session, the Gateway MUST assert `existingSession.state.astHash === request.ast_hash`.
2. **Drain Mode Routing**: If the hashes do not match, the Gateway checks multiple `document_hash.json` files in memory. If the old hash exists on disk, it dynamically routes the resumed session to the *legacy* AST schema. If the old hash is deleted, it throws a `FatalASTDriftError`.

---

## 3. Zero-Trust Restoration (`checkpoints.ts` & `schema.ts`)
**Goal:** Prevent Redis Checkpoint Database poisoning. 

**Implementation Plan:**
1. **Re-Validation Interceptor**: Modify `loadSession(sessionId)` inside `CheckpointStore`. 
2. When parsing the recovered `ExecutionState` string from the SQL/Redis store, the Gateway must iterate over the `state.scopes` array. 
3. Every payload inside the scopes that maps back to an LLM JSON object MUST be piped through `validateAgainstSchema()` again using the loaded AST `document.json` schemas.
4. If validation fails, `loadSession` throws `CorruptedCheckpointError`.

---

## 4. Financial Circuit Breaker (`traversal.ts`)
**Goal:** Prevent 502/422 infinite retry loops against OpenAI.

**Implementation Plan:**
1. **State Tracking**: Add `retryCount: Record<string, number>` to `ExecutionState`.
2. **Guardrail Hook**: In `validateToolResult(result, schema)` inside `traversal.ts`, if `isSchemaDegraded` returns true, we increment `state.retryCount[statementPath]`.
3. **Circuit Break**: If `retryCount > 3`, we bypass the retry mechanisms and throw an uncatchable `FatalCircuitBreakerError("Max schema degradation retries exceeded")`, fatally halting the workflow.

---

## 5. Gateway Binary Resolution Waterfall (`engine/documents.ts`)
**Goal:** Locate the Rust compiler robustly for generating underlying AST definitions regardless of global vs. npm workspace installation.

**Implementation Plan:**
1. Create a `resolveCompilerBinary()` helper using `existsSync`:
```typescript
function resolveCompilerBinary(configPath?: string): string {
    if (configPath && existsSync(configPath)) return configPath;
    if (process.env.CLAW_BINARY_PATH && existsSync(process.env.CLAW_BINARY_PATH)) return process.env.CLAW_BINARY_PATH;
    
    // Check locally scoped NPM Wrapper
    const localNpm = path.join(process.cwd(), 'node_modules', '.bin', 'claw');
    if (existsSync(localNpm)) return localNpm;
    
    // Fallback to absolute system path
    return 'claw';
}
```
