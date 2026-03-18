export class OpenClawExecutionError extends Error {
  constructor(message, options = {}) {
    super(message);
    this.name = "OpenClawExecutionError";
    this.sessionId = options.sessionId;
    this.status = options.status;
    this.event = options.event ?? null;
    this.payload = options.payload ?? null;
  }
}

export class OpenClawClient {
  constructor(options = {}) {
    this.endpoint = (options.endpoint ?? "http://127.0.0.1:8080").replace(/\/$/, "");
    this.apiKey = options.api_key ?? options.apiKey ?? null;
  }

  async executeWorkflow({ workflowName, arguments: args, astHash, resumeSessionId }) {
    const sessionId = resumeSessionId ?? `req_${crypto.randomUUID()}`;
    const headers = {
      "content-type": "application/json"
    };
    if (this.apiKey) {
      headers["x-openclaw-key"] = this.apiKey;
    }

    const response = await fetch(`${this.endpoint}/workflows/execute`, {
      method: "POST",
      headers,
      body: JSON.stringify({
        workflow: workflowName,
        arguments: args,
        ast_hash: astHash,
        session_id: sessionId
      })
    });

    const payload = await response.json();
    if (!response.ok || payload.status !== "success") {
      throw new OpenClawExecutionError(
        payload.message ?? `Workflow execution failed with status ${response.status}`,
        {
          sessionId: payload.session_id ?? sessionId,
          status: payload.status,
          event: payload.event ?? null,
          payload
        }
      );
    }

    return payload.result;
  }

  /**
   * Stream a workflow execution over WebSocket, yielding checkpoint events.
   * Returns an async generator of gateway messages (checkpoint, result, error).
   *
   * Usage:
   *   for await (const event of client.streamWorkflow({ ... })) {
   *     if (event.type === "checkpoint") console.log(event.node_path);
   *     if (event.type === "result") return event.result;
   *   }
   */
  async *streamWorkflow({ workflowName, arguments: args, astHash, resumeSessionId }) {
    // WebSocket via dynamic import so it works in both Node and browser
    const wsEndpoint = this.endpoint.replace(/^http/, "ws") + "/workflows/stream";
    const sessionId = resumeSessionId ?? `req_${crypto.randomUUID()}`;

    const { WebSocket: WS } = await import("ws").catch(() => ({ WebSocket: globalThis.WebSocket }));
    const headers = {};
    if (this.apiKey) {
      headers["x-openclaw-key"] = this.apiKey;
    }

    const ws = new WS(wsEndpoint, { headers });
    const messageQueue = [];
    let resolve = null;
    let done = false;

    ws.onmessage = (event) => {
      const data = typeof event.data === "string" ? JSON.parse(event.data) : JSON.parse(event.data.toString());
      if (resolve) {
        const r = resolve;
        resolve = null;
        r(data);
      } else {
        messageQueue.push(data);
      }
    };

    ws.onerror = (error) => {
      done = true;
      if (resolve) {
        const r = resolve;
        resolve = null;
        r({ type: "error", message: error.message ?? "WebSocket error" });
      }
    };

    ws.onclose = () => {
      done = true;
      if (resolve) {
        const r = resolve;
        resolve = null;
        r(null);
      }
    };

    await new Promise((r) => { ws.onopen = r; });

    ws.send(JSON.stringify({
      workflow: workflowName,
      arguments: args,
      ast_hash: astHash,
      session_id: sessionId
    }));

    while (!done) {
      const message = messageQueue.length > 0
        ? messageQueue.shift()
        : await new Promise((r) => { resolve = r; });

      if (!message) break;

      yield message;

      if (message.type === "result" || message.type === "error") {
        if (message.type === "error") {
          throw new OpenClawExecutionError(message.message, {
            sessionId: message.session_id ?? sessionId,
            status: "error",
            payload: message
          });
        }
        break;
      }
    }

    ws.close();
  }
}

export { OpenClawExecutionError as AgentExecutionError };
