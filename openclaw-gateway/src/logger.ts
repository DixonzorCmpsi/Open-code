export type LogLevel = "error" | "warn" | "info" | "debug";

const PRIORITY: Record<LogLevel, number> = {
  error: 0,
  warn: 1,
  info: 2,
  debug: 3
};

export function log(level: LogLevel, event: string, data?: Record<string, unknown>): void {
  if (PRIORITY[level] > PRIORITY[configuredLevel()]) {
    return;
  }

  if (process.env.CLAW_LOG_FORMAT === "json") {
    const entry = {
      timestamp: new Date().toISOString(),
      level,
      event,
      ...data
    };
    process.stderr.write(`${JSON.stringify(entry)}\n`);
    return;
  }

  const prefix = "[claw-gateway]";
  const message = data ? `${event} ${JSON.stringify(data)}` : event;
  if (level === "error") {
    console.error(`${prefix} ERROR: ${message}`);
    return;
  }

  if (level === "warn") {
    console.error(`${prefix} WARN: ${message}`);
    return;
  }

  console.log(`${prefix} ${message}`);
}

function configuredLevel(): LogLevel {
  const value = process.env.CLAW_LOG_LEVEL;
  if (value === "error" || value === "warn" || value === "info" || value === "debug") {
    return value;
  }
  return "info";
}
