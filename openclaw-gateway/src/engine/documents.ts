import { readFile } from "node:fs/promises";
import { join } from "node:path";

import type { CompiledDocumentFile } from "../types.ts";

const cache = new Map<string, CompiledDocumentFile>();

export async function loadCompiledDocument(
  astHash: string,
  workspaceRoot = process.cwd()
): Promise<CompiledDocumentFile> {
  if (cache.has(astHash)) {
    return cache.get(astHash)!;
  }

  const filePath = join(workspaceRoot, "generated", "claw", "documents", `${astHash}.json`);
  const contents = await readFile(filePath, "utf8");
  const compiledDocument = JSON.parse(contents) as CompiledDocumentFile;
  if (compiledDocument.ast_hash !== astHash) {
    throw new Error(`Compiled document hash mismatch for ${astHash}`);
  }

  cache.set(astHash, compiledDocument);
  return compiledDocument;
}
