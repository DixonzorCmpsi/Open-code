# Claw Agent DSL (.claw) Core Specifications

## 1. Overview and Motivation

The Claw orchestration language (`.claw`) is a domain-specific language purposefully built to give developers **deterministic, programmable control over non-deterministic AI agents**.

Instead of writing orchestrations as brittle string prompts or unstructured JSON configurations, the `.claw` DSL treats Agents, Tools, and Events as strongly-typed objects in an execution graph. The language is designed to look native to C/Python developers while ensuring strict compile-time checks on state and tool usage boundaries.

## 2. Core Language Philosophies

1.  **AI execution is the "inside", the code is the "outside"**: The language wraps the agent. The agent does not generate the language. 
2.  **Determinism via Types**: Data jumping between agents or between a tool output and an agent prompt is strictly validated using BAML-like type enforcement.
3.  **Encapsulation via OOP**: Agents extend base agents to inherit personas, tools, and constraints.
4.  **First-class Multimodality**: Visual, audio, and browser operations are built-in primitives.

## 3. Structural Specifications

### 3.1 Types and Data Contracts
The lowest level of the language is the `type` declaration. This enforces what data shapes enter and exit tools or agent execution blocks.

```claw
// Types are similar to structs or interfaces
type SearchResult {
    url: string
    confidence_score: float
    snippet: string
    tags: list<string>
}
```

```claw
// Tools are the deterministic bridges to the real world.
// You can import native Claw tools directly:
import { WebScraper } from "@claw/tools.browser"
import { FileSystem } from "@claw/tools.fs"

// Or define your own tools that bridge to existing code:
tool AnalyzeSentiment(text: string) -> float {
    // Bridges to the actual execution layer (e.g. TS SDK or Gateway)
    invoke: module("scripts.analysis").function("get_sentiment")
}
```

### 3.3 Clients and Agents
To make `.claw` fully "Plug-and-Play", you first define a **Client**. A Client tells the compiler exactly which language model to connect to, what API key to use, and how to handle retries (similar to configuring a database connection).

```claw
// Standard Managed Providers
client FastOpenAI {
    provider = "openai"
    model = "gpt-4o-mini"
    retries = 3
}

// True Model-Agnostic Execution: Connect directly to local SLMs, Ollama, 
// vLLM, HuggingFace TGI, or ANY OpenAI-compatible endpoint.
client LocalOrCustomLLM {
    provider = "custom"
    model = "meta-llama/Llama-3.2-11B-Vision-Instruct"
    endpoint = env("CUSTOM_LLM_URL") // e.g. http://localhost:11434/v1
    api_key = env("CUSTOM_LLM_KEY")
}
```

Agents are encapsulated state machines. They configure the client, the system prompt constraints, and the tools they are allowed to use.

```claw
agent Researcher {
    // Attach the previously defined client connection
    client = FastOpenAI
    
    system_prompt = "You form exact hypotheses before executing tools."
    tools = [WebScraper, AnalyzeSentiment]
    
    settings = {
        max_steps: 5,
        temperature: 0.1
    }
}

// Inheritance allows for hierarchical sub-agents
agent SeniorResearcher extends Researcher {
    client = DeepResearchClaude
    tools += [FileSystem.write] 
}
```

## 4. Execution Specifications (Programmable Logic)

The primary goal of the DSL is to allow programmatic, deterministic looping and conditionals around agentic execution. According to industry best practices for agent DSLs, this includes native support for specific orchestration patterns.

### 4.1 Agent Orchestration Patterns
The `.claw` language must formally support these standard execution patterns: 
1. **Pipeline (Sequential Workflow):** Agents chain sequentially where the output of Agent A is strongly typed as the input to Agent B.
2. **Fan-Out / Fan-In (Parallel Execution):** Launching multiple agents simultaneously to process a list, then awaiting the aggregated results.
3. **Orchestrator-Worker:** A primary agent delegates tasks to highly specialized sub-agents based on the context.

### 4.2 Loops and Conditional Handoffs (Example)

Here is a Fan-Out / Pipeline pattern where a deterministic loop wraps non-deterministic agent runs.

```claw
workflow AnalyzeCompetitors(companies: list<string>) -> list<string> {
    let final_reports = []
    
    // Fan-Out/Pipeline Pattern inside a deterministic loop
    for (company in companies) {
        
        let findings: list<SearchResult> = []
        
        // Execute block: pipeline step 1 with explicit Runtime fault tolerance
        try {
            findings = execute Researcher.run(
                task: "Find the latest product announcements for ${company}",
                require_type: list<SearchResult>
            )
        } catch (e: AgentExecutionError) {
            log.error("Researcher failed to generate valid schema for ${company} after 3 retries: ${e.message}")
            // Fallback: Continue the loop without crashing the orchestration
            continue 
        }
        
        if (findings.length() == 0) {
            log.warn("No findings for ${company}")
            continue
        }
        
        // Block 2: Handoff data to the next agent in the chain. 
        // We explicitly pass the findings as JSON context, but NOT the conversation history of the Researcher.
        let report: string = execute SeniorResearcher.run(
            task: "Draft a 1-paragraph summary from these findings",
            context: findings 
        )
        
        // Alternatively, if SeniorResearcher needed the full chat history of the Researcher:
        // let session: Session = Researcher.get_session()
        // execute SeniorResearcher.run(task: "...", session: session)
        
        final_reports.append(report)
    }
    
    return final_reports
}
```

## 5. Event/Routing Specifications

The DSL can define listeners to trigger workflows conditionally based on OpenCode events.

```claw
listener OnSlackMessage(event: Events.Slack.Message) {
    if (event.channel == "research-requests") {
        // Kick off asynchronous workflows
        let result = await AnalyzeCompetitors(["OpenAI", "Anthropic"])
        event.reply(result)
    }
}
```

## 6. Why is this better? (The "Steel Tube" Analogy)

To understand why building this `.claw` language is so powerful, you have to understand how most AI agents work today, and why they fail so often.

### How Most Agents Work Today (The Driving Test)
Imagine you are trying to teach someone how to drive a car perfectly.
Today, most developers build agents by handing the AI a massive instruction manual (a "prompt"). They say: *"You are an AI assistant. You have access to a steering wheel, a gas pedal, and brakes. Think step-by-step. Now, parallel park the car."*

The AI reads the manual, thinks about it, and then tries to steer and hit the gas. Because the AI is basically just guessing the next word (it's probabilistic), sometimes it hits the gas instead of the brakes, and the car crashes. 

To fix this, developers try to build better instruction manuals ("prompt engineering") or train smaller, smarter AI models to be better drivers. But at the end of the day, you can never 100% guarantee the AI won't accidentally hit the gas.

### The Claw Way (The Steel Tube)
Claw fixes this by removing the steering wheel and gas pedals entirely. 

Instead of teaching the AI how to drive, Claw puts the AI's car inside a physical, unbendable **steel tube** on a roller-coaster track that goes exactly where you want it to go. 

**How does this work in code?**
Instead of writing a long prompt, the developer writes a single line of code in the `.claw` language:
```claw
tool SearchWeb(query: string, max_results: int)
```

Behind the scenes, the Claw Compiler (`clawc`) instantly translates that line into a rigid **TypeBox Schema** (a strictly formatted JSON blueprint) that looks exactly like this:
```json
{
  "type": "object",
  "properties": {
    "query": { "type": "string" },
    "max_results": { "type": "integer" }
  },
  "required": ["query", "max_results"]
}
```
This strict JSON structure *is* the Blueprint. It mathematically defines that the AI is only allowed to output two exact keys (`query` and `max_results`) matching specific data types.

**How does the server actually enforce the blueprint? (The Bouncer Analogy)**
To understand *how* the server forces the AI into the steel tube, you have to know how AI writes sentences. AI doesn't think in whole sentences; it guesses one word (or "token") at a time. It's like someone playing a guessing game: *"I like to eat..."* and the AI guesses *"apples"*.

When Claw sends its TypeBox blueprint to the AI server (like OpenAI or Claude), the server puts a "Bouncer" at the door of the AI's brain. 

Every single time the AI tries to guess the next word, the Bouncer checks the blueprint:
1. The AI thinks: *"The next word should be the number 5, at 90% probability!"*
2. The Bouncer looks at the blueprint. The blueprint says: *"Right now, we are in the 'query' section. The 'query' section MUST be text, not a number."*
3. The Bouncer says: *"Nope, you are not allowed to say 5. I am changing the probability of you saying 5 to 0%."*
4. The AI is forced to pick its second-best guess, which is text, and the Bouncer lets it through.

By wrapping the AI in the `.claw` language and these rigid TypeBox blueprints, we don't need the AI to be a perfect driver anymore. The "steel tube" (managed by the Bouncer) mathematically prevents the AI from crashing, turning a guessing game into a 100% perfect execution every single time.

### A Real Example
Let's say we defined this tool earlier:
```claw
tool WebScraper(target_url: string, max_pages: int) -> ScrapedData
```

In the background, the `.claw` language turns this into a strict blueprint. When the `Researcher` agent starts running and decides it needs to scrape Wikipedia, here is exactly what happens with the Bouncer:

1. **AI starts writing JSON:** `{"target_url": "https://wikipedia.org",`
2. **AI tries to guess next:** It wants to add the `max_pages` argument.
3. **AI's Bad Guess:** The AI starts to write the string `"five"`.
4. **The Bouncer steps in:** The Bouncer looks at the blueprint and sees `max_pages: int`. It says to the AI, *"You cannot type the quote mark `"` here. You can only type numbers `0-9`."*
5. **The Correction:** The AI's probability for typing `"` drops to 0%. It is physically forced to type the number `5` instead.

The resulting output is perfectly formatted `{"target_url": "https://wikipedia.org", "max_pages": 5}`, and the Claw system executes the scraper tool flawlessly.

### How does the Tube know when to turn on?
You might wonder: *Does the Bouncer check every single word the AI ever says? How does it know we are doing a tool call right now?*

The answer lies in how modern AI APIs (like OpenAI) are structured. When Claw talks to the AI, it doesn't just send one giant chat message. It sends a structured package with two distinct parts:
1. **The Chat Log:** (e.g., "User: Can you scrape this website?")
2. **The Tools List:** A special API field (called `tools` in OpenAI) where Claw attaches the TypeBox blueprints. 

When the AI receives this package, it makes a high-level choice: *"Should I reply with a normal message to the user, OR should I call a tool?"*

**If the AI chooses to reply to the user:** 
The Bouncer is essentially turned off. The AI is allowed to freely generate conversational probability tokens without strict TypeBox constraints. 

**If the AI chooses to call a tool:**
The AI sends a special flag back to its own server that says *"I am activating tool: WebScraper"*. 
**The exact millisecond** the AI makes that decision, the server activates the Bouncer for the `WebScraper` blueprint. The server shifts into "Constrained Decoding Mode" and begins forcing the AI to output the structured JSON arguments, token-by-token, until the JSON object is completely finished.

## 7. The Compiler and Execution OS

To make the `.claw` language work, two components are required:

### 7.1 The Compiler (`clawc`) — Built in Rust

`clawc` reads the `.claw` file, checks it for errors, and translates it into configuration files and SDK code that the developer can run.

**Why Rust?**

1. **Parser Ecosystem:** `winnow` and `pest` are the best modern parser combinator libraries. They parse `.claw` syntax into a typed AST with exact byte spans for error reporting — better than any Python or TypeScript alternative.

2. **Instant Execution:** `clawc` compiles to a standalone machine-code binary that runs in milliseconds, with no Node.js or Python VM startup overhead.

3. **Zero-Dependency Distribution:** Developers download a single binary (`claw` for their OS) with no required runtime. No `cargo`, no `npm`, no `pip`.

4. **Memory Safety:** Rust's borrow checker prevents crashes while processing user-authored `.claw` files, even adversarial inputs.

### 7.2 The Execution OS — OpenCode

**OpenCode** (`opencode.ai`) is the execution runtime for Claw workflows. It is an open-source AI coding agent used by 5M+ developers with support for 75+ LLM providers.

`clawc build --lang opencode` compiles `.claw` source into OpenCode's native configuration:

```
.claw source
    │
    clawc
    │
    ├── opencode.json              ← provider + MCP config
    ├── .opencode/agents/*.md      ← one per `agent` block
    ├── .opencode/commands/*.md    ← one per `workflow` block
    ├── generated/mcp-server.js    ← MCP server for all `tool` blocks
    └── generated/claw-context.md  ← project context document
```

OpenCode handles all runtime concerns: LLM invocation, session management, tool execution, streaming, and permission sandboxing. Claw's job is compile-time type safety and deterministic orchestration definition.

**Full architecture contract:** `specs/25-OpenCode-Integration.md`
**MCP server generation:** `specs/26-MCP-Server-Generation.md`

### Summary of the Flow
1. **The Language:** Rust for the `clawc` compiler.
2. **The Output:** OpenCode configuration + TypeScript/Python SDKs for programmatic use.
3. **The Engine:** OpenCode — the execution OS that runs agent workflows.

## 8. Managing AI Pitfalls (Context Limits & Garbage Collection)

To prevent severe runtime bottlenecks such as OOM (Out-of-Memory) errors or Token Limit Exhaustion, `.claw` dictates strict paradigms for managing both software RAM and LLM context.

**Systems Memory & Garbage Collection (RAM):**
Standard DSLs (like C) require manual memory management (`malloc` / `free`). Because `.claw` compiles directly into high-level AST environments like TypeScript/Node (V8) and Python (CPython), the runtime seamlessly inherits their heavily optimized automatic Garbage Collection (GC). You do not manually allocate or free primitive arrays; the OpenCode runtime (or the generated TypeScript/Python SDK runtime) purges out-of-scope variables automatically. 

**Context Window Pruning (LLM Tokens):**
While RAM is managed automatically, **LLM Token Context** is the true bottleneck in agent architecture. Instead of blindly handing off massive, infinite `Session` objects between agents and hitting the 128k token limit, `.claw` exposes native Memory module primitives.
```claw
// Safely truncate the conversation history to the last 8000 tokens before handoff
let safe_session = Memory.truncate(Researcher.get_session(), 8000)

execute SeniorResearcher.run(
    task: "Summarize",
    session: safe_session
)
```

**Semantic Guardrails on Types:**
Even if an LLM is forced by the Bouncer to output a string, it might hallucinate a bad string. You can append semantic constraints to `.claw` types:
```claw
type VerifiedUser {
    // Fails execution if the LLM hallucinates a non-email string
    email: string @regex("^[\\w-\\.]+@([\\w-]+\\.)+[\\w-]{2,4}$")
    age: int @min(18)
}
```

## 9. Nested Workflow Calls

Workflows can invoke other workflows directly as function calls:

```claw
workflow Inner(input: string) -> Label {
    let label: Label = execute Labeler.run(task: input, require_type: Label)
    return label
}

workflow Outer(name: string) -> Label {
    let result: Label = Inner(name)  // Direct workflow call
    return result
}
```

**Rules:**
- The compiler MUST verify that `Inner`'s declared `return_type` matches the assignment type (`Label`).
- Recursion depth is bounded at **10 levels** by default. The gateway enforces this at runtime.
- Each nested call gets its own `session_id` scoped under the parent: `{parent_session_id}:{workflow_name}:{crypto.randomUUID()}` (per `specs/12-Security-Model.md` — NEVER use timestamps for IDs).
- Nested calls are checkpointed independently and can be resumed separately.

---

## 10. API Integration (The Batteries-Included Experience)

While the `.claw` language has powerful constructs, its primary purpose is to be **embedded in your existing software**. The core logic of the language is designed to feel exactly like calling a standard function, identical to the BAML developer experience.

If you write a `.claw` workflow, you can easily wrap it in a frontend or access it via a standard REST API.

**Here is what that looks like in a standard Express (TypeScript) server:**

```typescript
// 1. You import the strictly-typed workflow generated by the `clawc` compiler
import { AnalyzeCompetitors } from "../generated/claw"
import { ClawClient } from "@claw/sdk"

import express from 'express'
const app = express()
app.use(express.json())

// 2. Connect to OpenCode (which handles the LLMs, Browsers, and Sandboxes internally)
const gateway = new ClawClient({ opencode: true })

// 3. You build your standard API endpoint
app.post('/api/research', async (req, res) => {
    const { companies } = req.body // e.g. ["Apple", "Microsoft"]
    
    try {
        // 4. You call the `.claw` workflow exactly like a normal async function.
        // You pass the gateway client so the SDK knows where to execute the tools.
        // You get 100% IDE intellisense and guaranteed type safety.
        const reports: string[] = await AnalyzeCompetitors(companies, { client: gateway })
        
        // 5. Send the result back to your React/Next.js frontend
        res.json({ success: true, nested_reports: reports })
        
    } catch (e) {
        res.status(500).send("Agent failed to complete task")
    }
})

app.listen(3000)
```

**The Core Takeaway:** The `.claw` language compiles down into highly-typed, deterministic standard functions that gracefully plug into your existing Next.js apps, FastAPI logic, or background workers. You never have to write browser puppeteer scripts or Python sandboxes yourself; OpenCode handles the heavy lifting via MCP tool servers, keeping your API clean.
