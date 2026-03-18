import test from "node:test";
import assert from "node:assert/strict";

import { captureScreenshot, findElementByVision } from "./vision.ts";

test("captureScreenshot returns a base64 screenshot buffer from a mock page", async () => {
  const mockPage = {
    screenshot: async (options: { fullPage: boolean; type: string }) => {
      assert.equal(options.type, "png");
      assert.equal(options.fullPage, true);
      return Buffer.from("fake-png-data");
    }
  };

  const result = await captureScreenshot(mockPage as never);
  assert.ok(Buffer.isBuffer(result));
  assert.equal(result.toString(), "fake-png-data");
});

test("findElementByVision calls vision LLM with screenshot and returns coordinates", async () => {
  let capturedRequest: Record<string, unknown> | null = null;

  const mockVisionBridge = async (request: Record<string, unknown>): Promise<unknown> => {
    capturedRequest = request;
    return { x: 320, y: 240, confidence: 0.92, description: "blue submit button" };
  };

  const mockScreenshot = Buffer.from("fake-screenshot-png");
  const result = await findElementByVision({
    screenshot: mockScreenshot,
    instruction: "Click the blue submit button",
    visionBridge: mockVisionBridge
  });

  assert.ok(capturedRequest);
  assert.equal(capturedRequest!.instruction, "Click the blue submit button");
  assert.equal(typeof capturedRequest!.screenshot_base64, "string");
  assert.equal((result as { x: number }).x, 320);
  assert.equal((result as { y: number }).y, 240);
  assert.equal((result as { confidence: number }).confidence, 0.92);
});

test("findElementByVision returns null when vision confidence is below threshold", async () => {
  const lowConfidenceBridge = async (): Promise<unknown> => {
    return { x: 10, y: 10, confidence: 0.2, description: "uncertain" };
  };

  const result = await findElementByVision({
    screenshot: Buffer.from("fake"),
    instruction: "Find the hidden element",
    visionBridge: lowConfidenceBridge,
    confidenceThreshold: 0.5
  });

  assert.equal(result, null);
});
