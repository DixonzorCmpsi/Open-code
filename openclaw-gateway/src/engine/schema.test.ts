import test from "node:test";
import assert from "node:assert/strict";

import { isSchemaDegraded } from "./schema.ts";

test("isSchemaDegraded: individual 0 is NOT degraded (legitimate data)", () => {
  // Per specs/07-OpenClaw-OS.md Section 2.4: individual 0 is valid
  assert.equal(
    isSchemaDegraded({ count: 0, name: "Alice", verified: true }),
    false
  );
});

test("isSchemaDegraded: individual false is NOT degraded (legitimate data)", () => {
  assert.equal(
    isSchemaDegraded({ verified: false, reason: "ID invalid", count: 5 }),
    false
  );
});

test("isSchemaDegraded: ALL leaves zero-values simultaneously IS degraded", () => {
  assert.equal(
    isSchemaDegraded({ url: "", confidence_score: 0, snippet: "", verified: false }),
    true
  );
});

test("isSchemaDegraded: mixed zero and non-zero is NOT degraded", () => {
  assert.equal(
    isSchemaDegraded({ url: "https://example.com", confidence_score: 0, snippet: "" }),
    false
  );
});

test("isSchemaDegraded: null value IS degraded", () => {
  assert.equal(isSchemaDegraded(null), true);
});

test("isSchemaDegraded: empty object IS degraded", () => {
  assert.equal(isSchemaDegraded({}), true);
});

test("isSchemaDegraded: nested all-zero object IS degraded", () => {
  assert.equal(
    isSchemaDegraded({ result: { url: "", score: 0 }, tags: [] }),
    true
  );
});
