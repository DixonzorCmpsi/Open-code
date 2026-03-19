import { readFile } from "node:fs/promises";
import { join } from "node:path";

import { existsSync } from "node:fs";
import type { CheckpointStore } from "./checkpoints.ts";
import type { CompiledDocumentFile } from "../types.ts";

const cache = new Map<string, CompiledDocumentFile>();

export async function loadCompiledDocument(
  astHash: string,
  workspaceRoot = process.cwd(),
  checkpoints?: CheckpointStore
): Promise<CompiledDocumentFile> {
  if (cache.has(astHash)) {
    return cache.get(astHash)!;
  }

  let contents: string | null = null;
  const filePath = join(workspaceRoot, "generated", "claw", "documents", `${astHash}.json`);
  
  if (existsSync(filePath)) {
    contents = await readFile(filePath, "utf8");
    if (checkpoints) {
      await checkpoints.saveAstDocument(astHash, contents).catch(err => console.error("Failed to durably register AST:", err));
    }
  } else if (checkpoints) {
    // Durable AST Registry Drain Mode Fallback (GAN Audit 23)
    contents = await checkpoints.loadAstDocument(astHash);
  }

  if (!contents) {
    throw new Error(`Compiled document hash ${astHash} not found locally or in database registry.`);
  }

  const compiledDocument = JSON.parse(contents) as CompiledDocumentFile;
  if (compiledDocument.ast_hash !== astHash) {
    throw new Error(`Compiled document hash mismatch for ${astHash}`);
  }

  cache.set(astHash, compiledDocument);
  return compiledDocument;
}

export function resolveCompilerBinary(configPath?: string): string {
  if (configPath && existsSync(configPath)) return configPath;
  if (process.env.CLAW_BINARY_PATH && existsSync(process.env.CLAW_BINARY_PATH)) return process.env.CLAW_BINARY_PATH;
  
  const localNpm = join(process.cwd(), 'node_modules', '.bin', 'claw' + (process.platform === 'win32' ? '.cmd' : ''));
  if (existsSync(localNpm)) return localNpm;
  
  return 'claw';
}
