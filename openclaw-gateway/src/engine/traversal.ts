import { randomUUID } from "node:crypto";
import { search, navigate } from "../tools/browser.ts";
import type {
  AgentDecl,
  ClientDecl,
  CompiledDocumentFile,
  DataType,
  ExecutionFrame,
  ExecutionRequest,
  ExecutionState,
  Expr,
  SpannedExpr,
  Statement,
  ToolDecl
} from "../types.ts";
import { CheckpointStore } from "./checkpoints.ts";
import { findAgent, findClient, findTool, findType, findWorkflow, getVariant, resolveBlock, unwrapExpr } from "./ast.ts";
import { buildReturnSchema, generateStructuredResult } from "./llm.ts";
import { bamlFunctionName, callBamlFunction, loadBamlClient } from "../baml-bridge.ts";
import { AssertionError, HumanInterventionRequiredError, SchemaDegradationError } from "./errors.ts";
import { executeCustomTool } from "./runtime.ts";
import { isSchemaDegraded, validateAgainstSchema } from "./schema.ts";

interface TraversalOptions {
  compiled: CompiledDocumentFile;
  request: ExecutionRequest;
  checkpoints: CheckpointStore;
  workspaceRoot?: string;
}

export async function executeWorkflow(options: TraversalOptions): Promise<unknown> {
  const { compiled, request, checkpoints } = options;
  const existingSession = await checkpoints.loadSession(request.session_id);
  const state = existingSession?.state && existingSession.state.status !== "completed"
    ? existingSession.state
    : createInitialState(compiled, request);

  if (existingSession?.state.status === "completed") {
    return existingSession.result;
  }

  if (!existingSession) {
    await checkpoints.checkpoint(state, `workflow:${state.workflowName}`, "workflow_started", {
      arguments: request.arguments
    });
  }

  state.status = "running";

  try {
    while (state.frames.length > 0 && state.returnValue === null) {
      try {
        const frame = state.frames[state.frames.length - 1]!;
        if (frame.kind === "loop") {
          advanceLoopFrame(state, frame);
          await checkpoints.checkpoint(state, frame.statementPath, "loop_tick");
          continue;
        }

        if (frame.kind === "try_catch") {
          state.frames.pop();
          continue;
        }

        const block = resolveBlock(compiled.document, frame.blockPath);
        if (frame.nextIndex >= block.statements.length) {
          popFrame(state);
          continue;
        }

        const statementPath = `${frame.blockPath}/statements/${frame.nextIndex}`;
        const statement = block.statements[frame.nextIndex]!;
        await executeStatement(
          compiled,
          state,
          statement,
          statementPath,
          options.workspaceRoot ?? process.cwd(),
          checkpoints
        );
      } catch (error) {
        const caught = await handleFrameError(state, error, checkpoints);
        if (!caught) {
          throw error;
        }
      }
    }

    state.status = "completed";
    await checkpoints.checkpoint(
      state,
      `workflow:${state.workflowName}`,
      "workflow_completed",
      state.returnValue
    );
    return state.returnValue;
  } catch (error) {
    if (error instanceof HumanInterventionRequiredError) {
      state.status = "waiting_human";
      await checkpoints.emitHumanIntervention(state, error.event);
    }
    throw error;
  }
}

function createInitialState(compiled: CompiledDocumentFile, request: ExecutionRequest): ExecutionState {
  const workflow = findWorkflow(compiled.document, request.workflow);
  const scope = Object.fromEntries(workflow.arguments.map((argument) => [argument.name, request.arguments[argument.name]]));
  return {
    sessionId: request.session_id,
    astHash: request.ast_hash,
    workflowName: workflow.name,
    scopes: [scope],
    frames: [
      {
        kind: "block",
        blockPath: `workflow:${workflow.name}/body`,
        nextIndex: 0,
        createdScope: false
      }
    ],
    returnValue: null,
    status: "running"
  };
}

function advanceLoopFrame(state: ExecutionState, frame: Extract<ExecutionFrame, { kind: "loop" }>): void {
  if (frame.index >= frame.items.length) {
    state.frames.pop();
    return;
  }

  const item = frame.items[frame.index];
  frame.index += 1;
  state.scopes.push({ [frame.itemName]: item });
  state.frames.push({
    kind: "block",
    blockPath: frame.bodyPath,
    nextIndex: 0,
    createdScope: true
  });
}

async function executeStatement(
  compiled: CompiledDocumentFile,
  state: ExecutionState,
  statement: Statement,
  statementPath: string,
  workspaceRoot: string,
  checkpoints: CheckpointStore
): Promise<void> {
  const frame = state.frames[state.frames.length - 1] as Extract<ExecutionFrame, { kind: "block" }>;
  const [kind, payload] = getVariant(statement);

  switch (kind) {
    case "LetDecl": {
      const value = await evaluateExpr(compiled, state, (payload as { value: Expr }).value, statementPath, workspaceRoot, checkpoints);
      assignVariable(state, (payload as { name: string }).name, value);
      frame.nextIndex += 1;
      await checkpoints.checkpoint(state, statementPath, "let_decl", value);
      return;
    }
    case "ForLoop": {
      const loopPayload = payload as {
        item_name: string;
        iterator_name?: string;
        iterator?: Expr | SpannedExpr;
        body: unknown;
      };
      const values = loopPayload.iterator
        ? await evaluateExpr(compiled, state, loopPayload.iterator, statementPath, workspaceRoot, checkpoints)
        : resolveVariable(state, loopPayload.iterator_name!);
      if (!Array.isArray(values)) {
        throw new Error(`ForLoop iterator ${loopPayload.iterator_name ?? "<expr>"} must be an array`);
      }
      frame.nextIndex += 1;
      state.frames.push({
        kind: "loop",
        statementPath,
        itemName: loopPayload.item_name,
        items: values,
        index: 0,
        bodyPath: `${statementPath}/body`
      });
      await checkpoints.checkpoint(state, statementPath, "for_loop");
      return;
    }
    case "IfCond": {
      const ifPayload = payload as { condition: Expr | SpannedExpr; else_body: unknown };
      const condition = await evaluateExpr(compiled, state, ifPayload.condition, statementPath, workspaceRoot, checkpoints);
      frame.nextIndex += 1;
      const branch = condition ? "if_body" : ifPayload.else_body ? "else_body" : null;
      if (branch) {
        state.scopes.push({});
        state.frames.push({
          kind: "block",
          blockPath: `${statementPath}/${branch}`,
          nextIndex: 0,
          createdScope: true
        });
      }
      await checkpoints.checkpoint(state, statementPath, "if_cond", { branch });
      return;
    }
    case "TryCatch": {
      const tryCatchPayload = payload as {
        catch_name: string;
      };
      frame.nextIndex += 1;
      state.frames.push({
        kind: "try_catch",
        statementPath,
        catchName: tryCatchPayload.catch_name,
        catchBodyPath: `${statementPath}/catch_body`
      });
      state.scopes.push({});
      state.frames.push({
        kind: "block",
        blockPath: `${statementPath}/try_body`,
        nextIndex: 0,
        createdScope: true
      });
      await checkpoints.checkpoint(state, statementPath, "try_catch_enter");
      return;
    }
    case "ExecuteRun": {
      await executeAgentRun(compiled, state, payload as StatementExecuteRun, statementPath, workspaceRoot, checkpoints);
      frame.nextIndex += 1;
      await checkpoints.checkpoint(state, statementPath, "execute_run");
      return;
    }
    case "Return": {
      state.returnValue = await evaluateExpr(
        compiled,
        state,
        (payload as { value: Expr }).value,
        statementPath,
        workspaceRoot,
        checkpoints
      );
      state.status = "completed";
      frame.nextIndex += 1;
      await checkpoints.checkpoint(state, statementPath, "return", state.returnValue);
      return;
    }
    case "Expression": {
      const expr = Array.isArray(payload) ? (payload as [Expr, unknown])[0] : (payload as Expr | SpannedExpr);
      await evaluateExpr(compiled, state, expr, statementPath, workspaceRoot, checkpoints);
      frame.nextIndex += 1;
      await checkpoints.checkpoint(state, statementPath, "expression");
      return;
    }
    case "Continue": {
      while (state.frames.length > 0) {
        const top = state.frames[state.frames.length - 1]!;
        if (top.kind === "loop") {
          break;
        }
        popFrame(state);
      }
      await checkpoints.checkpoint(state, statementPath, "continue");
      return;
    }
    case "Break": {
      while (state.frames.length > 0) {
        const top = popFrame(state);
        if (top?.kind === "loop") {
          break;
        }
      }
      await checkpoints.checkpoint(state, statementPath, "break");
      return;
    }
    case "Assert": {
      const assertPayload = payload as { condition: Expr | SpannedExpr; message: string | null };
      const condition = await evaluateExpr(
        compiled,
        state,
        assertPayload.condition,
        statementPath,
        workspaceRoot,
        checkpoints
      );
      if (!condition) {
        throw new AssertionError(
          assertPayload.message ?? `Assertion failed at ${statementPath}`,
          statementPath
        );
      }
      frame.nextIndex += 1;
      await checkpoints.checkpoint(state, statementPath, "assert_pass");
      return;
    }
    default:
      throw new Error(`Unsupported statement kind ${kind}`);
  }
}

type StatementExecuteRun = {
  agent_name: string;
  kwargs: Array<[string, Expr | SpannedExpr]>;
  require_type: DataType | null;
};

async function evaluateExpr(
  compiled: CompiledDocumentFile,
  state: ExecutionState,
  expr: Expr | SpannedExpr,
  statementPath: string,
  workspaceRoot: string,
  checkpoints: CheckpointStore
): Promise<unknown> {
  const [kind, payload] = getVariant(expr);

  switch (kind) {
    case "StringLiteral":
    case "IntLiteral":
    case "FloatLiteral":
    case "BoolLiteral":
      return payload;
    case "Identifier":
      return resolveVariable(state, payload as string);
    case "ArrayLiteral":
      return Promise.all(
        (payload as Array<Expr | SpannedExpr>).map((item) =>
          evaluateExpr(compiled, state, item, statementPath, workspaceRoot, checkpoints)
        )
      );
    case "MemberAccess": {
      const [targetExpr, propertyName] = payload as [Expr | SpannedExpr, string];
      const target = await evaluateExpr(compiled, state, targetExpr, statementPath, workspaceRoot, checkpoints);
      if (typeof target === "object" && target !== null && propertyName in target) {
        return (target as Record<string, unknown>)[propertyName];
      }
      throw new Error(`Unsupported member access ${propertyName}`);
    }
    case "ExecuteRun":
      return executeAgentRun(compiled, state, payload as StatementExecuteRun, statementPath, workspaceRoot, checkpoints);
    case "BinaryOp": {
      const left = await evaluateExpr(compiled, state, (payload as { left: Expr | SpannedExpr }).left, statementPath, workspaceRoot, checkpoints);
      const right = await evaluateExpr(compiled, state, (payload as { right: Expr | SpannedExpr }).right, statementPath, workspaceRoot, checkpoints);
      const result = JSON.stringify(left) === JSON.stringify(right);
      await checkpoints.checkpoint(state, statementPath, "binary_op", { result });
      return result;
    }
    case "MethodCall": {
      const methodResult = await applyMethodCall(
        compiled,
        state,
        payload as [Expr | SpannedExpr, string, Array<Expr | SpannedExpr>],
        statementPath,
        workspaceRoot,
        checkpoints
      );
      await checkpoints.checkpoint(state, statementPath, "method_call", { result: methodResult });
      return methodResult;
    }
    case "Call": {
      const [workflowName, argExprs] = payload as [string, Array<Expr | SpannedExpr>];
      if (workflowName === "env") {
        if (argExprs.length !== 1) {
          throw new Error("env() expects exactly one argument");
        }
        const envName = await evaluateExpr(
          compiled,
          state,
          argExprs[0]!,
          statementPath,
          workspaceRoot,
          checkpoints
        );
        if (typeof envName !== "string") {
          throw new Error("env() expects a string argument");
        }
        const envValue = process.env[envName];
        if (envValue === undefined) {
          throw new Error(`Environment variable ${envName} is not set`);
        }
        return envValue;
      }
      const targetWorkflow = findWorkflow(compiled.document, workflowName);
      const evaluatedArgs = await Promise.all(
        argExprs.map((argExpr) => evaluateExpr(compiled, state, argExpr, statementPath, workspaceRoot, checkpoints))
      );
      const callArgs: Record<string, unknown> = {};
      for (let i = 0; i < targetWorkflow.arguments.length; i++) {
        callArgs[targetWorkflow.arguments[i].name] = evaluatedArgs[i];
      }
      const nestedResult = await executeWorkflow({
        compiled: { ast_hash: state.astHash, document: compiled.document },
        request: {
          workflow: workflowName,
          arguments: callArgs,
          ast_hash: state.astHash,
          session_id: `${state.sessionId}:${workflowName}:${randomUUID()}`
        },
        checkpoints,
        workspaceRoot
      });
      return nestedResult;
    }
    default:
      throw new Error(`Unsupported expression kind ${kind}`);
  }
}

async function applyMethodCall(
  compiled: CompiledDocumentFile,
  state: ExecutionState,
  payload: [Expr | SpannedExpr, string, Array<Expr | SpannedExpr>],
  statementPath: string,
  workspaceRoot: string,
  checkpoints: CheckpointStore
): Promise<unknown> {
  const [targetExpr, methodName, args] = payload;
  const target = await evaluateExpr(compiled, state, targetExpr, statementPath, workspaceRoot, checkpoints);

  if (methodName === "length" && Array.isArray(target)) {
    return target.length;
  }

  const rawTargetExpr = unwrapExpr<Expr>(targetExpr);
  if (methodName === "append" && "Identifier" in rawTargetExpr && Array.isArray(target)) {
    const [nextValue] = await Promise.all(
      args.map((arg) => evaluateExpr(compiled, state, arg, statementPath, workspaceRoot, checkpoints))
    );
    target.push(nextValue);
    assignVariable(state, rawTargetExpr.Identifier, target);
    return target;
  }

  if (methodName === "reply") {
    return { replied: true, value: target };
  }

  throw new Error(`Unsupported method call ${methodName}`);
}

async function executeAgentRun(
  compiled: CompiledDocumentFile,
  state: ExecutionState,
  executeRun: StatementExecuteRun,
  statementPath: string,
  workspaceRoot: string,
  checkpoints: CheckpointStore
): Promise<unknown> {
  const agent = findAgent(compiled.document, executeRun.agent_name);
  const client = await resolveClientConfig(
    compiled,
    state,
    findClient(compiled.document, agent.client),
    statementPath,
    workspaceRoot,
    checkpoints
  );
  const kwargs = Object.fromEntries(
    await Promise.all(
      executeRun.kwargs.map(async ([key, value]) => [
        key,
        await evaluateExpr(compiled, state, value, statementPath, workspaceRoot, checkpoints)
      ])
    )
  ) as Record<string, unknown>;

  const tools = agent.tools
    .map((toolName) => findTool(compiled.document, toolName))
    .filter((tool): tool is ToolDecl => tool !== null);

  const requireTypeName = extractCustomTypeName(executeRun.require_type);
  const returnType = requireTypeName ? findType(compiled.document, requireTypeName) : null;
  const returnSchema = requireTypeName ? buildReturnSchema(compiled.document, requireTypeName) : null;
  const mockResult = await resolveMockResult(
    compiled,
    state,
    executeRun.agent_name,
    statementPath,
    workspaceRoot,
    checkpoints
  );
  if (mockResult !== undefined) {
    return validateToolResult(mockResult, returnSchema);
  }
  if (agent.tools.some((toolName) => toolName === "Browser.search") && typeof kwargs.query === "string") {
    return validateToolResult(
      await search(String(kwargs.query), {
        sessionId: state.sessionId,
        nodePath: statementPath,
        consumeOverride: () => checkpoints.consumeHumanOverride(state.sessionId)
      }),
      returnSchema
    );
  }

  if (agent.tools.some((toolName) => toolName === "Browser.navigate") && typeof kwargs.url === "string") {
    return validateToolResult(
      await navigate(String(kwargs.url), {
        sessionId: state.sessionId,
        nodePath: statementPath,
        consumeOverride: () => checkpoints.consumeHumanOverride(state.sessionId)
      }),
      returnSchema
    );
  }

  if (tools.length === 1 && tools[0].invoke_path) {
    return validateToolResult(
      await executeCustomTool(tools[0].invoke_path, kwargs, workspaceRoot),
      returnSchema
    );
  }

  if (!returnSchema) {
    return null;
  }

  // Priority 3: BAML (if available and agent has no tools) — spec 18 §5.1
  if (agent.tools.length === 0) {
    await loadBamlClient(workspaceRoot);
    const fnName = bamlFunctionName(agent.name, requireTypeName);
    const bamlPromise = callBamlFunction(fnName, kwargs);
    if (bamlPromise !== null) {
      const bamlResult = await bamlPromise;
      return validateToolResult(bamlResult, returnSchema);
    }
  }

  // Priority 4: Raw HTTP fallback
  return validateToolResult(await generateStructuredResult({
    document: compiled.document,
    agent,
    client,
    returnType,
    returnSchema,
    kwargs,
    tools
  }), returnSchema);
}

function extractCustomTypeName(dataType: DataType | null): string | null {
  if (!dataType) {
    return null;
  }
  const [kind, payload] = getVariant(dataType);
  if (kind === "Custom") {
    return (payload as [string, unknown])[0];
  }
  return null;
}

function resolveVariable(state: ExecutionState, name: string): unknown {
  for (let index = state.scopes.length - 1; index >= 0; index -= 1) {
    const scope = state.scopes[index]!;
    if (name in scope) {
      return scope[name];
    }
  }
  throw new Error(`Unknown variable ${name}`);
}

function assignVariable(state: ExecutionState, name: string, value: unknown): void {
  for (let index = state.scopes.length - 1; index >= 0; index -= 1) {
    if (name in state.scopes[index]!) {
      state.scopes[index]![name] = value;
      return;
    }
  }
  state.scopes[state.scopes.length - 1]![name] = value;
}

async function resolveClientConfig(
  compiled: CompiledDocumentFile,
  state: ExecutionState,
  client: ClientDecl | null,
  statementPath: string,
  workspaceRoot: string,
  checkpoints: CheckpointStore
): Promise<ClientDecl | null> {
  if (!client) {
    return null;
  }

  const endpoint = await resolveClientField(
    compiled,
    state,
    client,
    client.endpoint,
    "endpoint",
    statementPath,
    workspaceRoot,
    checkpoints
  );
  const apiKey = await resolveClientField(
    compiled,
    state,
    client,
    client.api_key,
    "api_key",
    statementPath,
    workspaceRoot,
    checkpoints
  );

  return {
    ...client,
    endpoint: endpoint === null ? null : {
      expr: { StringLiteral: endpoint },
      span: client.endpoint?.span ?? { start: 0, end: 0 }
    },
    api_key: apiKey === null ? null : {
      expr: { StringLiteral: apiKey },
      span: client.api_key?.span ?? { start: 0, end: 0 }
    }
  };
}

async function resolveClientField(
  compiled: CompiledDocumentFile,
  state: ExecutionState,
  client: ClientDecl,
  expression: Expr | SpannedExpr | null,
  fieldName: string,
  statementPath: string,
  workspaceRoot: string,
  checkpoints: CheckpointStore
): Promise<string | null> {
  if (!expression) {
    return null;
  }

  const rawExpression = unwrapExpr<Expr>(expression);
  if ("Call" in rawExpression && rawExpression.Call[0] === "env") {
    const [, args] = rawExpression.Call;
    if (args.length !== 1) {
      throw new Error(`Client ${client.name} ${fieldName} env() call must have exactly one argument`);
    }
    const envName = await evaluateExpr(
      compiled,
      state,
      args[0]!,
      statementPath,
      workspaceRoot,
      checkpoints
    );
    if (typeof envName !== "string") {
      throw new Error(`Client ${client.name} ${fieldName} env() argument must resolve to a string`);
    }
    const envValue = process.env[envName];
    if (envValue === undefined) {
      throw new Error(`Environment variable ${envName} is not set (required by client ${client.name})`);
    }
    return envValue;
  }

  const value = await evaluateExpr(compiled, state, expression, statementPath, workspaceRoot, checkpoints);
  if (typeof value !== "string") {
    throw new Error(`Client ${client.name} ${fieldName} must resolve to a string`);
  }
  return value;
}

async function resolveMockResult(
  compiled: CompiledDocumentFile,
  state: ExecutionState,
  agentName: string,
  statementPath: string,
  workspaceRoot: string,
  checkpoints: CheckpointStore
): Promise<unknown | undefined> {
  const mock = [...compiled.document.mocks]
    .reverse()
    .find((candidate) => candidate.target_agent === agentName);
  if (!mock) {
    return undefined;
  }

  const entries = await Promise.all(
    mock.output.map(async ([key, value]) => [
      key,
      await evaluateExpr(compiled, state, value, statementPath, workspaceRoot, checkpoints)
    ])
  );
  return Object.fromEntries(entries);
}

function popFrame(state: ExecutionState): ExecutionFrame | undefined {
  const frame = state.frames.pop();
  if (frame?.kind === "block" && frame.createdScope) {
    state.scopes.pop();
  }
  return frame;
}

async function handleFrameError(
  state: ExecutionState,
  error: unknown,
  checkpoints: CheckpointStore
): Promise<boolean> {
  const tryCatchIndex = findNearestTryCatchFrame(state.frames);
  if (tryCatchIndex === -1) {
    return false;
  }

  while (state.frames.length > tryCatchIndex) {
    const top = popFrame(state);
    if (top?.kind === "try_catch") {
      const boundError = error instanceof Error ? error : new Error(String(error));
      state.scopes.push({ [top.catchName]: boundError });
      state.frames.push({
        kind: "block",
        blockPath: top.catchBodyPath,
        nextIndex: 0,
        createdScope: true
      });
      await checkpoints.checkpoint(state, top.statementPath, "try_catch_caught", {
        message: boundError.message
      });
      return true;
    }
  }

  return false;
}

function findNearestTryCatchFrame(frames: ExecutionFrame[]): number {
  for (let index = frames.length - 1; index >= 0; index -= 1) {
    if (frames[index]?.kind === "try_catch") {
      return index;
    }
  }

  return -1;
}

function validateToolResult(result: unknown, schema: ReturnType<typeof buildReturnSchema>): unknown {
  if (!schema) {
    return result;
  }

  validateAgainstSchema(result, schema);
  if (isSchemaDegraded(result, schema ?? undefined)) {
    throw new SchemaDegradationError("Tool execution produced a schema-degraded payload", result);
  }

  return result;
}
