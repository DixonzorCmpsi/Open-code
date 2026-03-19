# Quickstart & Testing Guide

Welcome to the Claw DSL! This guide will walk you through compiling your first agent orchestration graph and running your test suites using OpenCode.

## 1. Setup the CLI

Ensure you have Rust installed. Clone the repository and build the `claw` CLI tool.
```bash
git clone https://github.com/open-code/claw.git
cd claw
cargo install --path .
```

Verify the installation:
```bash
claw --version
# Output: claw 0.1.0
```

## 2. Compiling your First Agent Pipeline

Create a new file `example.claw` in your project workspace:
```claw
type Greeting {
    message: string
}

client MyClaude {
    provider = "anthropic"
    model = "claude-4-sonnet"
}

agent Greeter {
    client = MyClaude
    system_prompt = "You are a friendly greeter."
}

workflow HelloClaw(name: string) -> Greeting {
    let result: Greeting = execute Greeter.run(
        task: "Say hello to ${name}",
        require_type: Greeting
    )
    return result
}
```

Run the compiler to build the project for OpenCode.

```bash
claw build example.claw
```

If your syntax is correct, `claw` will output an `opencode.json` configuration, a `.opencode/` directory with command templates, and a `generated/` directory with the MCP server.

## 3. Writing and Running Tests

You can test your programmatic orchestration logic locally before deploying.

In your `example.claw` file, append a `test` block:

```claw
test "Verify Greeter Workflow" {
    // This executes the workflow in test mode
    let response = HelloClaw("Alice")
    assert(response.message.contains("Alice"))
}
```

To run your tests via the `claw` CLI:

```bash
claw test example.claw
```

## 4. Development Mode

For rapid iteration, use the `dev` command to watch your source files and auto-rebuild:

```bash
claw dev
```

## What's Next?
Once your local tests pass, it's time to run your workflow with OpenCode.

```bash
opencode /HelloClaw "Alice"
```

See [PRODUCTION.md](./PRODUCTION.md) to learn how to deploy your Claw project to production.
