# Phase 7 GAN Audit: Binary Distribution & Corporate Environments

In this GAN (Generative Adversarial Network) Audit, **The Breaker** (Security/Enterprise Architect) interrogates **The Maker** (Lead Engineer) regarding the introduction of `specs/19-Binary-Distribution.md` (distributing the Rust compiler via an NPM `postinstall` hook).

---

## 1. The Corporate Proxy & Firewall Block

**Breaker (The Attacker):**
> "Your plan to use an `npm postinstall` script to anonymously download a zip file from `github.com/releases` is going to crash and burn in enterprise environments. 
> Corporations heavily proxy their networks. They use custom SSL certificates and actively block raw outbound `curl` / HTTP requests that don't originate from authorized registries. When a Fortune 500 bank runs `npm install @claw/cli`, the network will sever the binary download. `npm install` hangs, fails, and developers toss Claw in the garbage."

**Maker (The Defender):**
> "This is a known issue for Prisma, Esbuild, and BAML. You are correct that we cannot blindly assume `github.com` is highly available for binary downloads."

**Resolution (MAKER YIELDS - SPEC MUTATION):**
*Implementation Fix:* We will modify `specs/19-Binary-Distribution.md`. The `@claw/cli` installation script MUST respect standard system proxy configurations (`HTTP_PROXY`, `HTTPS_PROXY`, `npm config get proxy`). 
Furthermore, it must support an environment variable `CLAW_DOWNLOAD_MIRROR` (allowing enterprises to host the binaries on their internal Artifactory) and `CLAW_BINARY_PATH` (allowing users to completely bypass the download and point the NPM wrapper to a pre-installed local binary).

---

## 2. Supply Chain Code Injection

**Breaker (The Attacker):**
> "By distributing binaries through a `postinstall` hook, you are opening up the mother of all supply chain attack vectors. If compromised, an attacker can ship malicious code, and because `postinstall` runs arbitrary code upon execution of `npm install`, you execute malware on thousands of developer machines automatically before they even run your tool."

**Maker (The Defender):**
> "This is inherently true for any dynamically downloaded dependency."

**Resolution (BREAKER YIELDS - DX UPDATE):**
*Implementation Fix:* To mitigate this, the `@claw/cli` package will be strictly published with **Provenance via GitHub Actions (Sigstore)**. Additionally, the `postinstall` script will contain an internal dictionary of SHA-256 hashes corresponding to the exact expected GitHub release binaries. Upon downloading the `.tar.gz`, the script will cryptographically verify the checksum *before* extracting the executable. If the hash does not match, the installation will `exit 1` to immediately halt execution.

---

## 3. Resolving Execution Contexts in Downstream SDKs

**Breaker (The Attacker):**
> "Because the compiler is now hidden loosely inside `node_modules/.bin/claw`, what happens inside the execution gateway (`07-Claw-OS.md`)? 
> When the gateway does hot-reloading or needs to spawn a child process to parse AST configurations, where does it look? If a developer installed it globally (`npm i -g`) versus locally as a devDependency, your `exec()` calls will crash with 'EACCES: permission denied' or 'Claw not found'. You've fragmented the resolution context!"

**Maker (The Defender):**
> "Good catch. The current CLI specification (`14-CLI-Tooling.md`) assumes `$PATH` always contains `claw`. Moving to NPM binaries fragments the execution path."

**Resolution (MAKER YIELDS - COMPILER UPGRADE):**
*Implementation Fix:* The runtime Gateway module MUST implement a deterministic binary resolution waterfall:
1. Check `executable_path` in `claw.json`.
2. Check `CLAW_BINARY_PATH` environment variable.
3. Check `node_modules/.bin/claw` resolved relative to `process.cwd()`.
4. Check global `$PATH`.
5. Finally, if none of those work, print a helpful error: *"Claw binary not found. Did you run 'npm install @claw/cli' or define CLAW_BINARY_PATH?"*

---

## 4. Mac Silicon Quarantine (Gatekeeper)

**Breaker (The Attacker):**
> "You're publishing raw Darwin binaries on GitHub Releases and downloading them via `node`. Apple's Gatekeeper routinely flags binaries downloaded dynamically outside of the App Store or Xcode as 'unverified developers' and physically prevents them from executing on macOS Apple Silicon. Users will run `npx claw test` and get an Apple pop-up saying the file is damaged and should be moved to the Trash."

**Maker (The Defender):**
> "We must formally code-sign the Darwin binaries."

**Resolution (MAKER YIELDS - OS UPGRADE):**
*Implementation Fix:* The GitHub Actions pipeline (`release.yml`) must integrate Apple `codesign` and `notarytool` for the `x86_64-apple-darwin` and `aarch64-apple-darwin` targets. Alternatively, if code signing is deferred for Phase 8, the `postinstall` script running on macOS must explicitly strip the quarantine meta-attribute during extraction: `xattr -d com.apple.quarantine bin/claw`.

---

### Audit Conclusion
Moving to a binary distribution pipeline drastically lowers the barrier to entry but heavily increases supply chain and network perimeter risks. 
By instituting **SHA-256 Checksum validation**, **Proxy compliance**, a **Deterministic Execution Waterfall**, and **Gatekeeper avoidance**, we can safely launch `@claw/cli` on NPM with enterprise-grade reliability.
