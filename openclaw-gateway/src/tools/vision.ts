/**
 * Visual Intelligence module for Claw browser automation.
 *
 * When a CSS selector fails, agents can pass a screenshot to a multimodal LLM
 * (Claude Sonnet 4.6 / GPT-4o) to determine coordinate-based clicks.
 */

type BrowserPageLike = {
  screenshot(options: { fullPage: boolean; type: string }): Promise<Buffer>;
};

export interface VisionCoordinates {
  x: number;
  y: number;
  confidence: number;
  description: string;
}

export interface FindElementRequest {
  screenshot: Buffer;
  instruction: string;
  visionBridge?: (request: Record<string, unknown>) => Promise<unknown>;
  confidenceThreshold?: number;
}

export async function captureScreenshot(page: BrowserPageLike): Promise<Buffer> {
  return page.screenshot({ fullPage: true, type: "png" });
}

export async function findElementByVision(
  request: FindElementRequest
): Promise<VisionCoordinates | null> {
  const threshold = request.confidenceThreshold ?? 0.5;
  const bridge = request.visionBridge ?? callDefaultVisionBridge;

  const result = await bridge({
    screenshot_base64: request.screenshot.toString("base64"),
    instruction: request.instruction
  }) as VisionCoordinates;

  if (!result || typeof result.confidence !== "number" || result.confidence < threshold) {
    return null;
  }

  return result;
}

/**
 * Calls the configured multimodal LLM provider to analyze a screenshot.
 * Supports both Anthropic (Claude) and OpenAI (GPT-4o) vision APIs.
 */
async function callDefaultVisionBridge(
  request: Record<string, unknown>
): Promise<VisionCoordinates | null> {
  if (process.env.ANTHROPIC_API_KEY) {
    return callAnthropicVision(request);
  }

  if (process.env.OPENAI_API_KEY) {
    return callOpenAIVision(request);
  }

  return null;
}

async function callAnthropicVision(
  request: Record<string, unknown>
): Promise<VisionCoordinates | null> {
  const response = await fetch("https://api.anthropic.com/v1/messages", {
    method: "POST",
    headers: {
      "content-type": "application/json",
      "x-api-key": process.env.ANTHROPIC_API_KEY!,
      "anthropic-version": "2025-01-01"
    },
    body: JSON.stringify({
      model: process.env.CLAW_VISION_MODEL ?? "claude-sonnet-4-6",
      max_tokens: 512,
      messages: [
        {
          role: "user",
          content: [
            {
              type: "image",
              source: {
                type: "base64",
                media_type: "image/png",
                data: request.screenshot_base64
              }
            },
            {
              type: "text",
              text: buildVisionPrompt(String(request.instruction))
            }
          ]
        }
      ]
    })
  });

  if (!response.ok) {
    return null;
  }

  return parseVisionResponse(await response.json());
}

async function callOpenAIVision(
  request: Record<string, unknown>
): Promise<VisionCoordinates | null> {
  const endpoint = process.env.OPENAI_BASE_URL ?? "https://api.openai.com/v1/chat/completions";
  const response = await fetch(endpoint, {
    method: "POST",
    headers: {
      "content-type": "application/json",
      authorization: `Bearer ${process.env.OPENAI_API_KEY}`
    },
    body: JSON.stringify({
      model: process.env.CLAW_VISION_MODEL ?? "gpt-4o",
      max_tokens: 512,
      messages: [
        {
          role: "user",
          content: [
            {
              type: "image_url",
              image_url: {
                url: `data:image/png;base64,${request.screenshot_base64}`
              }
            },
            {
              type: "text",
              text: buildVisionPrompt(String(request.instruction))
            }
          ]
        }
      ],
      response_format: { type: "json_object" }
    })
  });

  if (!response.ok) {
    return null;
  }

  const payload = await response.json();
  const text = payload.choices?.[0]?.message?.content;
  if (!text) {
    return null;
  }

  try {
    return parseVisionCoordinates(JSON.parse(text));
  } catch {
    return null;
  }
}

function buildVisionPrompt(instruction: string): string {
  return [
    "You are a visual UI element locator for browser automation.",
    `Task: ${instruction}`,
    "",
    "Analyze the screenshot and return a JSON object with:",
    '  "x": pixel X coordinate of the element center',
    '  "y": pixel Y coordinate of the element center',
    '  "confidence": 0.0-1.0 confidence score',
    '  "description": brief description of the matched element',
    "",
    "Return ONLY valid JSON, no markdown or explanation."
  ].join("\n");
}

function parseVisionResponse(payload: Record<string, unknown>): VisionCoordinates | null {
  const content = (payload.content as Array<{ text?: string }>)?.[0]?.text;
  if (!content) {
    return null;
  }

  try {
    return parseVisionCoordinates(JSON.parse(content));
  } catch {
    return null;
  }
}

function parseVisionCoordinates(raw: unknown): VisionCoordinates | null {
  if (typeof raw !== "object" || raw === null) {
    return null;
  }

  const data = raw as Record<string, unknown>;
  if (typeof data.x !== "number" || typeof data.y !== "number") {
    return null;
  }

  return {
    x: data.x,
    y: data.y,
    confidence: typeof data.confidence === "number" ? data.confidence : 0,
    description: typeof data.description === "string" ? data.description : ""
  };
}
