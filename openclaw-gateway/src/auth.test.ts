import assert from "node:assert/strict";
import test from "node:test";

import { authorizeGatewayRequest, gatewayApiKeyFromEnv } from "./auth.ts";

test("gateway auth is disabled when no api key env var is configured", () => {
  assert.equal(gatewayApiKeyFromEnv({}), null);
  assert.equal(
    authorizeGatewayRequest({ headers: {} }, null),
    null
  );
});

test("gateway auth prefers the configured api key env var over the deprecated fallback", () => {
  assert.equal(
    gatewayApiKeyFromEnv({
      CLAW_GATEWAY_API_KEY_ENV: "CUSTOM_GATEWAY_KEY",
      CUSTOM_GATEWAY_KEY: "preferred",
      GATEWAY_AUTH_KEY: "deprecated"
    }),
    "preferred"
  );
});

test("gateway auth accepts x-claw-key and bearer authorization headers", () => {
  const expected = "prod_secret";

  assert.equal(
    authorizeGatewayRequest(
      {
        headers: {
          "x-claw-key": expected
        }
      },
      expected
    ),
    null
  );

  assert.equal(
    authorizeGatewayRequest(
      {
        headers: {
          authorization: `Bearer ${expected}`
        }
      },
      expected
    ),
    null
  );
});

test("gateway auth rejects missing and invalid api keys", () => {
  assert.deepEqual(
    authorizeGatewayRequest({ headers: {} }, "prod_secret"),
    {
      statusCode: 401,
      payload: {
        status: "unauthorized",
        message: "Missing Claw API key"
      }
    }
  );

  assert.deepEqual(
    authorizeGatewayRequest(
      {
        headers: {
          "x-claw-key": "wrong"
        }
      },
      "prod_secret"
    ),
    {
      statusCode: 403,
      payload: {
        status: "forbidden",
        message: "Invalid Claw API key"
      }
    }
  );
});
