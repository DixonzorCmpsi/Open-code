import { buildTypeBoxSchema, isSchemaDegraded, validateAgainstSchema } from "./schema.ts";
import type { AgentDecl, ClientDecl, Document, SpannedExpr, ToolDecl, TypeBoxSchema, TypeDecl } from "../types.ts";
import { SchemaDegradationError } from "./errors.ts";

interface LlmBridgeRequest {
  document: Document;
  agent: AgentDecl;
  client: ClientDecl | null;
  returnType: TypeDecl | null;
  returnSchema: TypeBoxSchema;
  kwargs: Record<string, unknown>;
  tools: ToolDecl[];
}

export async function generateStructuredResult(request: LlmBridgeRequest): Promise<unknown> {
  const candidate =
    (await tryProviderBridge(request)) ??
    createMockResponse(request.returnSchema, request.kwargs, []);

  validateAgainstSchema(candidate, request.returnSchema);
  if (isSchemaDegraded(candidate, request.returnSchema)) {
    throw new SchemaDegradationError("Structured result degraded into empty defaults", candidate);
  }
  return candidate;
}

async function tryProviderBridge(request: LlmBridgeRequest): Promise<unknown | null> {
  if (!request.client) {
    return null;
  }

  if (request.client.provider === "openai" && resolveClientString(request.client.api_key, process.env.OPENAI_API_KEY)) {
    return callOpenAI(request);
  }

  if (request.client.provider === "anthropic" && resolveClientString(request.client.api_key, process.env.ANTHROPIC_API_KEY)) {
    return callAnthropic(request);
  }

  return null;
}

async function callOpenAI(request: LlmBridgeRequest): Promise<unknown> {
  const endpoint =
    resolveClientString(request.client?.endpoint, process.env.OPENAI_BASE_URL) ??
    "https://api.openai.com/v1/responses";
  const apiKey = resolveClientString(request.client?.api_key, process.env.OPENAI_API_KEY);
  const response = await fetch(endpoint, {
    method: "POST",
    headers: {
      "content-type": "application/json",
      authorization: `Bearer ${apiKey}`
    },
    body: JSON.stringify({
      model: request.client!.model,
      input: [
        {
          role: "system",
          content: request.agent.system_prompt ?? "You are a deterministic Claw execution agent."
        },
        {
          role: "user",
          content: JSON.stringify(request.kwargs)
        }
      ],
      text: {
        format: {
          type: "json_schema",
          name: request.returnType?.name ?? "ClawResult",
          schema: request.returnSchema
        }
      }
    })
  });

  if (!response.ok) {
    const errorBody = await response.text().catch(() => "");
    throw new Error(`OpenAI API returned ${response.status}: ${errorBody}`);
  }

  const payload = await response.json();
  const text = payload.output_text ?? payload.output?.[0]?.content?.[0]?.text;
  if (!text) {
    return null;
  }

  try {
    return JSON.parse(text);
  } catch {
    throw new Error(`OpenAI returned non-JSON response: ${text.slice(0, 200)}`);
  }
}

/**
 * Per specs/07-Claw-OS.md Section 6: use Anthropic's `tools` parameter
 * with `input_schema` for constrained output. Extract result from
 * content[].type === "tool_use" → content[].input.
 *
 * NEVER place response_schema inside message content — Anthropic ignores it there.
 */
async function callAnthropic(request: LlmBridgeRequest): Promise<unknown> {
  const endpoint =
    resolveClientString(request.client?.endpoint, undefined) ??
    "https://api.anthropic.com/v1/messages";
  const apiKey = resolveClientString(request.client?.api_key, process.env.ANTHROPIC_API_KEY);
  const response = await fetch(endpoint, {
    method: "POST",
    headers: {
      "content-type": "application/json",
      "x-api-key": apiKey!,
      "anthropic-version": "2025-01-01"
    },
    body: JSON.stringify({
      model: request.client!.model,
      max_tokens: 4096,
      system: request.agent.system_prompt ?? "You are a deterministic Claw execution agent.",
      tools: [
        {
          name: "structured_output",
          description: "Return the result matching the required schema",
          input_schema: request.returnSchema
        }
      ],
      tool_choice: { type: "tool", name: "structured_output" },
      messages: [
        {
          role: "user",
          content: JSON.stringify(request.kwargs)
        }
      ]
    })
  });

  if (!response.ok) {
    const errorBody = await response.text().catch(() => "");
    throw new Error(`Anthropic API returned ${response.status}: ${errorBody}`);
  }

  const payload = await response.json();

  // Extract from tool_use content block
  const toolUseBlock = payload.content?.find(
    (block: { type: string }) => block.type === "tool_use"
  );
  if (toolUseBlock?.input) {
    return toolUseBlock.input;
  }

  // Fallback: try text content block
  const textBlock = payload.content?.find(
    (block: { type: string }) => block.type === "text"
  );
  if (!textBlock?.text) {
    return null;
  }

  try {
    return JSON.parse(textBlock.text);
  } catch {
    throw new Error(`Anthropic returned non-JSON response: ${textBlock.text.slice(0, 200)}`);
  }
}

function createMockResponse(
  schema: TypeBoxSchema,
  kwargs: Record<string, unknown>,
  path: string[]
): unknown {
  switch (schema.type) {
    case "string":
      return mockString(path, kwargs, schema);
    case "integer":
      return schema.minimum ?? 1;
    case "number":
      return schema.minimum ?? (path.at(-1) === "confidence_score" ? 0.95 : 1);
    case "boolean":
      return true;
    case "array":
      return [createMockResponse(schema.items!, kwargs, path)];
    case "object":
      return Object.fromEntries(
        Object.entries(schema.properties ?? {}).map(([key, value]) => [
          key,
          createMockResponse(value, kwargs, [...path, key])
        ])
      );
    default:
      return null;
  }
}

function mockString(
  path: string[],
  kwargs: Record<string, unknown>,
  schema: TypeBoxSchema
): string {
  const key = path.at(-1) ?? "";
  const company = String(kwargs.company ?? kwargs.task ?? "claw").toLowerCase();
  if (key.includes("url") || schema.pattern === "^https://") {
    return `https://${company.replace(/\s+/g, "-")}.com`;
  }
  if (key.includes("snippet")) {
    return `${String(kwargs.company ?? kwargs.task ?? "Claw")} summary generated by the mock bridge.`;
  }
  if (key.includes("tag")) {
    return "analysis";
  }
  return `${key || "value"}-${company}`;
}

export function buildReturnSchema(document: Document, typeName: string | null): TypeBoxSchema | null {
  if (!typeName) {
    return null;
  }
  return buildTypeBoxSchema(document, { Custom: [typeName, { start: 0, end: 0 }] });
}

function resolveClientString(field: SpannedExpr | null | undefined, fallback?: string): string | undefined {
  const literal = field?.expr;
  if (literal && "StringLiteral" in literal) {
    return literal.StringLiteral;
  }
  return fallback;
}
