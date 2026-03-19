# Spec 28: Claw DSL — Desired Use Cases & Product Vision

**Status:** ACTIVE
**Purpose:** Defines the end-product vision, target user, and concrete use cases that every implementation decision must serve. This is the "why" behind every spec.

---

## 0. The Product in One Sentence

Claw is **N8N as code** — a statically-typed, deterministic orchestration language that compiles to OpenCode workflows, letting developers program multi-agent pipelines with the same confidence as writing a shell script but with the power of 75+ LLM providers, browser automation, file systems, and arbitrary CLI tools.

---

## 1. The Mental Model

OpenCode is the execution OS — it can be driven entirely from the command line, has a rich agent/command/MCP architecture, and supports dozens of providers. But raw OpenCode configuration is:
- **Imperative and manual** — you write markdown prompts and JSON config by hand
- **Untyped** — no compile-time guarantee that agent A's output matches agent B's input
- **Non-deterministic by default** — no programmatic control flow, branching, or retry logic
- **Not composable** — no way to build libraries of reusable typed agents and tools

Claw fixes all of this. The relationship is:

```
Claw DSL       : OpenCode  =  SQL          : a database engine
Claw DSL       : OpenCode  =  Terraform    : cloud infrastructure
Claw DSL       : OpenCode  =  Dockerfile   : container runtime
```

A developer writes `.claw` source. The `clawc` compiler verifies types, validates agent boundaries, and emits the OpenCode config. OpenCode executes it. The developer never writes JSON config or markdown prompts by hand.

---

## 2. Target User

| User | Background | What Claw Gives Them |
|------|-----------|----------------------|
| **Senior developer** | Knows their toolchain | Deterministic multi-step automation with type safety. Replace fragile bash scripts with typed agent workflows. |
| **Platform/DevOps engineer** | Manages CI/CD, infra, releases | Codified release engineering, dependency audits, environment bootstrapping as a typed program. |
| **ML/AI engineer** | Builds LLM-powered products | Structured LLM orchestration with compile-time type guarantees between pipeline stages. |
| **Startup founder** | Non-specialist operator | Automate repetitive multi-tool workflows (GitHub → Jira → Slack → deploy) without custom code. |

---

## 3. Core Use Cases

### 3.1 Dev Environment Bootstrap

**The scenario:** A new team member joins. Their machine needs a complete dev environment set up from scratch: tools installed, repos cloned, IDE configured, secrets injected from the team vault.

**What a human does manually (30–60 min):**
1. Check if Homebrew/nvm/Docker is installed
2. Clone N repos
3. Install VS Code (if missing) and specific extensions
4. Set up `.env` files from 1Password/Vault
5. Configure git identity, SSH keys, GPG
6. Run the project's bootstrap script

**The Claw program:**
```claw
type ToolStatus {
    name: string
    installed: boolean
    version: string
}

type EnvSecret {
    key: string
    value: string
    source: string
}

tool CheckTool(name: string) -> ToolStatus {
    invoke: module("scripts/system").function("checkTool")
}

tool InstallBrew(package: string) -> ToolStatus {
    invoke: module("scripts/system").function("installBrew")
}

tool CloneRepo(url: string, path: string) -> boolean {
    invoke: module("scripts/git").function("cloneIfMissing")
}

tool LoadSecret(key: string, vault: string) -> EnvSecret {
    invoke: module("scripts/vault").function("fetchSecret")
}

tool OpenVSCode(path: string) -> boolean {
    invoke: module("scripts/ide").function("openVSCode")
}

tool InstallVSCodeExtension(id: string) -> boolean {
    invoke: module("scripts/ide").function("installExtension")
}

client DevAgent {
    provider = "anthropic"
    model = "claude-sonnet-4-6"
}

agent EnvironmentSetup {
    client = DevAgent
    system_prompt = "You set up developer environments. Follow the bootstrap plan exactly. Report each step with status."
    tools = [CheckTool, InstallBrew, CloneRepo, LoadSecret, OpenVSCode, InstallVSCodeExtension]
    settings = { max_steps: 30, temperature: 0.0 }
}

workflow BootstrapDevMachine(repo_url: string, project_name: string) -> ToolStatus {
    // Check and install VS Code
    let vscode: ToolStatus = execute EnvironmentSetup.run(
        task: "Check if VS Code is installed. If not, install it via brew install --cask visual-code.",
        require_type: ToolStatus
    )

    // Install required extensions
    let _ = execute EnvironmentSetup.run(
        task: "Install these VS Code extensions: ms-python.python, rust-lang.rust-analyzer, bradlc.vscode-tailwindcss",
        require_type: ToolStatus
    )

    // Clone the repo
    let _ = execute EnvironmentSetup.run(
        task: "Clone ${repo_url} into ~/projects/${project_name} if not already present.",
        require_type: ToolStatus
    )

    // Load secrets from vault and write .env
    let api_key: EnvSecret = execute EnvironmentSetup.run(
        task: "Pull ANTHROPIC_API_KEY and DATABASE_URL from vault path 'team/${project_name}' and write them to ~/projects/${project_name}/.env",
        require_type: EnvSecret
    )

    // Open project in VS Code
    let result: ToolStatus = execute EnvironmentSetup.run(
        task: "Open ~/projects/${project_name} in VS Code",
        require_type: ToolStatus
    )

    return result
}
```

**What compiles:** OpenCode command `/BootstrapDevMachine`, one MCP server exposing all 6 tools, an agent config with temperature 0.0 (pure determinism), typed boundaries at each step.

**OpenCode invocation:**
```bash
opencode /BootstrapDevMachine "https://github.com/myorg/myapp" "myapp"
```

---

### 3.2 Feature Implementation Across a Repository

**The scenario:** A product manager has a list of features. A developer wants to delegate the implementation to an agent: open the repo in VS Code, add the features one by one, running tests after each, committing with a meaningful message, then opening a PR.

**The Claw program:**
```claw
type Feature {
    name: string
    description: string
    acceptance_criteria: list<string>
}

type PullRequest {
    url: string
    branch: string
    title: string
    tests_passed: boolean
}

type CommitResult {
    sha: string
    message: string
    files_changed: int
}

tool GitCheckout(branch: string) -> boolean {
    invoke: module("scripts/git").function("checkout")
}

tool RunTests(path: string) -> boolean {
    invoke: module("scripts/test").function("run")
}

tool GitCommit(message: string, files: list<string>) -> CommitResult {
    invoke: module("scripts/git").function("commit")
}

tool OpenPullRequest(branch: string, title: string, body: string) -> PullRequest {
    invoke: module("scripts/github").function("openPR")
}

client Engineer {
    provider = "anthropic"
    model = "claude-sonnet-4-6"
}

agent FeatureAgent {
    client = Engineer
    system_prompt = "You are a senior engineer implementing features. Write clean, tested code. Follow the project's existing patterns. Never skip tests."
    tools = [GitCheckout, RunTests, GitCommit, OpenPullRequest]
    settings = { max_steps: 50, temperature: 0.1 }
}

workflow ImplementFeatures(features: list<Feature>, repo_path: string) -> PullRequest {
    // Create feature branch
    let _ = execute FeatureAgent.run(
        task: "Create and checkout a new branch named 'feat/agent-batch-${features[0].name}' in ${repo_path}",
        require_type: CommitResult
    )

    // Implement each feature sequentially
    for feature in features {
        let commit: CommitResult = execute FeatureAgent.run(
            task: "Implement '${feature.name}': ${feature.description}. Acceptance criteria: ${feature.acceptance_criteria}. Run tests after implementation. Commit the changes with a descriptive message.",
            require_type: CommitResult
        )
    }

    // Final test run before PR
    let pr: PullRequest = execute FeatureAgent.run(
        task: "Run the full test suite in ${repo_path}. If all tests pass, open a GitHub PR with a summary of all implemented features.",
        require_type: PullRequest
    )

    return pr
}
```

**OpenCode invocation:**
```bash
opencode /ImplementFeatures '[{"name":"dark-mode","description":"Add dark mode toggle","acceptance_criteria":["toggle persists","all pages affected"]}]' "/Users/me/projects/myapp"
```

---

### 3.3 Multi-Repo Dependency Audit and Auto-Fix

**The scenario:** A security advisory is published for a package. The platform team needs to audit 20 repositories, identify which ones are affected, update the dependency, run tests, and open PRs — all automatically.

**The Claw program:**
```claw
type AuditResult {
    repo: string
    affected: boolean
    current_version: string
    safe_version: string
    cve_id: string
}

type FixResult {
    repo: string
    pr_url: string
    tests_passed: boolean
}

tool ScanDependency(repo_path: string, package: string) -> AuditResult {
    invoke: module("scripts/audit").function("scan")
}

tool UpdateDependency(repo_path: string, package: string, version: string) -> boolean {
    invoke: module("scripts/audit").function("update")
}

tool OpenPullRequest(repo: string, title: string, body: string) -> FixResult {
    invoke: module("scripts/github").function("openPR")
}

client SecurityBot {
    provider = "anthropic"
    model = "claude-sonnet-4-6"
}

agent SecurityAuditor {
    client = SecurityBot
    system_prompt = "You are a security engineer. Audit repositories for vulnerable dependencies and apply minimal, safe fixes. Never change anything unrelated to the vulnerability."
    tools = [ScanDependency, UpdateDependency, OpenPullRequest]
    settings = { max_steps: 20, temperature: 0.0 }
}

workflow AuditAndFix(repos: list<string>, package: string, safe_version: string, cve_id: string) -> list<FixResult> {
    let results: list<FixResult> = []

    for repo in repos {
        let audit: AuditResult = execute SecurityAuditor.run(
            task: "Scan ${repo} for ${package}. Report if affected by ${cve_id}.",
            require_type: AuditResult
        )

        if audit.affected {
            let fix: FixResult = execute SecurityAuditor.run(
                task: "Update ${package} to ${safe_version} in ${repo}. Run tests. If tests pass, open a PR titled 'fix: patch ${cve_id} in ${package}'.",
                require_type: FixResult
            )
        }
    }

    return results
}
```

---

### 3.4 Release Engineering Pipeline

**The scenario:** Cutting a release: bump versions, update changelogs, tag, build artifacts, publish to NPM/PyPI, notify Slack.

**The Claw program:**
```claw
type ReleaseConfig {
    version: string
    packages: list<string>
    changelog_entry: string
}

type ReleaseArtifact {
    version: string
    npm_url: string
    pypi_url: string
    github_release_url: string
    changelog_entry: string
}

type PublishResult {
    registry: string
    url: string
    success: boolean
}

tool BumpVersion(package_path: string, version: string) -> boolean {
    invoke: module("scripts/release").function("bumpVersion")
}

tool UpdateChangelog(version: string, entry: string) -> boolean {
    invoke: module("scripts/release").function("updateChangelog")
}

tool GitTag(tag: string, message: string) -> boolean {
    invoke: module("scripts/git").function("createTag")
}

tool PublishNPM(package_path: string) -> PublishResult {
    invoke: module("scripts/publish").function("npm")
}

tool PublishPyPI(package_path: string) -> PublishResult {
    invoke: module("scripts/publish").function("pypi")
}

tool NotifySlack(channel: string, message: string) -> boolean {
    invoke: module("scripts/notify").function("slack")
}

client ReleaseBot {
    provider = "anthropic"
    model = "claude-sonnet-4-6"
}

agent ReleaseEngineer {
    client = ReleaseBot
    system_prompt = "You are a release engineer executing a release checklist. Each step is required. Report pass/fail for every step."
    tools = [BumpVersion, UpdateChangelog, GitTag, PublishNPM, PublishPyPI, NotifySlack]
    settings = { max_steps: 25, temperature: 0.0 }
}

workflow CutRelease(config: ReleaseConfig) -> ReleaseArtifact {
    // Bump versions across all packages
    for package in config.packages {
        let _ = execute ReleaseEngineer.run(
            task: "Bump ${package} to version ${config.version}",
            require_type: boolean
        )
    }

    // Update changelog
    let _ = execute ReleaseEngineer.run(
        task: "Update CHANGELOG.md with entry for ${config.version}: ${config.changelog_entry}",
        require_type: boolean
    )

    // Publish to registries
    let npm: PublishResult = execute ReleaseEngineer.run(
        task: "Publish npm-cli/ to NPM as @claw/cli@${config.version}",
        require_type: PublishResult
    )

    let pypi: PublishResult = execute ReleaseEngineer.run(
        task: "Publish python-sdk/ to PyPI as claw-sdk@${config.version}",
        require_type: PublishResult
    )

    // Tag and notify
    let artifact: ReleaseArtifact = execute ReleaseEngineer.run(
        task: "Create git tag v${config.version} and a GitHub release with changelog entry. Then notify #releases on Slack.",
        require_type: ReleaseArtifact
    )

    return artifact
}
```

**OpenCode invocation:**
```bash
opencode /CutRelease '{"version":"1.2.0","packages":["npm-cli","python-sdk"],"changelog_entry":"Add BAML codegen target and offline test runner"}'
```

---

### 3.5 Automated Code Review Pipeline

**The scenario:** Every PR triggers an agent pipeline: read the diff, run static analysis tools, check test coverage, write a structured review comment, and optionally block merge if critical issues are found.

```claw
type PRDiff {
    files_changed: int
    additions: int
    deletions: int
    diff_text: string
}

type ReviewComment {
    severity: string  // "blocking" | "suggestion" | "nitpick"
    file: string
    line: int
    message: string
    suggested_fix: string
}

type ReviewResult {
    approved: boolean
    comments: list<ReviewComment>
    summary: string
    coverage_delta: float
}

tool FetchPRDiff(pr_number: int, repo: string) -> PRDiff {
    invoke: module("scripts/github").function("getPRDiff")
}

tool RunLinter(path: string) -> list<ReviewComment> {
    invoke: module("scripts/lint").function("run")
}

tool GetCoverage(path: string) -> float {
    invoke: module("scripts/test").function("coverage")
}

tool PostReview(pr_number: int, result: ReviewResult) -> boolean {
    invoke: module("scripts/github").function("postReview")
}

client Reviewer {
    provider = "anthropic"
    model = "claude-sonnet-4-6"
}

agent CodeReviewer {
    client = Reviewer
    system_prompt = "You are a senior code reviewer. Focus on correctness, security, and maintainability. Be specific. Blocking comments must cite the exact risk."
    tools = [FetchPRDiff, RunLinter, GetCoverage, PostReview]
    settings = { max_steps: 15, temperature: 0.1 }
}

workflow ReviewPR(pr_number: int, repo: string) -> ReviewResult {
    let result: ReviewResult = execute CodeReviewer.run(
        task: "Review PR #${pr_number} in ${repo}. Fetch the diff, run the linter, check coverage delta. Post a structured review. Approve only if: no blocking linter errors, no test coverage regression > 5%, no obvious security issues.",
        require_type: ReviewResult
    )
    return result
}
```

---

### 3.6 Incident Response Automation

**The scenario:** A monitoring alert fires. An on-call agent is triggered: diagnose the issue, check logs, identify root cause, apply a known fix if available, and page a human only if the fix fails.

```claw
type Alert {
    service: string
    severity: string
    metric: string
    threshold: float
    current_value: float
    timestamp: string
}

type Diagnosis {
    root_cause: string
    affected_service: string
    log_evidence: string
    known_fix_available: boolean
}

type IncidentReport {
    alert: Alert
    diagnosis: Diagnosis
    fix_applied: boolean
    human_paged: boolean
    resolution_time_seconds: int
}

tool QueryLogs(service: string, time_range: string) -> string {
    invoke: module("scripts/observability").function("queryLogs")
}

tool QueryMetrics(service: string, metric: string, time_range: string) -> string {
    invoke: module("scripts/observability").function("queryMetrics")
}

tool ApplyFix(service: string, fix_id: string) -> boolean {
    invoke: module("scripts/runbook").function("applyFix")
}

tool PageHuman(team: string, incident: IncidentReport) -> boolean {
    invoke: module("scripts/notify").function("pageOnCall")
}

client IncidentBot {
    provider = "anthropic"
    model = "claude-sonnet-4-6"
}

agent IncidentResponder {
    client = IncidentBot
    system_prompt = "You are an SRE incident responder. Query logs and metrics to diagnose issues. Apply known runbook fixes. Only page a human when you have exhausted automated options or the severity is P0."
    tools = [QueryLogs, QueryMetrics, ApplyFix, PageHuman]
    settings = { max_steps: 20, temperature: 0.0 }
}

workflow RespondToAlert(alert: Alert) -> IncidentReport {
    let report: IncidentReport = execute IncidentResponder.run(
        task: "Alert fired: ${alert.service} — ${alert.metric} is ${alert.current_value} (threshold: ${alert.threshold}). Query the last 30 minutes of logs and metrics for ${alert.service}. Diagnose the root cause. If a known fix exists in the runbook, apply it and verify recovery. If not, page the on-call team.",
        require_type: IncidentReport
    )
    return report
}
```

---

### 3.7 Data Pipeline Orchestration

**The scenario:** Scrape structured data from multiple sources, normalize it, validate it against a typed schema, deduplicate, and write to a database — all as a typed, repeatable workflow with no ad hoc scripting.

```claw
type ScrapedRecord {
    source_url: string
    title: string
    price: float
    category: string
    scraped_at: string
}

type NormalizedRecord {
    id: string
    title: string
    price_usd: float
    category: string
    source: string
}

type PipelineResult {
    records_scraped: int
    records_valid: int
    records_written: int
    errors: list<string>
}

tool ScrapeSource(url: string, selector: string) -> list<ScrapedRecord> {
    invoke: module("scripts/scraper").function("scrape")
}

tool NormalizeRecords(records: list<ScrapedRecord>) -> list<NormalizedRecord> {
    invoke: module("scripts/transform").function("normalize")
}

tool WriteToDatabase(records: list<NormalizedRecord>, table: string) -> int {
    invoke: module("scripts/db").function("upsert")
}

client DataAgent {
    provider = "anthropic"
    model = "claude-sonnet-4-6"
}

agent PipelineRunner {
    client = DataAgent
    system_prompt = "You orchestrate data pipelines. Process each source in sequence. Report counts and errors at each stage."
    tools = [ScrapeSource, NormalizeRecords, WriteToDatabase]
    settings = { max_steps: 30, temperature: 0.0 }
}

workflow RunDailyPipeline(sources: list<string>, target_table: string) -> PipelineResult {
    let result: PipelineResult = execute PipelineRunner.run(
        task: "Scrape all sources in ${sources}, normalize the records, deduplicate by id, and write to ${target_table}. Report total counts and any per-source errors.",
        require_type: PipelineResult
    )
    return result
}
```

---

### 3.8 CLI-Authenticated GitHub + VS Code Workflow

**The scenario** (the canonical example from the product owner): A workflow that goes to GitHub, opens VS Code (installs it first if absent), adds a list of features via a coding agent, handles CLI authentication from `.env`, and opens a PR.

This is the prototype use case that defines the product's core value proposition.

```claw
type GitHubAuth {
    token: string
    username: string
    authenticated: boolean
}

type VSCodeStatus {
    installed: boolean
    version: string
    path: string
}

type FeatureSpec {
    name: string
    description: string
}

type PRResult {
    pr_url: string
    branch: string
    commits: int
    tests_passed: boolean
}

tool LoadEnvVar(key: string) -> string {
    invoke: module("scripts/env").function("load")
}

tool AuthGitHubCLI(token: string) -> GitHubAuth {
    invoke: module("scripts/github").function("cliLogin")
}

tool CheckVSCode() -> VSCodeStatus {
    invoke: module("scripts/ide").function("checkVSCode")
}

tool InstallVSCode() -> VSCodeStatus {
    invoke: module("scripts/ide").function("installVSCode")
}

tool CloneOrPullRepo(repo_url: string, local_path: string, token: string) -> boolean {
    invoke: module("scripts/git").function("cloneOrPull")
}

tool OpenInVSCode(path: string) -> boolean {
    invoke: module("scripts/ide").function("open")
}

tool OpenPullRequest(repo: string, branch: string, title: string, body: string) -> PRResult {
    invoke: module("scripts/github").function("openPR")
}

client DevOrchestrator {
    provider = "anthropic"
    model = "claude-sonnet-4-6"
}

agent SetupAgent {
    client = DevOrchestrator
    system_prompt = "You set up developer environments and handle CLI authentication. Follow each step exactly. Pull secrets only from the approved .env paths listed in the task."
    tools = [LoadEnvVar, AuthGitHubCLI, CheckVSCode, InstallVSCode, CloneOrPullRepo, OpenInVSCode]
    settings = { max_steps: 15, temperature: 0.0 }
}

agent FeatureAgent {
    client = DevOrchestrator
    system_prompt = "You implement software features in existing codebases. Follow the project's existing patterns. Write tests for each feature. Commit after each feature with a clear message."
    tools = [OpenPullRequest]
    settings = { max_steps: 50, temperature: 0.1 }
}

workflow AddFeaturesToRepo(repo_url: string, features: list<FeatureSpec>, local_path: string) -> PRResult {
    // Step 1: Load credentials from .env (never hardcoded)
    let gh_token: string = execute SetupAgent.run(
        task: "Load GITHUB_TOKEN from .env at ${local_path}/.env",
        require_type: string
    )

    // Step 2: Authenticate GitHub CLI
    let auth: GitHubAuth = execute SetupAgent.run(
        task: "Authenticate the GitHub CLI using the token. Confirm authentication succeeded.",
        require_type: GitHubAuth
    )

    // Step 3: Check VS Code — install if missing
    let vscode: VSCodeStatus = execute SetupAgent.run(
        task: "Check if VS Code is installed. If not, install it via: brew install --cask visual-studio-code",
        require_type: VSCodeStatus
    )

    // Step 4: Clone or pull the repo
    let _ = execute SetupAgent.run(
        task: "Clone ${repo_url} into ${local_path} using the GitHub token for auth. If it already exists, pull latest from main.",
        require_type: boolean
    )

    // Step 5: Open in VS Code
    let _ = execute SetupAgent.run(
        task: "Open ${local_path} in VS Code",
        require_type: boolean
    )

    // Step 6: Implement features one by one
    for feature in features {
        let _ = execute FeatureAgent.run(
            task: "Implement feature '${feature.name}': ${feature.description}. Create a new file or modify existing ones as needed. Write tests. Commit with message 'feat: ${feature.name}'.",
            require_type: boolean
        )
    }

    // Step 7: Open PR
    let pr: PRResult = execute FeatureAgent.run(
        task: "Push the feature branch to ${repo_url} and open a pull request titled 'feat: batch feature implementation'. List each feature in the PR body.",
        require_type: PRResult
    )

    return pr
}
```

**OpenCode invocation:**
```bash
opencode /AddFeaturesToRepo \
  "https://github.com/myorg/myapp" \
  '[{"name":"dark-mode","description":"Add dark/light mode toggle that persists in localStorage"},{"name":"rate-limiter","description":"Add API rate limiting middleware: 100 req/min per IP"}]' \
  "~/projects/myapp"
```

---

## 4. What Makes Claw Different from Alternatives

| Alternative | Limitation | Claw's Answer |
|-------------|-----------|---------------|
| **Raw OpenCode config** (hand-written JSON + markdown) | No type safety, no compile-time validation, no reusable types, no control flow | Claw compiles to OpenCode config — get OpenCode's power with typed, validated programs |
| **LangChain / LangGraph** | Python-first, heavy dependencies, no static analysis, runtime type errors | Claw is a compiled language — type errors caught before execution |
| **N8N / Zapier** | GUI-only, not version-controlled, poor developer experience, limited LLM integration | Claw is code — lives in git, reviewed in PRs, tested in CI |
| **Raw bash scripts** | No LLM integration, brittle, no parallelism, hard to maintain | Claw adds typed agent steps to your pipeline with the same determinism as shell |
| **Temporal / Prefect** | Requires a workflow server, significant infra overhead | Claw compiles to OpenCode — the only infra is `opencode` on the developer's machine or CI runner |
| **BAML** | Excellent for LLM function calls, but no full workflow orchestration | Claw can target BAML (`--lang baml`) — they are complementary |

---

## 5. The Determinism Guarantee

Claw workflows are **deterministic programs**, not prompts. This means:

1. **Control flow is code, not prose.** `for feature in features` is a real loop — not "please handle each feature". The agent executes N iterations.
2. **Types are enforced at compile time.** If `FeatureAgent` returns `PRResult` and a downstream step expects `CommitResult`, `clawc` rejects it before a single token is generated.
3. **Agent boundaries are typed contracts.** `require_type: SearchResult` is a constraint the compiler verifies — not a suggestion the LLM can ignore.
4. **Secrets never appear in prompts.** `LoadEnvVar("GITHUB_TOKEN")` loads from `.env` via an MCP tool call. The token is never injected into the system prompt or task string.
5. **Tools are validated before execution.** All tool inputs are validated against their JSON Schema by the MCP server before the handler function is called. Invalid inputs return a structured error; the server keeps running.

---

## 6. Relationship to OpenCode CLI

OpenCode can be driven from the command line with full programmatic control:

```bash
# Run a named workflow command
opencode /WorkflowName "arg1" "arg2"

# Run non-interactively with a specific model
opencode --model anthropic/claude-sonnet-4-6 -p "task text"

# Use a specific agent
opencode --agent Researcher "find info about X"
```

Claw's compiled output makes ALL of the above work correctly:
- `opencode.json` configures the provider, model, and MCP server
- `.opencode/commands/WorkflowName.md` defines the `/WorkflowName` command
- `.opencode/agents/Researcher.md` defines the `Researcher` agent
- `generated/mcp-server.js` exposes all tools via MCP

A Claw program IS an OpenCode program — it just has a type-safe, statically-verified, version-controlled source that compiles to it.

---

## 7. Implementation Priority for Use Cases

| Priority | Use Case | Why |
|----------|---------|-----|
| P0 | 3.8 GitHub + VS Code + .env workflow | The canonical product demo — covers install detection, env secret loading, git auth, feature implementation, PR creation |
| P0 | 3.1 Dev environment bootstrap | Most relatable developer pain point |
| P1 | 3.2 Feature implementation | Core "agent writes code" use case |
| P1 | 3.4 Release engineering | Shows multi-step deterministic pipelines |
| P2 | 3.3 Dependency audit | Shows `for` loop + conditional branching |
| P2 | 3.5 Code review | Shows integration with CI/CD |
| P3 | 3.6 Incident response | Shows monitoring + runbook integration |
| P3 | 3.7 Data pipeline | Shows non-code agentic use cases |

All `example.claw` files generated by `claw init` and all documentation examples SHOULD reflect the P0/P1 use cases.
