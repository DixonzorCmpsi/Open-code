# OpenClaw Agent DSL GitHub Configurations

This `.github` folder is the source of truth for repository configurations, continuous integration, and issue tracking for the OpenClaw Agent DSL.

As we build the compiler and SDK generators for the `.claw` domain-specific language, we will store:
- `ISSUE_TEMPLATE`: Templates for requesting new language features (e.g., new standard library tools or concurrency support).
- `workflows`: CI/CD actions for compiling the AST and testing the generated Python/TS SDKs against the OpenClaw Gateway.
