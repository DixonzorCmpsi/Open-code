# Spec 45: Web Research Pipeline — OpenClaw Patterns + GPT Researcher + Spec-Driven Synthesis

**Status:** SPECCED 2026-03-20. Supersedes the stub `scripts/search.mjs` approach.
**Depends on:** Spec 32 (synthesis pipeline), Spec 42 (OpenCode engine), Spec 43 (headless interface), Spec 44 (NER+BAML)

---

## 0. What This Spec Covers

Three additions that work together:

1. **OpenClaw browser patterns** — adopt the three-tier fetcher system (Fetcher → DynamicFetcher → StealthyFetcher) from the OpenClaw ecosystem so Claw tools can do real web operations without detection
2. **GPT Researcher at compound time** — after the synthesis pass generates tool code, GPT Researcher iterates toward the best artifact via a gradient-descent-style loop; also available as a `research {}` block at runtime
3. **Spec markdown compilation** — `.claw` files compile to per-component markdown spec files; these specs carry the user's declared variables/args into the synthesis prompts, and the `.claw` return type defines both the validation gate AND the output format

---

## 1. OpenClaw Browser Patterns

### 1.1 The three-tier fetcher

OpenClaw's ecosystem (specifically `openclaw-ultra-scraping` / Scrapling) exposes:

| Tier | Class | When to use |
|---|---|---|
| 1 | `Fetcher` | Static HTML, no JS, no bot protection |
| 2 | `DynamicFetcher` | JavaScript-rendered pages |
| 3 | `StealthyFetcher` | Cloudflare, Turnstile, aggressive anti-bot |

The principle: start with the lightest tier, escalate automatically on failure. Never start with StealthyFetcher (slower, heavier) when Fetcher works.

### 1.2 How this maps to `.claw` tool declarations

```
// Tier 1 — static page fetch
tool FetchPage(url: string) -> PageContent {
    using: fetch
}

// Tier 2 — JS-rendered page
tool FetchDynamic(url: string) -> PageContent {
    using: playwright
}

// Tier 3 — stealth (Cloudflare-resistant)
tool FetchStealth(url: string) -> PageContent {
    using: playwright
    synthesize {
        strategy: "stealth"
        note:     "Use StealthyFetcher from Scrapling — bypasses Cloudflare Turnstile"
    }
}

// Image fetch — downloads to disk, returns path
tool FetchImage(url: string, dest: string) -> ImageFile {
    using: fetch
    synthesize {
        note: "Download binary content, write to dest path, return { path, size_bytes, mime_type }"
    }
}
```

### 1.3 Generated Python tool implementations

When the synthesis pass processes a `using: playwright` tool with `strategy: "stealth"`, it generates:

```python
# generated/tools/FetchStealth.py — SYNTHESIZED
from scrapling import StealthyFetcher

async def FetchStealth(inputs: dict) -> dict:
    url = inputs["url"]
    page = StealthyFetcher.fetch(
        url,
        headless=True,
        network_idle=True,
        block_images=False,
        disable_resources=False,
    )
    return {
        "url":     url,
        "html":    page.html,
        "title":   page.find("title").text if page.find("title") else "",
        "status":  page.status,
    }
```

For the TypeScript path, ClawBird is used:

```typescript
// generated/tools/FetchStealth.ts — SYNTHESIZED
import { CustomBrowserDriver } from 'clawbird';

export async function FetchStealth(inputs: { url: string }): Promise<PageContent> {
  const driver = new CustomBrowserDriver();
  await driver.start({ name: 'claw-stealth' });
  const tab = await driver.open(inputs.url);
  const snapshot = await driver.snapshot(tab.targetId);
  await driver.stop();
  return {
    url:   inputs.url,
    html:  snapshot.html,
    title: snapshot.title ?? '',
  };
}
```

### 1.4 Web search — real implementation

The `WebSearch` tool is the most common. Synthesized implementation uses DuckDuckGo (no key) with automatic fallback to Scrapling for JS-heavy results pages:

```python
# generated/tools/WebSearch.py — SYNTHESIZED
import urllib.request, urllib.parse, json
from scrapling import Fetcher, StealthyFetcher

async def WebSearch(inputs: dict) -> dict:
    query = inputs["query"]
    params = urllib.parse.urlencode({"q": query, "format": "json", "no_html": "1"})
    url = f"https://api.duckduckgo.com/?{params}"

    try:
        req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
        with urllib.request.urlopen(req, timeout=8) as r:
            data = json.loads(r.read())
    except Exception:
        # Fallback: StealthyFetcher
        page = StealthyFetcher.fetch(url)
        data = json.loads(page.html)

    top_url     = data.get("AbstractURL") or (data.get("Results") or [{}])[0].get("FirstURL", "")
    snippet     = data.get("Abstract")    or (data.get("Results") or [{}])[0].get("Text", "")
    confidence  = 0.9 if snippet else 0.4

    return {"url": top_url, "snippet": snippet, "confidence_score": confidence}
```

### 1.5 Image artifact type

A new artifact format `"image"` is added alongside `json`, `markdown`, `text`:

```
workflow FindPurse(style: string, color: string) -> PurseReport {
    artifact {
        format = "image"
        path   = "~/Desktop/purse-finds/${color}-${style}.jpg"
    }
    ...
}
```

When `format = "image"`, the artifact block downloads the `url` field from the result and saves it as a binary file. The compiled artifact save code:

```python
if result_url := getattr(result, "url", None) or (result.get("url") if isinstance(result, dict) else None):
    import urllib.request
    urllib.request.urlretrieve(result_url, str(_artifact_path))
```

---

## 2. GPT Researcher Integration

### 2.1 Where GPT Researcher sits

Two positions in the pipeline:

**Position A — Synthesis time (compound time):**
After OpenCode generates tool TypeScript/Python, GPT Researcher runs a research pass to validate the implementation approach. It researches the API, library, or pattern being implemented and feeds findings back as synthesis context. This is the "gradient descent toward best artifact" the user described.

**Position B — Runtime (as a `research {}` block in `.claw`):**
At the END of a compound workflow, a `research {}` block calls GPT Researcher to do deep web research on the result, enriching the artifact with citations and external validation.

### 2.2 DSL addition — `research {}` block

```
workflow FindBestPurse(style: string, color: string, brand: string) -> PurseResearchReport {
    artifact {
        format = "json"
        path   = "~/Desktop/purse-research/${brand}-${color}-${style}.json"
    }

    // Step 1: get initial search result
    let search: SearchResult = execute Searcher.run(
        task: "Find ${color} ${style} purse by ${brand}",
        require_type: SearchResult
    )

    // Step 2: reason about match quality
    reason {
        using:       Analyst
        input:       search
        goal:        "Evaluate this product against criteria: style=${style}, color=${color}, brand=${brand}"
        output_type: PurseVerdict
        bind:        verdict
    }

    // Step 3: deep research on the final result — GPT Researcher
    research {
        query:        "${brand} ${color} ${style} purse review authenticity price"
        input:        verdict
        depth:        "standard"           // "quick" | "standard" | "deep"
        output_type:  PurseResearchReport
        bind:         result
    }

    return result
}
```

### 2.3 How `research {}` compiles

The `research {}` block compiles to a call to GPT Researcher:

```python
# Generated by Claw compiler for research {} block
from gpt_researcher import GPTResearcher as _GPTResearcher

_researcher = _GPTResearcher(
    query="${brand} ${color} ${style} purse review authenticity price",
    report_type="research_report",
    verbose=False,
)
await _researcher.conduct_research()
_raw_report = await _researcher.write_report()

# Inject input context (the verdict from reason {}) + research report
_research_context = json.dumps(verdict.model_dump() if hasattr(verdict, "model_dump") else verdict, indent=2)
_research_task = f"Given this prior analysis:\n{_research_context}\n\nAnd this research report:\n{_raw_report}\n\nSynthesize into the required output type."
_raw_result = await _run_agent_analyst(_research_task)
result = PurseResearchReport.model_validate(
    json.loads(_raw_result) if isinstance(_raw_result, str) else _raw_result
)
```

### 2.4 Synthesis time — GPT Researcher as gradient descent

At synthesis (Stage 2), before OpenCode generates tool code, GPT Researcher runs a targeted research query about the implementation:

```
Tool: WebSearch
Capability: fetch
Query GPT Researcher researches: "DuckDuckGo API Python implementation best practices 2026 rate limits response format"
```

GPT Researcher returns a research report → this becomes additional context in the synthesis prompt sent to OpenCode:

```
═══ RESEARCH CONTEXT (auto-generated) ═══
Source: GPT Researcher — query: "DuckDuckGo API Python..."

[2000-word report with citations about DuckDuckGo API, rate limits,
 response format, common pitfalls, working examples...]
═══════════════════════════════════════
```

OpenCode now generates code against current, real documentation — not its training data cutoff.

The "gradient descent" loop:
1. GPT Researcher researches the implementation pattern
2. OpenCode synthesizes code using research context
3. Contract tests run
4. If tests fail → GPT Researcher researches the FAILING pattern specifically
5. OpenCode re-synthesizes with updated context
6. Repeat up to 3x

---

## 3. Spec Markdown Compilation

### 3.1 The idea

Every `.claw` declaration compiles to a markdown spec file. These spec files are the bridge between the user's declarative intent and the synthesis prompt. They carry:
- The component's purpose (from type/tool/agent/workflow name and comments)
- The user's declared variables, args, constraints
- The return type schema
- The test cases

### 3.2 Output structure

```
generated/
├── specs/                          ← NEW: markdown specs per component
│   ├── types/
│   │   ├── PurseReport.md
│   │   └── SearchResult.md
│   ├── tools/
│   │   ├── WebSearch.md
│   │   └── FetchImage.md
│   ├── agents/
│   │   ├── Searcher.md
│   │   └── Analyst.md
│   └── workflows/
│       └── FindPurse.md
├── tools/                          ← synthesized TypeScript/Python
├── workflows/                      ← generated deterministically
└── synthesis-report.md
```

### 3.3 Spec markdown format

Each spec file is the canonical description of one component. Example:

```markdown
# Tool: WebSearch

**Source:** examples/purse_finder.claw
**Compiled:** 2026-03-20T18:00:00Z

## Signature
```
WebSearch(query: string) -> SearchResult
```

## Return Type: SearchResult
| Field | Type | Constraints |
|---|---|---|
| url | string | non-empty |
| snippet | string | non-empty |
| confidence_score | float | range [0.0, 1.0] |

## Implementation Capability
`fetch` — HTTP request, no browser required

## User Variables That Flow Into This Tool
- `style` (string) — purse style, e.g. "tote"
- `color` (string) — purse color, e.g. "tan"
- `brand` (string) — brand name, e.g. "Coach"

The query argument is typically constructed from these: `"${color} ${style} purse by ${brand}"`

## Test Cases
| Input | Expected |
|---|---|
| `{ query: "Coach tan tote" }` | `url != "", confidence_score in [0,1]` |

## Synthesis Notes
- No API key required (DuckDuckGo)
- Fall back to StealthyFetcher if standard HTTP fails
- Return type must match SearchResult schema exactly
```

### 3.4 User variables carried into specs

When a workflow declares `FindPurse(style: string, color: string, brand: string)`, the compiler traces which arguments flow into each tool/agent call and records them in the spec. The synthesis prompt then knows concretely what values these will be at runtime — giving the synthesizer better context for generating realistic examples and edge cases.

### 3.5 Return type as validation gate

The `.claw` return type declaration serves three roles at once:

**Role 1 — Type constraint:** The synthesized code validates its output against this schema
**Role 2 — Test spec:** Contract tests are auto-generated from the return type fields and constraints
**Role 3 — GPT Researcher output format:** When `research {}` runs, the output format template given to GPT Researcher is derived from the return type. GPT Researcher is told to structure its synthesis into `PurseResearchReport` fields — ensuring the research output is machine-parseable, not a freeform report.

```python
# Return type drives GPT Researcher format
format_template = "\n".join([
    f"- {field.name} ({field.type}): ..."
    for field in PurseResearchReport.__fields__.values()
])
_researcher = _GPTResearcher(
    query=query,
    report_format=format_template,  # constrained to declared return type
    ...
)
```

---

## 4. GAN Audit

### Gaps

**G1: Scrapling requires a virtual environment.**
Scrapling uses compiled C extensions (for browser fingerprinting). Cannot be installed system-wide in all environments. The synthesis runner needs to check for Scrapling availability and emit a clear error if absent.

**G2: GPT Researcher requires API keys.**
GPT Researcher uses LLM providers (OpenAI by default, configurable). At synthesis time, this means a second LLM API key is needed beyond the one for synthesis. At runtime, the `research {}` block needs the key. Claw must surface this requirement clearly rather than failing silently.

**G3: `research {}` output size is large.**
GPT Researcher generates 2000+ word reports. Passing this to an agent runner (which has its own context window) risks token limit errors. The compiled `research {}` block must truncate or summarize the report before injecting it.

**G4: Synthesis-time GPT Researcher adds latency.**
Running GPT Researcher before each synthesis adds 30-120s per tool. This makes synthesis even slower. The synthesis cache must include the GPT Researcher output so it's not re-run for unchanged tools.

**G5: Image artifact format requires URL in result.**
The `format = "image"` artifact type assumes the return type has a `url: string` field. If the workflow returns a type without a URL, the image download silently fails. The compiler should validate this at compile time: `artifact { format = "image" }` requires return type to have a `url` field.

**G6: Stealth fetching and terms of service.**
StealthyFetcher bypasses bot detection. This is technically equivalent to what a human browser does, but violates the ToS of many sites. Claw's documentation must note that `strategy: "stealth"` is the user's responsibility with respect to target site ToS.

### Assumptions

**A1: `pip install gpt-researcher` works cleanly.**
GPT Researcher has many dependencies. Need to verify it installs without conflicts with pydantic/httpx versions already required.

**A2: Scrapling's StealthyFetcher is synchronous.**
The current Python codegen wraps tool calls with `run_in_executor`. Scrapling's sync API fits this pattern. Need to verify it doesn't block the event loop unacceptably.

**A3: ClawBird works with Playwright installed globally.**
The TypeScript path for stealth tools uses ClawBird which requires a Playwright-compatible browser binary. `npx playwright install chromium` must be part of the setup.

### Downstream consequences

**N1: The synthesis → research loop creates a feedback cycle.**
If GPT Researcher researches the wrong thing, synthesis produces wrong code, which then researches the wrong thing on retry. Need clear termination: max 3 GPT Researcher passes per tool regardless of failures.

**N2: Spec markdown files become the source of truth.**
Once spec markdown files exist, developers may want to EDIT them directly to guide synthesis. This is valid and should be supported: if a spec file exists and is newer than the source `.claw`, use it as-is rather than re-generating.

**N3: Return type as output format constrains GPT Researcher.**
Making GPT Researcher emit structured output (matching the `.claw` return type) means the research report is less readable as a human document. There's a tension between machine-parseable output and readable research. Resolution: GPT Researcher generates BOTH — a `content: string` field with the full report AND structured fields matching the return type.

---

## 5. New DSL Elements Required

### `research {}` block grammar

```
research_stmt = {
    "research" ~ "{" ~
    ("query"       ~ ":" ~ string_or_template  ~ ","?) ~
    ("input"       ~ ":" ~ identifier          ~ ","?)? ~
    ("depth"       ~ ":" ~ research_depth      ~ ","?)? ~
    ("output_type" ~ ":" ~ data_type           ~ ","?) ~
    ("bind"        ~ ":" ~ identifier          ~ ","?) ~
    "}"
}
research_depth = { "quick" | "standard" | "deep" }
```

### `synthesize {}` block addition — `strategy` and `note`

```
synthesize_block = {
    "synthesize" ~ "{" ~
    ("strategy" ~ ":" ~ string_lit ~ ","?)? ~
    ("note"     ~ ":" ~ string_lit ~ ","?)? ~
    "}"
}
```

### `artifact` format addition — `"image"`

```
artifact_format = { "json" | "markdown" | "text" | "html" | "image" }
```

Validation rule: `format = "image"` requires the workflow return type to have a `url: string` field (enforced in semantic pass).

---

## 6. Implementation Stages

### Stage A — Real web tools (unblocks current testing)
- Write `scripts/search.py` (DuckDuckGo, real — already done)
- Write `scripts/fetch_page.py` (Scrapling DynamicFetcher)
- Write `scripts/fetch_image.py` (download binary to path)
- Add `format = "image"` to artifact codegen in `python.rs` and `typescript.rs`
- Add semantic validation: `format = "image"` requires `url` field in return type

### Stage B — `research {}` block
- Add `Research` variant to `Statement` enum in `ast.rs`
- Add `research_stmt` parser
- Emit GPT Researcher call in `python.rs` codegen
- Semantic validation: `output_type` in `research {}` must match workflow's declared return type or be a subtype

### Stage C — Spec markdown compilation
- Add `claw compile --specs` flag that emits `generated/specs/*.md`
- Per-component spec format as described in §3.3
- Variable tracing: which workflow args flow into which tool calls

### Stage D — Synthesis-time GPT Researcher
- In `synth_runner.rs`, before building synthesis prompt, run GPT Researcher for each tool
- Cache result keyed on `(tool_spec_hash, gpt_researcher_query)`
- Inject research report into synthesis prompt as `═══ RESEARCH CONTEXT ═══` section

### Stage E — Image artifact type
- Implement `format = "image"` download in artifact codegen
- Semantic check: return type must have `url: string`

---

## 7. Implementation Prompt for Stage A (unblocks now)

```
TASK: Implement Stage A of Spec 45 — real web tools for the Claw language.

Repository: /Users/dixon.zor/Documents/Open-code

FILES TO CREATE (do not modify any existing files):

1. scripts/fetch_page.py
   - Function: run(url: str) -> dict
   - Try standard urllib.request first (fast path)
   - If content is empty or returns error, fall back to Scrapling DynamicFetcher
   - If Scrapling not installed, fall back to urllib with JS-disabled warning
   - Return: { url, title, content (text, not HTML), word_count, status_code }

2. scripts/fetch_image.py
   - Function: run(url: str, dest: str = None) -> dict
   - Downloads image from url to dest path (or ~/Downloads/claw-images/ if no dest)
   - Creates parent directories if needed
   - Returns: { url, path (absolute), size_bytes, mime_type, width, height (if PIL available) }

3. Update scripts/search.py (the REAL one already written — verify it works, do not regress)

FILES TO MODIFY:

4. src/codegen/python.rs — add "image" format handling in the artifact block:
   In the match on artifact.format, add arm:
   "image" => code that downloads result.url to the artifact path using urllib

5. src/codegen/typescript.rs — same for TypeScript:
   Add "image" arm that generates fetch() + fs.writeFile() for binary download

6. src/ast.rs — no changes needed (ArtifactSpec already has format: String)

7. src/semantic/mod.rs — add validation:
   If artifact.format == "image", check that workflow return type has a field named "url"
   Emit a SemanticError if not.

CONSTRAINTS:
- Do not modify any parser code
- Do not modify python.rs except the artifact format match
- Do not modify typescript.rs except the artifact format match
- All existing tests must continue to pass (cargo test)
- scripts/*.py must work with Python 3.9+ (system Python on macOS)
- Scrapling is OPTIONAL — degrade gracefully if not installed

VERIFICATION:
1. cargo test — all 91 tests pass
2. python3 scripts/fetch_page.py (manual test, prints result)
3. python3 scripts/fetch_image.py (downloads a test image)
4. Write a new .claw file examples/image_finder.claw that uses FetchImage tool with artifact { format = "image" }
5. claw build --lang python examples/image_finder.claw && python3 examples/generated/claw/__init__.py FindImage --arg query="Coach tan tote bag"
6. Verify image file appears at ~/Desktop/claw-images/Coach-tan-tote-bag.jpg
```
