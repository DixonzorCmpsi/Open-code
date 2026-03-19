import { timingSafeEqual } from "node:crypto";
import type { IncomingHttpHeaders } from "node:http";

interface AuthorizationFailure {
  statusCode: number;
  payload: {
    status: string;
    message: string;
  };
}

type HeaderValue = string | string[] | undefined;
type HeaderSource = Headers | IncomingHttpHeaders | Record<string, HeaderValue>;

export function gatewayApiKeyFromEnv(
  environment: Record<string, string | undefined> = process.env
): string | null {
  const configuredEnvName = environment.CLAW_GATEWAY_API_KEY_ENV ?? "CLAW_GATEWAY_API_KEY";
  return environment[configuredEnvName] ?? environment.GATEWAY_AUTH_KEY ?? null;
}

export function authorizeGatewayRequest(
  request: { headers?: HeaderSource },
  expectedApiKey: string | null
): AuthorizationFailure | null {
  if (!expectedApiKey) {
    return null;
  }

  const providedApiKey = extractApiKey(request.headers ?? {});
  if (!providedApiKey) {
    return {
      statusCode: 401,
      payload: {
        status: "unauthorized",
        message: "Missing Claw API key"
      }
    };
  }

  if (!timingSafeCompare(providedApiKey, expectedApiKey)) {
    return {
      statusCode: 403,
      payload: {
        status: "forbidden",
        message: "Invalid Claw API key"
      }
    };
  }

  return null;
}

function extractApiKey(headers: HeaderSource): string | null {
  const explicitKey = readHeader(headers, "x-claw-key");
  if (explicitKey) {
    return explicitKey;
  }

  const authorization = readHeader(headers, "authorization");
  if (!authorization) {
    return null;
  }

  const bearerMatch = authorization.match(/^Bearer\s+(.+)$/i);
  return bearerMatch ? bearerMatch[1] : authorization;
}

function timingSafeCompare(a: string, b: string): boolean {
  const bufA = Buffer.from(a);
  const bufB = Buffer.from(b);
  if (bufA.length !== bufB.length) {
    return false;
  }
  return timingSafeEqual(bufA, bufB);
}

function readHeader(headers: HeaderSource, name: string): string | null {
  if (headers instanceof Headers) {
    return headers.get(name);
  }

  for (const [headerName, headerValue] of Object.entries(headers)) {
    if (headerName.toLowerCase() !== name.toLowerCase()) {
      continue;
    }
    if (Array.isArray(headerValue)) {
      return headerValue[0] ?? null;
    }
    return headerValue ?? null;
  }

  return null;
}
