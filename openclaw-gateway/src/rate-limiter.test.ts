import assert from "node:assert/strict";
import test from "node:test";

import { createRateLimiter } from "./rate-limiter.ts";

test("rate limiter allows requests under the configured limit", () => {
  const limiter = createRateLimiter(100);
  try {
    for (let attempt = 0; attempt < 50; attempt += 1) {
      assert.equal(limiter.check("127.0.0.1"), true);
    }
  } finally {
    limiter.close();
  }
});

test("rate limiter blocks requests over the configured limit", () => {
  const limiter = createRateLimiter(100);
  try {
    const results = Array.from({ length: 150 }, () => limiter.check("127.0.0.1"));
    assert.ok(results.some((allowed) => !allowed));
  } finally {
    limiter.close();
  }
});

test("rate limiter refills tokens over time", () => {
  let now = 0;
  const limiter = createRateLimiter(2, {
    now: () => now
  });

  try {
    assert.equal(limiter.check("127.0.0.1"), true);
    assert.equal(limiter.check("127.0.0.1"), true);
    assert.equal(limiter.check("127.0.0.1"), false);

    now = 1_000;
    assert.equal(limiter.check("127.0.0.1"), true);
  } finally {
    limiter.close();
  }
});

test("rate limiter cleanup removes stale entries", () => {
  let now = 0;
  const limiter = createRateLimiter(10, {
    now: () => now
  });

  try {
    for (const key of ["a", "b", "c", "d", "e"]) {
      assert.equal(limiter.check(key), true);
    }
    assert.equal(limiter.__testing.bucketCount(), 5);

    now = 6 * 60 * 1_000;
    limiter.__testing.cleanupNow();

    assert.equal(limiter.__testing.bucketCount(), 0);
  } finally {
    limiter.close();
  }
});
