# Production Deployment Guide

Deploying a `.claw` pipeline involves compiling your source code into OpenCode-native configurations and an MCP server.

---

## 1. SDK Synchronization in CI/CD (Client)

The generated SDK code (`generated/claw/*`) should **never be committed** to version control (e.g., GitHub). Because it relies on static `.claw` files, committing the generated output leads to severe team synchronization drift if someone edits `example.claw` but forgets to run `claw build`.

### Fix: The Pre-build Hook
Add the `claw build` command directly into your `package.json` build scripts so that the TypeScript or Python SDK is generated dynamically inside your serverless container (e.g. Vercel) or your GitHub Actions Pipeline.

```json
{
  "scripts": {
    "prebuild": "claw build example.claw --lang ts",
    "build": "next build",
    "test": "claw test example.claw && jest"
  }
}
```

For local iteration, the CLI can bootstrap and watch your project:

```bash
claw init
claw dev
```

---

## Deployment

1. Install the Claw compiler: `cargo install clawc` (or download from GitHub Releases)
2. Write your `.claw` file
3. Compile: `claw build`
4. Install MCP deps: `npm install`
5. Run with OpenCode: `opencode /WorkflowName "arg"`
6. For local models: set `LOCAL_ENDPOINT=http://localhost:11434` in your environment

---

## 2. Using the Generated SDK

In your Next.js API route or Python backend:

```typescript
import { FindInfo } from "./generated/claw"

export async function POST(req: Request) {
    const data = await req.json()
    
    // The orchestration is executed by OpenCode
    const report = await FindInfo(data.topic)
    
    return Response.json(report)
}
```
