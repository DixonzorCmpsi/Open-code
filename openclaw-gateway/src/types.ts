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
  value: Expr;
  span: Span;
}

export interface ClientDecl {
  name: string;
  provider: string;
  model: string;
  retries: number | null;
  timeout_ms: number | null;
  endpoint: Expr | null;
  api_key: Expr | null;
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
  mock_input: Expr;
  mock_output: Expr;
  span: Span;
}

export interface Block {
  statements: Statement[];
  span: Span;
}

export type Statement =
  | {
      LetDecl: {
        name: string;
        explicit_type: DataType | null;
        value: Expr;
        span: Span;
      };
    }
  | {
      ForLoop: {
        item_name: string;
        iterator_name: string;
        body: Block;
        span: Span;
      };
    }
  | {
      IfCond: {
        condition: Expr;
        if_body: Block;
        else_body: Block | null;
        span: Span;
      };
    }
  | {
      ExecuteRun: {
        agent_name: string;
        kwargs: Array<[string, Expr]>;
        require_type: DataType | null;
        span: Span;
      };
    }
  | {
      Return: {
        value: Expr;
        span: Span;
      };
    }
  | {
      Expression: [Expr, Span];
    };

export type Expr =
  | { StringLiteral: string }
  | { IntLiteral: number }
  | { FloatLiteral: number }
  | { BoolLiteral: boolean }
  | { Identifier: string }
  | { ArrayLiteral: Expr[] }
  | { Call: [string, Expr[]] }
  | { MethodCall: [Expr, string, Expr[]] }
  | {
      ExecuteRun: {
        agent_name: string;
        kwargs: Array<[string, Expr]>;
        require_type: DataType | null;
      };
    }
  | {
      BinaryOp: {
        left: Expr;
        op: BinaryOp;
        right: Expr;
      };
    };

export type BinaryOp = "Equal";

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

export type ExecutionFrame = BlockFrame | LoopFrame;

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
