import { mkdir, mkdtemp, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";

import { HumanInterventionRequiredError } from "../engine/errors.ts";
import type { HumanInterventionEvent } from "../types.ts";
import { captureScreenshot, findElementByVision } from "./vision.ts";
import type { VisionCoordinates } from "./vision.ts";

interface BrowserContext {
  sessionId: string;
  nodePath?: string;
  consumeOverride?: () => Promise<unknown | null>;
}

export interface ScreenshotRecord {
  path: string;
  timestamp: string;
  action: string;
  url: string;
}

export async function search(query: string, context: BrowserContext): Promise<Record<string, unknown>> {
  const override = await consumeOverride(context);
  if (override) {
    return override;
  }

  const runtime = await getBrowserRuntime();
  if (!runtime) {
    return {
      url: `https://duckduckgo.com/?q=${encodeURIComponent(query)}`,
      text: `Playwright unavailable; fallback search context for ${query}.`
    };
  }

  const page = await runtime.newPage();
  try {
    await page.goto(`https://duckduckgo.com/?q=${encodeURIComponent(query)}`, {
      waitUntil: "domcontentloaded"
    });
    await ensureNoCaptcha(page, context.sessionId, {
      action: "search",
      query,
      node_path: context.nodePath
    });
    const screenshot = await saveActionScreenshot(page, context.sessionId, "search");
    return {
      url: page.url(),
      title: await page.title(),
      html: await page.content(),
      text: (await page.locator("body").innerText()).slice(0, 6000),
      screenshot: screenshot?.path ?? null
    };
  } finally {
    await page.close();
  }
}

export async function navigate(url: string, context: BrowserContext): Promise<Record<string, unknown>> {
  const override = await consumeOverride(context);
  if (override) {
    return override;
  }

  const runtime = await getBrowserRuntime();
  if (!runtime) {
    return {
      url,
      text: `Playwright unavailable; fallback navigation context for ${url}.`
    };
  }

  const page = await runtime.newPage();
  try {
    await page.goto(url, { waitUntil: "domcontentloaded" });
    await ensureNoCaptcha(page, context.sessionId, {
      action: "navigate",
      url,
      node_path: context.nodePath
    });
    const screenshot = await saveActionScreenshot(page, context.sessionId, "navigate");
    return {
      url: page.url(),
      title: await page.title(),
      html: await page.content(),
      text: (await page.locator("body").innerText()).slice(0, 6000),
      screenshot: screenshot?.path ?? null
    };
  } finally {
    await page.close();
  }
}

/**
 * Visual Marker: attempt to locate an element by sending a screenshot to a
 * multimodal LLM when a CSS selector lookup fails.
 */
export async function locateByVision(
  page: BrowserPage,
  instruction: string
): Promise<VisionCoordinates | null> {
  const screenshotBuffer = await captureScreenshot(page as never);
  return findElementByVision({ screenshot: screenshotBuffer, instruction });
}

let playwrightModulePromise:
  | Promise<{ chromium: { launch(options: { headless: boolean }): Promise<BrowserRuntime> } } | null>
  | null = null;
let browserRuntimePromise: Promise<BrowserRuntime | null> | null = null;

type BrowserRuntime = {
  newPage(): Promise<BrowserPage>;
  close(): Promise<void>;
};

type BrowserPage = {
  goto(url: string, options: { waitUntil: string }): Promise<unknown>;
  title(): Promise<string>;
  content(): Promise<string>;
  locator(selector: string): { innerText(): Promise<string> };
  screenshot(options: { path: string; fullPage: boolean }): Promise<unknown>;
  url(): string;
  close(): Promise<void>;
};

async function ensureNoCaptcha(
  page: BrowserPage,
  sessionId: string,
  metadata: Record<string, unknown>
): Promise<void> {
  const html = (await page.content()).toLowerCase();
  const captchaDetected =
    html.includes("captcha") || html.includes("cloudflare") || html.includes("verify you are human");
  if (!captchaDetected) {
    return;
  }

  const screenshotPath = await resolveCaptchaScreenshotPath(sessionId);
  await page.screenshot({ path: screenshotPath, fullPage: true });

  const event: HumanInterventionEvent = {
    type: "HumanInterventionRequired",
    session_id: sessionId,
    reason: "CAPTCHA detected in browser automation",
    metadata: {
      ...metadata,
      url: page.url(),
      screenshot_url: `/sessions/${encodeURIComponent(sessionId)}/screenshot`
    }
  };
  throw new HumanInterventionRequiredError(event);
}

async function loadBrowser(): Promise<{ chromium: { launch(options: { headless: boolean }): Promise<BrowserRuntime>, launchPersistentContext(userDataDir: string, options: any): Promise<BrowserRuntime> } } | null> {
  if (!playwrightModulePromise) {
    playwrightModulePromise = import("playwright")
      .then(
        (module) =>
          module as any
      )
      .catch(() => null);
  }

  return playwrightModulePromise as any;
}

async function getBrowserRuntime(): Promise<BrowserRuntime | null> {
  if (!browserRuntimePromise) {
    browserRuntimePromise = loadBrowser().then(async (browser) => {
      if (!browser) return null;
      try {
        console.log("[Claw OS Browser] Binding to local Chrome Profile...");
        return await browser.chromium.launchPersistentContext(
            "C:\\Users\\dixon\\AppData\\Local\\Google\\Chrome\\User Data", 
            {
                channel: "chrome",
                headless: false,
                args: ["--profile-directory=Default"]
            }
        );
      } catch (e) {
        console.error("[Claw OS Browser] Persistent profile locked (Chrome running?), falling back to headless.", e);
        return await browser.chromium.launch({ headless: true });
      }
    });
  }

  return browserRuntimePromise;
}

async function closeBrowserRuntime(): Promise<void> {
  if (!browserRuntimePromise) {
    return;
  }

  const runtime = await browserRuntimePromise;
  browserRuntimePromise = null;
  if (runtime) {
    await runtime.close();
  }
}

process.once("exit", () => {
  void closeBrowserRuntime();
});

async function saveActionScreenshot(
  page: BrowserPage,
  sessionId: string,
  action: string
): Promise<ScreenshotRecord | null> {
  try {
    const screenshotDir = await mkdtemp(join(tmpdir(), "claw-screenshot-"));
    const screenshotPath = join(screenshotDir, `${sessionId}-${action}-${Date.now()}.png`);
    await page.screenshot({ path: screenshotPath, fullPage: true });
    return {
      path: screenshotPath,
      timestamp: new Date().toISOString(),
      action,
      url: page.url()
    };
  } catch {
    return null;
  }
}

async function resolveCaptchaScreenshotPath(sessionId: string): Promise<string> {
  const screenshotRoot =
    process.env.CLAW_SCREENSHOT_DIR ??
    join(process.env.CLAW_STATE_DIR ?? tmpdir(), "screenshots");
  const sessionDir = join(screenshotRoot, sessionId);
  await mkdir(sessionDir, { recursive: true });
  return join(sessionDir, "captcha.png");
}

async function consumeOverride(context: BrowserContext): Promise<Record<string, unknown> | null> {
  const override = await context.consumeOverride?.();
  if (!override) {
    return null;
  }

  if (
    typeof override === "object" &&
    override !== null &&
    "result" in override &&
    typeof (override as { result?: unknown }).result === "object"
  ) {
    return (override as { result: Record<string, unknown> }).result;
  }

  if (typeof override === "object" && override !== null) {
    return override as Record<string, unknown>;
  }

  return { value: override };
}
