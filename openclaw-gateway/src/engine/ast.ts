import type {
  AgentDecl,
  Block,
  ClientDecl,
  DataType,
  Document,
  Expr,
  SpannedExpr,
  Statement,
  ToolDecl,
  TypeDecl,
  WorkflowDecl
} from "../types.ts";

function isSpannedExpr(value: unknown): value is SpannedExpr {
  return (
    typeof value === "object" &&
    value !== null &&
    "expr" in value &&
    "span" in value &&
    Object.keys(value).length === 2
  );
}

export function unwrapExpr<T>(value: T | SpannedExpr): T {
  return isSpannedExpr(value) ? (value.expr as T) : (value as T);
}

export function getVariant<T extends Record<string, unknown>>(value: T | SpannedExpr): [string, unknown] {
  const entries = Object.entries(unwrapExpr(value) as Record<string, unknown>);
  if (entries.length !== 1) {
    throw new Error(`Expected single-variant object, received ${JSON.stringify(value)}`);
  }
  return entries[0];
}

export function findWorkflow(document: Document, name: string): WorkflowDecl {
  const workflow = document.workflows.find((item) => item.name === name);
  if (!workflow) {
    throw new Error(`Unknown workflow ${name}`);
  }
  return workflow;
}

export function findAgent(document: Document, name: string): AgentDecl {
  const agent = document.agents.find((item) => item.name === name);
  if (!agent) {
    throw new Error(`Unknown agent ${name}`);
  }
  return agent;
}

export function findClient(document: Document, name: string | null): ClientDecl | null {
  if (!name) {
    return null;
  }
  return document.clients.find((item) => item.name === name) ?? null;
}

export function findTool(document: Document, name: string): ToolDecl | null {
  return document.tools.find((item) => item.name === name) ?? null;
}

export function findType(document: Document, name: string): TypeDecl {
  const typeDecl = document.types.find((item) => item.name === name);
  if (!typeDecl) {
    throw new Error(`Unknown type ${name}`);
  }
  return typeDecl;
}

export function dataTypeName(dataType: DataType): string {
  const [kind, payload] = getVariant(dataType);
  if (kind === "Custom") {
    return (payload as [string, unknown])[0];
  }
  if (kind === "List") {
    return `list<${dataTypeName((payload as [DataType, unknown])[0])}>`;
  }
  return kind.toLowerCase();
}

export function resolveBlock(document: Document, blockPath: string): Block {
  const parts = blockPath.split("/");
  if (!parts[0].startsWith("workflow:")) {
    throw new Error(`Unsupported block path ${blockPath}`);
  }

  const workflowName = parts[0].slice("workflow:".length);
  const workflow = findWorkflow(document, workflowName);

  let current: unknown = workflow;
  for (const part of parts.slice(1)) {
    if (
      part === "body" &&
      typeof current === "object" &&
      current !== null &&
      "body" in current
    ) {
      current = (current as WorkflowDecl | { body: Block }).body;
      continue;
    }

    if (
      part === "statements" &&
      typeof current === "object" &&
      current !== null &&
      "statements" in current
    ) {
      current = (current as Block).statements;
      continue;
    }

    if (/^\d+$/.test(part)) {
      current = (current as Statement[])[Number(part)];
      continue;
    }

    const [variantName, payload] = getVariant(current as Statement | Expr);
    if (variantName === "ForLoop" && part === "body") {
      current = (payload as { body: Block }).body;
      continue;
    }
    if (variantName === "IfCond" && part === "if_body") {
      current = (payload as { if_body: Block }).if_body;
      continue;
    }
    if (variantName === "TryCatch" && part === "try_body") {
      current = (payload as { try_body: Block }).try_body;
      continue;
    }
    if (variantName === "TryCatch" && part === "catch_body") {
      current = (payload as { catch_body: Block }).catch_body;
      continue;
    }
    if (variantName === "IfCond" && part === "else_body") {
      const rawElse = (payload as { else_body: unknown }).else_body;
      if (!rawElse) {
        current = null;
      } else if (
        typeof rawElse === "object" &&
        rawElse !== null &&
        "statements" in rawElse &&
        "span" in rawElse
      ) {
        current = rawElse;
      } else {
        const [branchKind, branchPayload] = getVariant(rawElse as Record<string, unknown>);
        if (branchKind === "Else") {
          // ElseBranch::Else(Block) — payload is the Block directly
          current = branchPayload;
        } else if (branchKind === "ElseIf") {
          // ElseBranch::ElseIf(Box<Statement>) — synthesize a single-statement Block
          // so the frame executor can process it via the normal executeStatement path
          current = { statements: [branchPayload], span: { start: 0, end: 0 } };
        } else {
          throw new Error(`Unexpected else branch kind "${branchKind}" in path ${blockPath}`);
        }
      }
      continue;
    }

    throw new Error(`Unsupported block path segment ${part} in ${blockPath}`);
  }

  if (!current || typeof current !== "object" || !("statements" in current)) {
    throw new Error(`Resolved block path ${blockPath} did not produce a block`);
  }

  return current as Block;
}
