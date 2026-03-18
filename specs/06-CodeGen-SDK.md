# OpenClaw DSL: SDK Generation

Once the `.claw` code is parsed into an AST and validated by the Semantic Analyzer, the `clawc` compiler moves to Phase 3: Code Generation. This phase outputs the `.claw` workflows into standard, strictly-typed SDK files for use in the developer's application.

## 1. Generation Engine

The code generation will use `minijinja` (a Rust Jinja implementation). 
* The AST nodes and TypeBox schemas are injected into templated strings representing standard TypeScript and Python boilerplate.
* The output is written to a `generated/ claw` directory in the user's workspace.

## 2. Emitting TypeScript SDK Code

For a `.claw` file containing the `AnalyzeCompetitors` workflow, `clawc` will generate standard TypeScript interfaces and async functions.

**Original `.claw`:**
```claw
workflow AnalyzeCompetitors(company: string) -> SearchResult { ... }
```

**Generated `claw/index.ts`:**
```typescript
import { OpenClawClient, AgentExecutionError } from "@openclaw/sdk";

// 1. The emitted Types (using Zod or pure Interfaces)
export interface SearchResult {
    url: string;
    confidence_score: number;
    snippet: string;
    tags: string[];
}

// 2. The emitted Workflow Function
export const AnalyzeCompetitors = async (
    company: string, 
    options: { client: OpenClawClient, resumeSessionId?: string }
): Promise<SearchResult> => {
    
    // The emitted function communicates with the Heavy Backend Gateway 
    // to manage the actual agent execution loop or resume from a crash.
    const result = await options.client.executeWorkflow({
        workflowName: "AnalyzeCompetitors",
        arguments: { company },
        resumeSessionId: options.resumeSessionId
    });
    
    // 3. The Runtime Boundary validation (Zod validation of the Gateway result)
    // Ensures the typescript types EXACTLY MATCH the gateway output.
    return result as SearchResult;
}
```

## 3. Emitting Python SDK Code

The identical process applies for Python, generating `Pydantic` models instead of TypeScript interfaces.

**Original `.claw`:**
```claw
workflow AnalyzeCompetitors(company: string) -> SearchResult { ... }
```

**Generated `claw/__init__.py`:**
```python
from pydantic import BaseModel
from openclaw_sdk import OpenClawClient
from typing import List

# 1. The emitted Pydantic Models
class SearchResult(BaseModel):
    url: str
    confidence_score: float
    snippet: str
    tags: List[str]

# 2. The emitted Workflow Function
async def analyze_competitors(company: str, client: OpenClawClient) -> SearchResult:
    # 3. Call the heavy Gateway for execution
    result_dict = await client.execute_workflow(
        workflow_name="AnalyzeCompetitors", 
        arguments={"company": company}
    )
    
    # 4. Enforce Pydantic validation on the result
    return SearchResult(**result_dict)
```

## 4. The Gateway Communication Contract

When the generated SDK executes, it serializes the workflow request into standard JSON and sends it over WebSockets to the `openclaw-gateway` (which acts as the operating system for agent execution).

The SDK is purely a lightweight router; the complex task of spinning up Playwright browsers, managing Docker-based Python scripts, and enforcing TypeBox constrained decoding via OpenAI is handled by the OpenClaw Gateway.
