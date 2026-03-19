import { ClawClient } from "@claw/sdk";
import {
  AnalyzeCompetitors,
  CLAW_AST_HASH
} from "../generated/claw/index.ts";

const client = new ClawClient({
  endpoint: process.env.CLAW_GATEWAY_URL ?? "http://127.0.0.1:8080"
});

const result = await AnalyzeCompetitors("Apple", { client });
console.log(
  JSON.stringify(
    {
      ast_hash: CLAW_AST_HASH,
      result
    },
    null,
    2
  )
);
