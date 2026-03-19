# Phase 9 GAN Audit: Sandbox Container Architectures

> **STATUS: SUPERSEDED — Gateway retired. OpenCode is now the execution OS.**
> See `specs/25-OpenCode-Integration.md` for the active architecture.
> This document is preserved for historical reference only.

---



In this audit, two LLM agent personas (The Generator/Maker vs. The Discriminator/Breaker) evaluate the Claw Gateway runtime execution engine (`runtime.ts`) for multi-OS container flaws before production deployment.

---

## 1. The Cross-OS Docker Pathing Crash

**Breaker (The Attacker):**
> "Your `07-Claw-OS.md` Gateway specification enforces strict Docker bounds for custom `.claw` tool invocations to protect the host OS. 
> However, your implementation of `toContainerPath` in `openclaw-gateway/src/engine/runtime.ts` uses `node:path.resolve()` securely mapping the absolute host path into `/workspace`. 
> Problem: If a developer runs the API Gateway on **Windows**, `path.resolve` generates a string like `\workspace\scripts\tool.py`. Docker parses that string and executes it *inside* the Linux-based container VM. Linux does not understand `\`, interpreting it as an escape character, completely crashing all Tool executions permanently on Windows instances."

**Maker (The Defender):**
> "You've identified a critical abstraction leak. The Gateway is cross-platform, but the target execution sandbox (`python:3.11-slim` or `node:22`) is strictly POSIX. Generating host-dependent paths for internal container execution breaks the abstraction."

**Resolution (MAKER YIELDS - CODE FIX):**
*Implementation Fix:* We will refactor `toContainerPath` inside `runtime.ts` to explicitly map the native host's `relative()` path into a strict POSIX format using `.replaceAll(sep, '/')` before concatenating it with the `/workspace` mount point. This guarantees that whether the Gateway runs on Windows, MacOS, or Linux, the inner Docker command string remains purely POSIX-compliant.

---

## 2. Defaulting to Zero-Trust Docker

**Breaker (The Attacker):**
> "Right now your Gateway defaults to `local` sandbox execution unless `CLAW_SANDBOX_BACKEND="docker"` is explicitly set via environment variables. This creates a massive security hole. Developers running your code without configuring a `.env` file run arbitrary LLM-generated code natively on their physical hard drives."

**Maker (The Defender):**
> "We threw a red console Warning when local sandboxing is active. It's designed for lower friction."

**Resolution (MAKER YIELDS - SECURE DEFAULTS):**
*Implementation Fix:* The baseline assumption MUST be containerized isolation. If Docker is missing, we gracefully throw a fast failure, requiring the developer to explicitly opt-in to `local` execution rather than defaulting to unsafe, non-isolated environments. We will align the documentation (`QUICKSTART.md`) and default `.env` files downstream to enforce this configuration shift securely.