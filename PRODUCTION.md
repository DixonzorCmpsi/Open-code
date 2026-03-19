# Production Deployment Guide

Deploying a `.claw` pipeline to a production environment requires two distinct layers: The Generated SDK (Client) and the OpenClaw Gateway (The Operating System).

---

## 1. SDK Synchronization in CI/CD (Client)

The generated SDK code (`generated/claw/*`) should **never be committed** to version control (e.g., GitHub). Because it relies on static `.claw` files, committing the generated output leads to severe team synchronization drift if someone edits `pipeline.claw` but forgets to run `clawc build`.

### Fix: The Pre-build Hook
Add the `clawc build` command directly into your `package.json` build scripts so that the TypeScript or Python SDK is generated dynamically inside your serverless container (e.g. Vercel) or your GitHub Actions Pipeline.

```json
{
  "scripts": {
    "prebuild": "clawc build src/pipeline.claw --lang ts",
    "build": "next build",
    "test": "clawc test src/pipeline.claw && jest"
  }
}
```

For local iteration, the new workspace CLI can bootstrap and watch your project:

```bash
openclaw init
openclaw build --watch
```

---

## 2. Deploying the OpenClaw OS Gateway

The generated SDK makes WebSocket or REST calls out to the Heavy execution runtime. 

By default, during local development, you do not need Authentication or a Redis Database. Your SDK simply spins up an ephemeral local gateway (or connects to `localhost:8080` unauthenticated) and uses a local SQLite file for checkpointing. 

For production, you must scale the OpenClaw OS persistently and secure it.

### The Gateway Docker Container
The OpenClaw OS requires Node.js, Playwright (Browser primitives), and secure sandboxing capabilities for external Custom Tools.

We provide a specialized high-performance Docker image intended for execution on managed services like AWS ECS, Azure Container Apps, or Google Cloud Run.

```yaml
version: '3.8'
services:
  openclaw-os:
    image: openclaw/gateway-os:latest
    environment:
      # Inject whatever endpoints your local models or APIs need
      - OPENAI_API_KEY=${OPENAI_API_KEY}
      - CUSTOM_LLM_URL=${CUSTOM_LLM_URL}
      - CUSTOM_LLM_KEY=${CUSTOM_LLM_KEY}
      
      # Optional Production Security & Scaling
      - CLAW_GATEWAY_API_KEY=${CLAW_GATEWAY_API_KEY} # Locks down the endpoint
      - REDIS_URL=redis://your-managed-db:6379 # Swaps SQLite for Redis Checkpointing
      - CLAW_SANDBOX_BACKEND=docker # Enforces containerized python()/typescript() tools
      - CLAW_PYTHON_SANDBOX_IMAGE=python:3.11-slim
      - CLAW_NODE_SANDBOX_IMAGE=node:22
    ports:
      - "8080:8080"
```

### Initializing your SDK with the Production Node
In your Next.js API route or Python backend:

```typescript
import { OpenClawClient } from "@openclaw/sdk"
import { AnalyzeCompetitor } from "./generated/claw"

// Point the client wrapper at your internal VPC Gateway
// If hitting localhost, api_key is not needed.
const prodGateway = new OpenClawClient({ 
    endpoint: "https://openclaw-os.internal.vpc.com",
    api_key: process.env.CLAW_GATEWAY_API_KEY 
})

export async function POST(req: Request) {
    const data = await req.json()
    
    // The orchestration is routed into the heavy background server
    const report = await AnalyzeCompetitor(data.company_url, { 
        client: prodGateway 
    })
    
    return Response.json(report)
}
```

---

## 3. Resume Checkpoints (Crash Recovery)

If your massive `.claw` workflow crashes midway through a 500-item `for` loop because the OpenClaw OS container was OOM-Killed, you can effortlessly resume it by referencing the exact AST state.

Because you provided a `REDIS_URL` to the OpenClaw Gateway, the OS automatically saved the deterministic `Session` state after every loop. 

Catch the crash in your web server, grab the failed `Session ID`, and pass it back in:

```typescript
try {
    const report = await AnalyzeCompetitor("https://apple.com", { client: prodGateway })
} catch (error) {
    if (error instanceof OpenClawExecutionError) {
        console.log("Server crashed on step 250. Checkpoint ID:", error.sessionId);
        
        // Pass the session ID back in to resume execution instantly where it failed:
        const recoveredReport = await AnalyzeCompetitor("https://apple.com", { 
            client: prodGateway,
            resumeSessionId: error.sessionId
        })
    }
}
```
