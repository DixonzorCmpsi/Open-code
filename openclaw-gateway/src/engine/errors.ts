import type { HumanInterventionEvent } from "../types.ts";

export class SchemaDegradationError extends Error {
  payload: unknown;

  constructor(message: string, payload: unknown) {
    super(message);
    this.name = "SchemaDegradationError";
    this.payload = payload;
  }
}

export class HumanInterventionRequiredError extends Error {
  event: HumanInterventionEvent;

  constructor(event: HumanInterventionEvent) {
    super(event.reason);
    this.name = "HumanInterventionRequiredError";
    this.event = event;
  }
}
