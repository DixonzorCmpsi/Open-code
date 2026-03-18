import { OpenClawClient } from "@openclaw/sdk";
import {
  AnalyzeCompetitors,
  OPENCLAW_AST_HASH
} from "../generated/claw/index.ts";

const client = new OpenClawClient({
  endpoint: process.env.OPENCLAW_GATEWAY_URL ?? "http://127.0.0.1:8080"
});

const result = await AnalyzeCompetitors("Apple", { client });
console.log(
  JSON.stringify(
    {
      ast_hash: OPENCLAW_AST_HASH,
      result
    },
    null,
    2
  )
);
