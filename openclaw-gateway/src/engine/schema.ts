import { dataTypeName, findType, getVariant } from "./ast.ts";
import type { DataType, Document, TypeBoxSchema, TypeField } from "../types.ts";

export function buildTypeBoxSchema(document: Document, dataType: DataType): TypeBoxSchema {
  const [kind, payload] = getVariant(dataType);

  switch (kind) {
    case "String":
      return { type: "string" };
    case "Int":
      return { type: "integer" };
    case "Float":
      return { type: "number" };
    case "Boolean":
      return { type: "boolean" };
    case "List":
      return {
        type: "array",
        items: buildTypeBoxSchema(document, (payload as [DataType, unknown])[0])
      };
    case "Custom": {
      const [name] = payload as [string, unknown];
      const typeDecl = findType(document, name);
      return {
        type: "object",
        properties: Object.fromEntries(
          typeDecl.fields.map((field) => [field.name, applyConstraints(buildTypeBoxSchema(document, field.data_type), field)])
        ),
        required: typeDecl.fields.map((field) => field.name),
        additionalProperties: false
      };
    }
    default:
      throw new Error(`Unsupported data type ${dataTypeName(dataType)}`);
  }
}

export function validateAgainstSchema(value: unknown, schema: TypeBoxSchema, path = "$"): void {
  switch (schema.type) {
    case "string":
      if (typeof value !== "string") {
        throw new TypeError(`${path} expected string`);
      }
      if (schema.minLength !== undefined && value.length < schema.minLength) {
        throw new TypeError(`${path} expected min length ${schema.minLength}`);
      }
      if (schema.maxLength !== undefined && value.length > schema.maxLength) {
        throw new TypeError(`${path} expected max length ${schema.maxLength}`);
      }
      if (schema.pattern && !new RegExp(schema.pattern).test(value)) {
        throw new TypeError(`${path} expected pattern ${schema.pattern}`);
      }
      return;
    case "integer":
      if (typeof value !== "number" || !Number.isInteger(value)) {
        throw new TypeError(`${path} expected integer`);
      }
      validateNumericBounds(value, schema, path);
      return;
    case "number":
      if (typeof value !== "number" || Number.isNaN(value)) {
        throw new TypeError(`${path} expected number`);
      }
      validateNumericBounds(value, schema, path);
      return;
    case "boolean":
      if (typeof value !== "boolean") {
        throw new TypeError(`${path} expected boolean`);
      }
      return;
    case "array":
      if (!Array.isArray(value)) {
        throw new TypeError(`${path} expected array`);
      }
      if (schema.minItems !== undefined && value.length < schema.minItems) {
        throw new TypeError(`${path} expected min items ${schema.minItems}`);
      }
      if (schema.maxItems !== undefined && value.length > schema.maxItems) {
        throw new TypeError(`${path} expected max items ${schema.maxItems}`);
      }
      value.forEach((item, index) => validateAgainstSchema(item, schema.items!, `${path}[${index}]`));
      return;
    case "object":
      if (value === null || typeof value !== "object" || Array.isArray(value)) {
        throw new TypeError(`${path} expected object`);
      }
      for (const key of schema.required ?? []) {
        if (!(key in (value as Record<string, unknown>))) {
          throw new TypeError(`${path}.${key} is required`);
        }
      }
      for (const [key, propertySchema] of Object.entries(schema.properties ?? {})) {
        validateAgainstSchema((value as Record<string, unknown>)[key], propertySchema, `${path}.${key}`);
      }
      if (schema.additionalProperties === false) {
        for (const key of Object.keys(value as Record<string, unknown>)) {
          if (!(schema.properties && key in schema.properties)) {
            throw new TypeError(`${path}.${key} is not allowed`);
          }
        }
      }
      return;
    default:
      throw new Error(`Unsupported schema type ${(schema as { type: string }).type}`);
  }
}

/**
 * A response is degraded if and only if ALL leaf values are their type's
 * zero-value simultaneously (every string is "", every number is 0, every
 * boolean is false). Individual 0, false, or "" values are NOT degraded —
 * they are legitimate data.
 *
 * Per specs/07-OpenClaw-OS.md Section 2.4.
 */
export function isSchemaDegraded(value: unknown): boolean {
  if (value == null) {
    return true;
  }
  if (typeof value === "string") {
    return value.trim().length === 0;
  }
  if (typeof value === "number") {
    return value === 0;
  }
  if (typeof value === "boolean") {
    return value === false;
  }
  if (Array.isArray(value)) {
    return value.length === 0 || value.every((item) => isSchemaDegraded(item));
  }
  if (typeof value === "object") {
    const values = Object.values(value as Record<string, unknown>);
    // Only degraded if ALL leaves are zero-values simultaneously
    return values.length === 0 || values.every((item) => isSchemaDegraded(item));
  }
  return false;
}

function applyConstraints(schema: TypeBoxSchema, field: TypeField): TypeBoxSchema {
  const constrained = { ...schema };
  for (const constraint of field.constraints) {
    const [valueKind, value] = getVariant(constraint.value);
    if (constraint.name === "regex" && valueKind === "StringLiteral") {
      constrained.pattern = value as string;
    }
    if (constraint.name === "min" && valueKind === "IntLiteral") {
      applyMinConstraint(constrained, value as number);
    }
    if (constraint.name === "max" && valueKind === "IntLiteral") {
      applyMaxConstraint(constrained, value as number);
    }
  }
  return constrained;
}

function applyMinConstraint(schema: TypeBoxSchema, minimum: number): void {
  if (schema.type === "string") {
    schema.minLength = minimum;
  } else if (schema.type === "array") {
    schema.minItems = minimum;
  } else {
    schema.minimum = minimum;
  }
}

function applyMaxConstraint(schema: TypeBoxSchema, maximum: number): void {
  if (schema.type === "string") {
    schema.maxLength = maximum;
  } else if (schema.type === "array") {
    schema.maxItems = maximum;
  } else {
    schema.maximum = maximum;
  }
}

function validateNumericBounds(value: number, schema: TypeBoxSchema, path: string): void {
  if (schema.minimum !== undefined && value < schema.minimum) {
    throw new TypeError(`${path} expected minimum ${schema.minimum}`);
  }
  if (schema.maximum !== undefined && value > schema.maximum) {
    throw new TypeError(`${path} expected maximum ${schema.maximum}`);
  }
}
