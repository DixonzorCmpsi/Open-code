# OpenClaw DSL: The Deterministic Agent Compiler

<div align="center">
  <h3>Code -> AI, not AI -> Code</h3>
  <p>An enterprise-grade, statically-typed compiler and DSL for orchestrating multi-agent topologies.</p>
</div>

---

## 🦅 What is `.claw`?

`OpenClaw` is a declarative, object-oriented language for deterministic multi-agent orchestration. It is built to solve the brittle, hallucination-prone nature of writing raw Python or TypeScript scripts to chain LLM calls together.

If you are currently chaining agents using pure dictionaries, regex parsing, and massive string prompts: you are building a liability.

The `.claw` compiler (`clawc`) allows you to define Agents, Tools, and Workflows with **mathematical type safety**. It parses your architecture, guarantees your tools match your agent requests, and outputs auto-generated SDKs (TypeScript and Python) that you simply `import` into your production code.

**Your backend application remains completely clean. The heavy lifting is handled by the OpenClaw OS Gateway.**

---

## ⚡ Why Use OpenClaw?

### 1. The "Steel Tube" (Constrained Decoding)
`.claw` compiles your `type` declarations into strict TypeBox JSON schemas. The OpenClaw execution gateway acts as a "Bouncer", physically preventing the LLM from outputting a token that does not perfectly conform to your structural constraints. If your agent is supposed to return a `SearchSummary`, the execution engine physically cannot return a malformed string.

### 2. Pure Deterministic Workflows
Forget ReAct loops that spiral out of control. OpenClaw lets you write massive, looping execution graphs with `for` loops, conditional `if/else`, and explicit `try/catch` error boundaries. The routing is deterministic; the AI only fills in the blanks.

### 3. Batteries-Included OS Sandboxing
You don't just get a compiler. You get the `OpenClaw Gateway`—an Agent Operating System. 
When your `.claw` script calls `Browser.search`, the Gateway natively spins up a Playwright headless browser for you. If you call `invoke: python("scripts.scraper")`, the Gateway spins up a secure, isolated Docker container to execute your code.

---

## 📖 The "Hello World" Example

```claw
import { WebScraper } from "./tools.claw"

type CompetitorData {
    url: string
    summary: string
    threat_level: int @max(10)
}

agent AnalystAgent {
    client = OpenAIClients.GPT_4O
    system_prompt = "You are an elite market analyst."
    tools = [WebScraper]
}

workflow AnalyzeCompetitor(target_url: string) -> CompetitorData {
    // 1. The Analyst executes the task, with the compiler proving it has the required tools.
    // 2. The TypeBox constrains the LLM to strictly return only valid `CompetitorData` JSON.
    let report: CompetitorData = execute AnalystAgent.run(
        task: "Analyze this competitor site.",
        params: { url: target_url },
        require_type: CompetitorData
    )
    
    return report
}
```

Then, in your **TypeScript Next.js Project**:
```typescript
import { OpenClawGateway } from "@openclaw/sdk"
import { AnalyzeCompetitor } from "./generated/claw"

// Natively execute the agent orchestration and get a perfectly typed result
const result = await AnalyzeCompetitor("https://apple.com", { client: new OpenClawGateway() })
console.log(result.threat_level) // Guaranteed to be an integer <= 10
```

---

## 📚 Documentation & Specifications

The internal mechanics of the `.claw` compiler and the Gateway architecture are meticulously documented for contributors. We enforce a strict **Test-Driven Development (TDD)** pipeline.

**Please read the specifications in sequential order before contributing:**
1. [Core Language Specification](specs/01-DSL-Core-Specification.md)
2. [Compiler Architecture](specs/02-Compiler-Architecture.md)
3. [Formal PEG Grammar](specs/03-Grammar.md)
4. [AST Structures (Rust)](specs/04-AST-Structures.md)
5. [Type System & Safety](specs/05-Type-System.md)
6. [SDK CodeGen (TS/Python)](specs/06-CodeGen-SDK.md)
7. [The OpenClaw OS Contract](specs/07-OpenClaw-OS.md)
8. [Testing Specifications](specs/08-Testing-Spec.md)
9. [Implementation Flow](specs/09-Implementation-Flow.md)
10. [Final GAN Architecture Audit](specs/10-GAN-Final-Audit.md)

---

## 🚀 Getting Started

If you are ready to write `.claw` code or hack on the Rust compiler itself, please review the developer guides:
- **[QUICKSTART.md](./QUICKSTART.md)**: How to set up the CLI, run tests, and execute your first workflow.
- **[PRODUCTION.md](./PRODUCTION.md)**: How to integrate the generated SDK into your CI/CD pipelines and scale the OpenClaw Gateway OS.

---

> *Code is a liability. Orchestration over configuration. Determinism over magic.*
