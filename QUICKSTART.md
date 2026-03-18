# Quickstart & Testing Guide

Welcome to the `.claw` compiler! This guide will walk you through compiling your first agent orchestration graph and running your test suites without burning expensive LLM tokens.

## 1. Setup the CLI

Ensure you have Rust installed. Clone the repository and build the `clawc` CLI tool.
```bash
git clone https://github.com/open-code/openclaw.git
cd openclaw
cargo install --path .
```

Verify the installation:
```bash
clawc --version
# Output: clawc 1.0.0
```

## 2. Compiling your First Agent Pipeline

Create a new file `pipeline.claw` in your project workspace:
```claw
type Greeting {
    message: string
}

agent Greeter {
    client = OpenAI.GPT_4O_MINI
    system_prompt = "You are a friendly greeter."
}

workflow HelloOpenClaw(name: string) -> Greeting {
    let result: Greeting = execute Greeter.run(
        task: "Say hello to ${name}",
        require_type: Greeting
    )
    return result
}
```

Run the compiler pointing to your target SDK language (default: TypeScript).

```bash
clawc build pipeline.claw --lang ts
```

If your syntax is correct, `clawc` will silently output a new `generated/claw/` directory into your project, containing strictly-typed `Greeting` interfaces and an async `HelloOpenClaw` network call wrapper.

## 3. Writing and Running Tests

You shouldn't execute against OpenAI every time your CI/CD pipeline runs. `.claw` supports native JSON mocking so you can test your programmatic orchestration logic locally.

In your `pipeline.claw` file, append a `test` block:

```claw
mock Greeter("Say hello to Alice") -> {
    "message": "Hello there, Alice!"
}

test "Verify Greeter Workflow" {
    // This executes entirely offline using the 'mock' block above.
    let response = HelloOpenClaw("Alice")
    
    // Test primitives
    assert(response.message == "Hello there, Alice!")
}
```

To run your tests via the `clawc` CLI:

```bash
clawc test pipeline.claw
```

The CLI will parse the AST, temporarily bypass the OpenClaw Gateway Network layer, inject your JSON mock schemas against the TypeSystem Bouncer, and execute the program flow locally.

## What's Next?
Once your local tests pass, it's time to integrate this AST into real code.

See [PRODUCTION.md](./PRODUCTION.md) to learn how to wire the generated `.ts` SDK into your Next.js/Express app.
