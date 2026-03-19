# Claw Visual Intelligence System

This document specifies the screenshot capture pipeline and multimodal LLM vision bridge used by Claw browser automation agents to interact with visual elements when CSS selectors fail.

---

## 1. Screenshot Capture Pipeline

### 1.1 Capture Triggers

Screenshots are captured in two scenarios:

| Trigger | When | Purpose |
|---------|------|---------|
| **Post-action** | After every `Browser.search()` and `Browser.navigate()` completes | Audit trail, debugging, visual state capture |
| **Selector fallback** | When a CSS selector lookup fails | Input to vision LLM for coordinate-based interaction |

### 1.2 DOM Stability Requirement

**MUST:** Before capturing a screenshot for vision analysis, the page MUST be visually stable. Capture screenshots ONLY after the DOM has "settled" — meaning no animations, spinners, or layout shifts are in progress.

**Implementation:** Wait until `requestAnimationFrame` reports less than 0.1% pixel delta between 3 consecutive frames, OR fall back to a 2-second delay if the stability check is unavailable (headless environments).

**Rationale:** If an LLM interprets a screenshot mid-animation, it will report incorrect coordinates for elements that are still moving.

### 1.3 Capture Format

- Format: PNG
- Maximum resolution: 1920x1080 (downsample if viewport is larger)
- Full-page: `true` (capture entire scrollable content)
- Storage: `{tmpdir}/claw-screenshot-{random}/{session_id}-{action}-{timestamp}.png`

### 1.4 Checkpoint Integration

- Screenshot file paths are stored in checkpoint events as metadata (not inline base64)
- Vision analysis results (coordinates, confidence) are checkpointed alongside the screenshot path
- On session resumption, the screenshot files may no longer exist — the checkpoint stores the result, not the image

---

## 2. Vision LLM Bridge

### 2.1 Purpose

When an agent needs to interact with a visual element on a web page but cannot locate it via CSS selectors (e.g., canvas-rendered buttons, image-based navigation, custom web components), the vision bridge sends a screenshot to a multimodal LLM to determine the pixel coordinates of the target element.

### 2.2 Supported Providers

| Provider | Model | Image Input Format |
|----------|-------|-------------------|
| Anthropic | `claude-sonnet-4-6` (default) | `content[].type: "image"` with `source.type: "base64"` |
| OpenAI | `gpt-4o` (default) | `content[].type: "image_url"` with `data:image/png;base64,...` |

The provider is selected based on available API keys (`ANTHROPIC_API_KEY` or `OPENAI_API_KEY`). The model can be overridden via `CLAW_VISION_MODEL` environment variable.

### 2.3 Vision Request

The bridge sends the screenshot as a base64-encoded PNG alongside a structured prompt:

```
You are a visual UI element locator for browser automation.
Task: {instruction}

Analyze the screenshot and return a JSON object with:
  "x": pixel X coordinate of the element center
  "y": pixel Y coordinate of the element center
  "confidence": 0.0-1.0 confidence score
  "description": brief description of the matched element

Return ONLY valid JSON, no markdown or explanation.
```

### 2.4 Vision Response

```typescript
interface VisionCoordinates {
  x: number;       // Pixel X coordinate
  y: number;       // Pixel Y coordinate
  confidence: number;  // 0.0 to 1.0
  description: string; // Human-readable description of matched element
}
```

---

## 3. Confidence Thresholds

| Confidence Range | Action |
|-----------------|--------|
| >= 0.7 | Accept coordinates, proceed with click/interaction |
| 0.3 - 0.7 | Retry with higher resolution screenshot OR different prompt |
| < 0.3 | Escalate to `HumanInterventionRequired` event |

The default threshold (0.7) can be overridden per-request via the `confidenceThreshold` parameter.

**NEVER:** Accept vision coordinates with confidence below 0.3. The risk of clicking the wrong element is too high.

---

## 4. CAPTCHA Detection

When the browser tool detects a CAPTCHA or verification challenge (keywords: "captcha", "cloudflare", "verify you are human" in page HTML):

1. Capture a full-page screenshot
2. Suspend the session into the checkpoint store with status `waiting_human`
3. Emit a `HumanInterventionRequired` event (via WebSocket or REST response)
4. Include the screenshot path and page URL in the event metadata
5. Wait for a human override via `POST /sessions/{id}/override`
6. Resume execution with the override payload

This flow is defined in `specs/07-Claw-OS.md` and `specs/10-GAN-Final-Audit.md` (Attack 3).

---

## 5. Limitations

- Vision-based interaction is significantly slower than CSS selector-based interaction (~2-5 seconds per LLM call vs <100ms for selector lookup)
- Vision accuracy degrades on pages with dense, overlapping elements
- The vision bridge requires an active LLM API key — without one, the function returns `null` and the agent must fall back to human intervention
- Screenshots consume disk space; the gateway does not automatically clean them up (the temp directory is cleaned on process exit)
