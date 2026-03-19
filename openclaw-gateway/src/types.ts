export interface Span {
  start: number;
  end: number;
}

export interface CompiledDocumentFile {
  ast_hash: string;
  document: Document;
}

export interface Document {
  imports: ImportDecl[];
  types: TypeDecl[];
  clients: ClientDecl[];
  tools: ToolDecl[];
  agents: AgentDecl[];
  workflows: WorkflowDecl[];
  listeners: ListenerDecl[];
  tests: TestDecl[];
  mocks: MockDecl[];
  span: Span;
}

export interface ImportDecl {
  names: string[];
  source: string;
  span: Span;
}

export interface TypeDecl {
  name: string;
  fields: TypeField[];
  span: Span;
}

export interface TypeField {
  name: string;
  data_type: DataType;
  constraints: Constraint[];
  span: Span;
}

export interface Constraint {
  name: string;
  value: Expr | SpannedExpr;
  span: Span;
}

export interface ClientDecl {
  name: string;
  provider: string;
  model: string;
  retries: number | null;
  timeout_ms: number | null;
  endpoint: SpannedExpr | null;
  api_key: SpannedExpr | null;
  span: Span;
}

export interface ToolDecl {
  name: string;
  arguments: TypeField[];
  return_type: DataType | null;
  invoke_path: string | null;
  span: Span;
}

export interface AgentDecl {
  name: string;
  extends: string | null;
  client: string | null;
  system_prompt: string | null;
  tools: string[];
  settings: AgentSettings;
  span: Span;
}

export interface AgentSettings {
  entries: AgentSetting[];
  span: Span;
}

export interface AgentSetting {
  name: string;
  value: SettingValue;
  span: Span;
}

export type SettingValue =
  | { Int: number }
  | { Float: number }
  | { Boolean: boolean };

export interface WorkflowDecl {
  name: string;
  arguments: TypeField[];
  return_type: DataType | null;
  body: Block;
  span: Span;
}

export interface ListenerDecl {
  name: string;
  event_type: string;
  body: Block;
  span: Span;
}

export interface TestDecl {
  name: string;
  body: Block;
  span: Span;
}

export interface MockDecl {
  target_agent: string;
  output: Array<[string, SpannedExpr]>;
  span: Span;
}

export interface SpannedExpr {
  expr: Expr;
  span: Span;
}

export interface Block {
  statements: Statement[];
  span: Span;
}

export type ElseBranch =
  | { Else: Block }
  | { ElseIf: Statement };

export type Statement =
  | {
      LetDecl: {
        name: string;
        explicit_type: DataType | null;
        value: Expr | SpannedExpr;
        span: Span;
      };
    }
  | {
      ForLoop: {
        item_name: string;
        iterator: Expr | SpannedExpr;
        body: Block;
        span: Span;
      };
    }
  | {
      IfCond: {
        condition: Expr | SpannedExpr;
        if_body: Block;
        else_body: ElseBranch | null;
        span: Span;
      };
    }
  | {
      ExecuteRun: {
        agent_name: string;
        kwargs: Array<[string, Expr | SpannedExpr]>;
        require_type: DataType | null;
        span: Span;
      };
    }
  | {
      Return: {
        value: Expr | SpannedExpr;
        span: Span;
      };
    }
  | {
      TryCatch: {
        try_body: Block;
        catch_name: string;
        catch_type: DataType;
        catch_body: Block;
        span: Span;
      };
    }
  | {
      Assert: {
        condition: Expr | SpannedExpr;
        message: string | null;
        span: Span;
      };
    }
  | {
      Continue: Span;
    }
  | {
      Break: Span;
    }
  | {
      Expression: SpannedExpr | [Expr, Span];
    };

export type Expr =
  | { StringLiteral: string }
  | { IntLiteral: number }
  | { FloatLiteral: number }
  | { BoolLiteral: boolean }
  | { Identifier: string }
  | { ArrayLiteral: Array<Expr | SpannedExpr> }
  | { Call: [string, Array<Expr | SpannedExpr>] }
  | { MemberAccess: [Expr | SpannedExpr, string] }
  | { MethodCall: [Expr | SpannedExpr, string, Array<Expr | SpannedExpr>] }
  | {
      ExecuteRun: {
        agent_name: string;
        kwargs: Array<[string, Expr | SpannedExpr]>;
        require_type: DataType | null;
      };
    }
  | {
      BinaryOp: {
        left: Expr | SpannedExpr;
        op: BinaryOp;
        right: Expr | SpannedExpr;
      };
    };

export type BinaryOp =
  | "Equal"
  | "NotEqual"
  | "LessThan"
  | "GreaterThan"
  | "LessEq"
  | "GreaterEq";

export type DataType =
  | { String: Span }
  | { Int: Span }
  | { Float: Span }
  | { Boolean: Span }
  | { List: [DataType, Span] }
  | { Custom: [string, Span] };

export interface ExecutionRequest {
  workflow: string;
  arguments: Record<string, unknown>;
  ast_hash: string;
  session_id: string;
}

export interface BlockFrame {
  kind: "block";
  blockPath: string;
  nextIndex: number;
  createdScope: boolean;
}

export interface LoopFrame {
  kind: "loop";
  statementPath: string;
  itemName: string;
  items: unknown[];
  index: number;
  bodyPath: string;
}

export interface TryCatchFrame {
  kind: "try_catch";
  statementPath: string;
  catchName: string;
  catchBodyPath: string;
}

export type ExecutionFrame = BlockFrame | LoopFrame | TryCatchFrame;

export interface ExecutionState {
  sessionId: string;
  astHash: string;
  workflowName: string;
  scopes: Array<Record<string, unknown>>;
  frames: ExecutionFrame[];
  returnValue: unknown | null;
  status: "running" | "completed" | "waiting_human";
}

export interface GatewaySuccess {
  session_id: string;
  status: "success";
  result: unknown;
}

export interface HumanInterventionEvent {
  type: "HumanInterventionRequired";
  session_id: string;
  reason: string;
  metadata: Record<string, unknown>;
}

export interface TypeBoxSchema {
  type: "string" | "number" | "integer" | "boolean" | "array" | "object";
  properties?: Record<string, TypeBoxSchema>;
  required?: string[];
  additionalProperties?: boolean;
  items?: TypeBoxSchema;
  minLength?: number;
  maxLength?: number;
  minimum?: number;
  maximum?: number;
  pattern?: string;
  minItems?: number;
  maxItems?: number;
}
