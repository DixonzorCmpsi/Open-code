import { search, navigate } from "../tools/browser.ts";
import type {
  AgentDecl,
  CompiledDocumentFile,
  DataType,
  ExecutionFrame,
  ExecutionRequest,
  ExecutionState,
  Expr,
  Statement,
  ToolDecl
} from "../types.ts";
import { CheckpointStore } from "./checkpoints.ts";
import { findAgent, findClient, findTool, findType, findWorkflow, getVariant, resolveBlock } from "./ast.ts";
import { buildReturnSchema, generateStructuredResult } from "./llm.ts";
import { HumanInterventionRequiredError, SchemaDegradationError } from "./errors.ts";
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
      const frame = state.frames[state.frames.length - 1]!;
      if (frame.kind === "loop") {
        advanceLoopFrame(state, frame);
        await checkpoints.checkpoint(state, frame.statementPath, "loop_tick");
        continue;
      }

      const block = resolveBlock(compiled.document, frame.blockPath);
      if (frame.nextIndex >= block.statements.length) {
        state.frames.pop();
        if (frame.createdScope) {
          state.scopes.pop();
        }
        continue;
      }

      const statementPath = `${frame.blockPath}/statements/${frame.nextIndex}`;
      const statement = block.statements[frame.nextIndex]!;
      await executeStatement(compiled, state, statement, statementPath, options.workspaceRoot ?? process.cwd(), checkpoints);
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
      const loopPayload = payload as { item_name: string; iterator_name: string; body: unknown };
      const values = resolveVariable(state, loopPayload.iterator_name);
      if (!Array.isArray(values)) {
        throw new Error(`ForLoop iterator ${loopPayload.iterator_name} must be an array`);
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
      const ifPayload = payload as { condition: Expr; else_body: unknown };
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
      const [expr] = payload as [Expr, unknown];
      await evaluateExpr(compiled, state, expr, statementPath, workspaceRoot, checkpoints);
      frame.nextIndex += 1;
      await checkpoints.checkpoint(state, statementPath, "expression");
      return;
    }
    default:
      throw new Error(`Unsupported statement kind ${kind}`);
  }
}

type StatementExecuteRun = {
  agent_name: string;
  kwargs: Array<[string, Expr]>;
  require_type: DataType | null;
};

async function evaluateExpr(
  compiled: CompiledDocumentFile,
  state: ExecutionState,
  expr: Expr,
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
      return Promise.all((payload as Expr[]).map((item) => evaluateExpr(compiled, state, item, statementPath, workspaceRoot, checkpoints)));
    case "ExecuteRun":
      return executeAgentRun(compiled, state, payload as StatementExecuteRun, statementPath, workspaceRoot, checkpoints);
    case "BinaryOp": {
      const left = await evaluateExpr(compiled, state, (payload as { left: Expr }).left, statementPath, workspaceRoot, checkpoints);
      const right = await evaluateExpr(compiled, state, (payload as { right: Expr }).right, statementPath, workspaceRoot, checkpoints);
      const result = JSON.stringify(left) === JSON.stringify(right);
      await checkpoints.checkpoint(state, statementPath, "binary_op", { result });
      return result;
    }
    case "MethodCall": {
      const methodResult = await applyMethodCall(compiled, state, payload as [Expr, string, Expr[]], statementPath, workspaceRoot, checkpoints);
      await checkpoints.checkpoint(state, statementPath, "method_call", { result: methodResult });
      return methodResult;
    }
    case "Call": {
      const [workflowName, argExprs] = payload as [string, Expr[]];
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
          session_id: `${state.sessionId}:${workflowName}:${Date.now()}`
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
  payload: [Expr, string, Expr[]],
  statementPath: string,
  workspaceRoot: string,
  checkpoints: CheckpointStore
): Promise<unknown> {
  const [targetExpr, methodName, args] = payload;
  const target = await evaluateExpr(compiled, state, targetExpr, statementPath, workspaceRoot, checkpoints);

  if (methodName === "length" && Array.isArray(target)) {
    return target.length;
  }

  if (methodName === "append" && "Identifier" in targetExpr && Array.isArray(target)) {
    const [nextValue] = await Promise.all(
      args.map((arg) => evaluateExpr(compiled, state, arg, statementPath, workspaceRoot, checkpoints))
    );
    target.push(nextValue);
    assignVariable(state, targetExpr.Identifier, target);
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
  const client = findClient(compiled.document, agent.client);
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

function validateToolResult(result: unknown, schema: ReturnType<typeof buildReturnSchema>): unknown {
  if (!schema) {
    return result;
  }

  validateAgainstSchema(result, schema);
  if (isSchemaDegraded(result)) {
    throw new SchemaDegradationError("Tool execution produced a schema-degraded payload", result);
  }

  return result;
}
