import { spawn } from "node:child_process";
import { access, realpath } from "node:fs/promises";
import { constants } from "node:fs";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, extname, isAbsolute, join, relative, resolve } from "node:path";
import { pathToFileURL } from "node:url";

const DEFAULT_SANDBOX_TIMEOUT_MS = 30_000;

interface RuntimeDescriptor {
  runtime: "python" | "typescript" | "module";
  target: string;
  functionName?: string;
}

interface ExecutionOptions {
  timeoutMs?: number;
}

interface RetryOptions {
  maxRetries: number;
  baseDelayMs?: number;
}

export async function retryWithBackoff<T>(
  fn: () => Promise<T>,
  options: RetryOptions
): Promise<T> {
  const { maxRetries, baseDelayMs = 200 } = options;
  let lastError: Error | undefined;

  for (let attempt = 0; attempt <= maxRetries; attempt++) {
    try {
      return await fn();
    } catch (error) {
      lastError = error instanceof Error ? error : new Error(String(error));
      if (attempt >= maxRetries) {
        break;
      }
      const delay = baseDelayMs * Math.pow(2, attempt);
      await new Promise((resolve) => setTimeout(resolve, delay));
    }
  }

  throw lastError;
}

export async function executeCustomTool(
  invokePath: string,
  args: Record<string, unknown>,
  workspaceRoot = process.cwd(),
  options: ExecutionOptions = {}
): Promise<unknown> {
  const descriptor = parseInvokePath(invokePath);

  if (descriptor.runtime === "module") {
    return executeModuleFunction(descriptor, args, workspaceRoot);
  }

  return executeIsolatedCommand(descriptor, args, workspaceRoot, options);
}

function parseInvokePath(invokePath: string): RuntimeDescriptor {
  const moduleMatch = invokePath.match(/^module\("([^"]+)"\)\.function\("([^"]+)"\)$/);
  if (moduleMatch) {
    return { runtime: "module", target: moduleMatch[1], functionName: moduleMatch[2] };
  }

  const pythonMatch = invokePath.match(/^python\("([^"]+)"\)$/);
  if (pythonMatch) {
    return { runtime: "python", target: pythonMatch[1] };
  }

  const typescriptMatch = invokePath.match(/^typescript\("([^"]+)"\)$/);
  if (typescriptMatch) {
    return { runtime: "typescript", target: typescriptMatch[1] };
  }

  throw new Error(`Unsupported invoke path ${invokePath}`);
}

async function executeModuleFunction(
  descriptor: RuntimeDescriptor,
  args: Record<string, unknown>,
  workspaceRoot: string
): Promise<unknown> {
  const modulePath = await resolveExistingPath(descriptor.target, workspaceRoot, [".js", ".mjs", ".ts"]);
  const module = await import(pathToFileURL(modulePath).href);
  const callable = module[descriptor.functionName!];
  if (typeof callable !== "function") {
    throw new Error(`Expected exported function ${descriptor.functionName} in ${modulePath}`);
  }
  return callable(args);
}

async function executeIsolatedCommand(
  descriptor: RuntimeDescriptor,
  args: Record<string, unknown>,
  workspaceRoot: string,
  options: ExecutionOptions = {}
): Promise<unknown> {
  const timeoutMs = options.timeoutMs ?? DEFAULT_SANDBOX_TIMEOUT_MS;
  const backend = process.env.CLAW_SANDBOX_BACKEND === "docker" ? "docker" : "local";
  const sandboxDir =
    backend === "docker"
      ? await mkdtemp(join(tmpdir(), "claw-tool-"))
      : null;
  const command = await buildSandboxCommand(
    descriptor,
    workspaceRoot,
    backend,
    sandboxDir ?? undefined
  );
  const payload = JSON.stringify(args);

  try {
    return await new Promise((resolvePromise, reject) => {
      const child = spawn(command.command, command.args, {
        cwd: workspaceRoot,
        stdio: ["pipe", "pipe", "pipe"],
        env: {
          ...process.env,
          CLAW_SANDBOX_MODE: command.mode
        }
      });

      let stdout = "";
      let stderr = "";
      let killed = false;

      const timer = setTimeout(() => {
        killed = true;
        child.kill("SIGKILL");
        reject(new Error(
          `Sandboxed tool timed out after ${timeoutMs}ms: ${command.command} ${command.args.join(" ")}`
        ));
      }, timeoutMs);

      child.stdout.on("data", (chunk) => {
        stdout += chunk.toString();
      });
      child.stderr.on("data", (chunk) => {
        stderr += chunk.toString();
      });

      child.on("error", (error) => {
        clearTimeout(timer);
        reject(error);
      });
      child.on("close", (code) => {
        clearTimeout(timer);
        if (killed) {
          return;
        }
        if (code !== 0) {
          reject(new Error(`Sandboxed tool exited with code ${code}: ${stderr}`));
          return;
        }
        resolvePromise(stdout.trim() ? JSON.parse(stdout) : null);
      });

      child.stdin.write(payload);
      child.stdin.end();
    });
  } finally {
    if (sandboxDir) {
      await rm(sandboxDir, { recursive: true, force: true });
    }
  }
}

async function buildSandboxCommand(
  descriptor: RuntimeDescriptor,
  workspaceRoot: string,
  backend: "docker" | "local" = process.env.CLAW_SANDBOX_BACKEND === "docker"
    ? "docker"
    : "local",
  sandboxDir = "/tmp/claw-sandbox"
): Promise<{ command: string; args: string[]; mode: "docker" | "local" }> {
  if (backend === "docker") {
    return buildDockerSandboxCommand(descriptor, workspaceRoot, sandboxDir);
  }
  return buildLocalSandboxCommand(descriptor, workspaceRoot);
}

async function buildLocalSandboxCommand(
  descriptor: RuntimeDescriptor,
  workspaceRoot: string
): Promise<{ command: string; args: string[]; mode: "local" }> {
  if (descriptor.runtime === "python") {
    if (looksLikePythonModule(descriptor.target)) {
      return {
        command: "python3",
        args: ["-m", descriptor.target],
        mode: "local"
      };
    }

    return {
      command: "python3",
      args: [await resolveExistingPath(descriptor.target, workspaceRoot, [".py"])],
      mode: "local"
    };
  }

  return {
    command: "node",
    args: [
      "--experimental-strip-types",
      await resolveExistingPath(descriptor.target, workspaceRoot, [".ts", ".mts", ".js", ".mjs"])
    ],
    mode: "local"
  };
}

async function buildDockerSandboxCommand(
  descriptor: RuntimeDescriptor,
  workspaceRoot: string,
  sandboxDir: string
): Promise<{ command: string; args: string[]; mode: "docker" }> {
  const workspaceMount = "/workspace";
  const sandboxMount = "/sandbox";
  const baseArgs = [
    "run",
    "--rm",
    "-i",
    "--network=none",
    "--read-only",
    "--cap-drop=ALL",
    "--security-opt=no-new-privileges",
    "--pids-limit=64",
    "--memory=256m",
    "--cpus=1",
    "--user=65532:65532",
    "-v",
    `${workspaceRoot}:${workspaceMount}:ro`,
    "-v",
    `${sandboxDir}:${sandboxMount}`,
    "-w",
    sandboxMount
  ];

  if (descriptor.runtime === "python") {
    const image = process.env.CLAW_PYTHON_SANDBOX_IMAGE ?? "python:3.11-slim";
    const targetArgs = looksLikePythonModule(descriptor.target)
      ? ["-e", `PYTHONPATH=${workspaceMount}`, image, "python", "-m", descriptor.target]
      : [
          image,
          "python",
          toContainerPath(
            workspaceRoot,
            await resolveExistingPath(descriptor.target, workspaceRoot, [".py"]),
            workspaceMount
          )
        ];

    return {
      command: "docker",
      args: [...baseArgs, ...targetArgs],
      mode: "docker"
    };
  }

  const image = process.env.CLAW_NODE_SANDBOX_IMAGE ?? "node:22";
  const targetPath = await resolveExistingPath(descriptor.target, workspaceRoot, [".ts", ".mts", ".js", ".mjs"]);
  return {
    command: "docker",
    args: [
      ...baseArgs,
      image,
      "node",
      "--experimental-strip-types",
      toContainerPath(workspaceRoot, targetPath, workspaceMount)
    ],
    mode: "docker"
  };
}

/**
 * Per specs/12-Security-Model.md Section 5: resolve with realpath()
 * and verify the resolved path remains within workspaceRoot.
 */
async function resolveExistingPath(target: string, workspaceRoot: string, extensions: string[]): Promise<string> {
  const realWorkspace = await realpath(workspaceRoot);
  const baseCandidates = new Set([
    resolve(workspaceRoot, target),
    resolve(workspaceRoot, target.replace(/\./g, "/"))
  ]);
  const candidates = Array.from(baseCandidates).flatMap((basePath) =>
    hasRecognizedExtension(basePath, extensions)
      ? [basePath]
      : [basePath, ...extensions.map((extension) => `${basePath}${extension}`)]
  );

  for (const candidate of candidates) {
    try {
      await access(candidate, constants.R_OK);
      const real = await realpath(candidate);
      const rel = relative(realWorkspace, real);
      if (rel.startsWith("..") || isAbsolute(rel)) {
        throw new Error(`Tool target resolves outside workspace: ${target}`);
      }
      return real;
    } catch (error) {
      if (error instanceof Error && error.message.includes("outside workspace")) {
        throw error;
      }
      continue;
    }
  }

  throw new Error(`Could not resolve runtime target ${target}`);
}

function hasRecognizedExtension(path: string, extensions: string[]): boolean {
  return extensions.includes(extname(path));
}

function looksLikePythonModule(target: string): boolean {
  return !target.includes("/") && !target.includes("\\") && !hasRecognizedExtension(target, [".py"]);
}

function toContainerPath(workspaceRoot: string, resolvedPath: string, workspaceMount: string): string {
  const relativePath = relative(workspaceRoot, resolvedPath);
  if (relativePath.startsWith("..")) {
    throw new Error(`Sandbox target ${basename(resolvedPath)} is outside the workspace`);
  }

  return resolve(workspaceMount, relativePath);
}

/**
 * Pre-pull Docker images at gateway startup so first tool executions don't
 * incur the image download penalty. Runs in the background and logs results.
 */
export async function prePullSandboxImages(): Promise<void> {
  if (process.env.CLAW_SANDBOX_BACKEND !== "docker") {
    return;
  }

  const images = [
    process.env.CLAW_PYTHON_SANDBOX_IMAGE ?? "python:3.11-slim",
    process.env.CLAW_NODE_SANDBOX_IMAGE ?? "node:22"
  ];

  await Promise.allSettled(
    images.map(async (image) => {
      try {
        await new Promise<void>((resolvePromise, reject) => {
          const child = spawn("docker", ["pull", image], {
            stdio: ["ignore", "pipe", "pipe"]
          });

          child.on("error", reject);
          child.on("close", (code) => {
            if (code === 0) {
              console.log(`[sandbox] pre-pulled ${image}`);
              resolvePromise();
            } else {
              reject(new Error(`docker pull ${image} exited with code ${code}`));
            }
          });
        });
      } catch (error) {
        console.warn(`[sandbox] failed to pre-pull ${image}: ${error instanceof Error ? error.message : error}`);
      }
    })
  );
}

export const __testing = {
  async buildSandboxCommand(
    invokePath: string,
    workspaceRoot: string,
    backend: "docker" | "local" = "local"
  ): Promise<{ command: string; args: string[]; mode: "docker" | "local" }> {
    return buildSandboxCommand(
      parseInvokePath(invokePath),
      workspaceRoot,
      backend
    );
  }
};
