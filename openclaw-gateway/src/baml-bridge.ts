import { stat } from "node:fs/promises";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

import { log } from "./logger.ts";

type BamlFunction = (args: Record<string, unknown>) => Promise<unknown>;

let bamlFunctions: Record<string, BamlFunction> | null = null;
let bamlLoadedAt = 0;

/**
 * Load (or hot-reload) the generated BAML client from
 * `{workspaceRoot}/generated/baml_client/index.js`.
 *
 * Checks the file's mtime and only re-imports when the file has changed,
 * enabling hot reload during `claw dev` without process restarts.
 * If the file does not exist or fails to import, BAML is silently disabled
 * and the gateway falls back to raw HTTP (spec 18 §8).
 */
export async function loadBamlClient(workspaceRoot: string, force = false): Promise<void> {
  const clientPath = join(workspaceRoot, "generated", "baml_client", "index.js");
  try {
    const fileStat = await stat(clientPath);
    const mtime = fileStat.mtimeMs;

    if (!force && bamlFunctions !== null && mtime <= bamlLoadedAt) {
      return; // Already up-to-date
    }

    // Cache-bust with mtime query string so Node re-evaluates the module
    const url = `${pathToFileURL(clientPath).href}?t=${mtime}`;
    const mod = await import(url) as Record<string, unknown>;

    const fns: Record<string, BamlFunction> = {};
    for (const [name, value] of Object.entries(mod)) {
      if (typeof value === "function") {
        fns[name] = value as BamlFunction;
      }
    }
    bamlFunctions = fns;
    bamlLoadedAt = mtime;
  } catch {
    // BAML client not available — fall back to raw HTTP
    bamlFunctions = null;
  }
}

/**
 * Call a BAML-generated function by name.
 * Returns null when BAML is not available or the function does not exist.
 */
export function callBamlFunction(
  functionName: string,
  args: Record<string, unknown>
): Promise<unknown> | null {
  if (bamlFunctions === null) {
    return null;
  }
  const fn = bamlFunctions[functionName];
  if (typeof fn !== "function") {
    return null;
  }
  return fn(args);
}

/**
 * Build the BAML function name from an agent name and return type name.
 * Naming convention: `{AgentName}Run_{ReturnTypeName}` (spec 18 §2.1).
 */
export function bamlFunctionName(agentName: string, returnTypeName: string | null): string {
  return `${agentName}Run_${returnTypeName ?? "String"}`;
}

/**
 * Returns true when a BAML client has been successfully loaded.
 * Used in tests and health-check logic.
 */
export function isBamlAvailable(): boolean {
  return bamlFunctions !== null;
}

/**
 * Reset BAML state. Used in tests to ensure isolation.
 * @internal
 */
export function resetBamlClient(): void {
  bamlFunctions = null;
  bamlLoadedAt = 0;
}
