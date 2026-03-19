# Claw DSL: The Deterministic Agent Compiler

<div align="center">
  <h3>Code -> AI, not AI -> Code</h3>
  <p>An enterprise-grade, statically-typed compiler and DSL for orchestrating multi-agent topologies.</p>
</div>

---

## Purpose & Goals

`Claw` is a declarative, object-oriented ecosystem for deterministic multi-agent orchestration. It was built to solve the brittle, hallucination-prone nature of writing raw Python or TypeScript scripts to chain arbitrary LLM calls together.

If you are currently chaining agents using pure dictionaries, regex parsing, and massive string prompts, you are building a liability. The overarching goal of Claw is to replace "magic" with **mathematical type safety**. 

The `.claw` compiler (`clawc`) allows developers to define Agents, Tools, and Workflows with absolute strictness. It parses your architecture, guarantees your tools match your agent requests, and outputs auto-generated SDKs (TypeScript and Python) that you simply `import` into your production code. Your backend application remains completely clean while the heavy lifting is handled by the deterministic Claw OS Gateway.

---

## Architecture

The Claw ecosystem is divided into three primary isolated components:

### 1. The Compiler (`clawc`)
Built in **Rust** for maximum performance and safety, the compiler is responsible for:
- Parsing `.claw` DSL files using strict `winnow` combinators.
- Running a 3-pass semantic type-checking engine to guarantee valid assignments and relationships.
- Emitting intermediate representations (IR) and generating type-safe SDK clients using `minijinja`.
- Hashing the AST to ensure the executed SDK perfectly matches the deployed runtime.

### 2. The Execution OS (`claw-gateway`)
Built in **TypeScript**, the Gateway acts as the physical runtime environment - or "Operating System" - for your agents.
- **Event Sourcing & Checkpointing**: The Gateway records *every* successful AST node execution (loops, conditions, statements) into a persistent store (SQLite or Redis). If an agent crashes half-way through a massive workflow, the OS can seamlessly resume exactly where it left off.
- **The Bouncer (Constrained Decoding)**: The OS physically prevents the LLM from outputting tokens that don't perfectly conform to the TypeBox JSON schemas compiled from your script. 
- **Tool Sandboxing**: When your code calls `Browser.search`, the Gateway natively spins up a Playwright headless browser for visual intelligence. When you invoke `python()`, it spins up a secure, isolated Docker container to execute untrusted code.

### 3. The Integration Layer
Claw natively orchestrates LLM completion requests through integrations like **BAML**, managing prompt injection, provider fallbacks, and multi-modal interactions.

---

## Why Use Claw?

- **Pure Deterministic Workflows**: Forget erratic ReAct loops that spiral out of control. Claw lets you write massive, looping execution graphs with `for` loops, conditional `if/else`, and explicit `try/catch` error boundaries. The routing is deterministic; the AI only fills in the blanks.
- **Schema Degradation Prevention**: The gateway intelligently detects and rejects "lazy" LLM responses (e.g., syntactically valid but semantically empty JSON payloads) using advanced structure validation.
- **Built-in Mocking & Testing**: Claw ships with a robust test runner (`claw test`) and first-class Mock block support so you can perform TDD on your agent architectures without making expensive live LLM requests.

---

## The "Hello World" Example

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
import { ClawClient } from "@claw/sdk"
import { AnalyzeCompetitor } from "./generated/claw"

// Natively execute the agent orchestration and get a perfectly typed result
const result = await AnalyzeCompetitor({
    client: new ClawClient({ endpoint: "http://127.0.0.1:8080" }),
    resumeSessionId: undefined
}, "https://apple.com")

console.log(result.threat_level) // Guaranteed to be an integer <= 10
```

---

## Documentation & Specifications

The internal mechanics of the `.claw` compiler and the Gateway architecture are meticulously documented for contributors. We enforce a strict **Test-Driven Development (TDD)** pipeline.

**Please read the specifications in sequential order before contributing:**
1. [Core Language Specification](specs/01-DSL-Core-Specification.md)
2. [Compiler Architecture](specs/02-Compiler-Architecture.md)
3. [Formal PEG Grammar](specs/03-Grammar.md)
4. [AST Structures (Rust)](specs/04-AST-Structures.md)
5. [Type System & Safety](specs/05-Type-System.md)
6. [SDK CodeGen (TS/Python)](specs/06-CodeGen-SDK.md)
7. [The Claw OS Contract](specs/07-Claw-OS.md)
8. [Testing Specifications](specs/08-Testing-Spec.md)
9. [Implementation Flow](specs/09-Implementation-Flow.md)
10. [Final GAN Architecture Audit](specs/10-GAN-Final-Audit.md)

---

## Getting Started

If you are ready to write `.claw` code or hack on the Rust compiler itself, please review the developer guides:
- **[QUICKSTART.md](./QUICKSTART.md)**: How to set up the CLI, run tests, and execute your first workflow.
- **[PRODUCTION.md](./PRODUCTION.md)**: How to integrate the generated SDK into your CI/CD pipelines and scale the Claw Gateway OS.

---

> *Code is a liability. Orchestration over configuration. Determinism over magic.*
