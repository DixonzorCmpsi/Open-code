const DEFAULT_MAX_PER_SECOND = 100;
const DEFAULT_MAX_BUCKETS = 10_000;
const DEFAULT_BUCKET_TTL_MS = 5 * 60 * 1_000;
const DEFAULT_CLEANUP_INTERVAL_MS = 60_000;

interface Bucket {
  tokens: number;
  lastRefill: number;
}

interface RateLimiterOptions {
  now?: () => number;
  cleanupIntervalMs?: number;
  bucketTtlMs?: number;
  maxBuckets?: number;
}

export interface RateLimiter {
  check(key: string): boolean;
  close(): void;
  __testing: {
    bucketCount(): number;
    cleanupNow(): void;
  };
}

export function createRateLimiter(
  maxPerSecond = DEFAULT_MAX_PER_SECOND,
  options: RateLimiterOptions = {}
): RateLimiter {
  const now = options.now ?? Date.now;
  const cleanupIntervalMs = options.cleanupIntervalMs ?? DEFAULT_CLEANUP_INTERVAL_MS;
  const bucketTtlMs = options.bucketTtlMs ?? DEFAULT_BUCKET_TTL_MS;
  const maxBuckets = options.maxBuckets ?? DEFAULT_MAX_BUCKETS;
  const buckets = new Map<string, Bucket>();

  const cleanupBuckets = () => {
    const currentTime = now();
    for (const [key, bucket] of buckets) {
      if (currentTime - bucket.lastRefill > bucketTtlMs) {
        buckets.delete(key);
      }
    }

    if (buckets.size > maxBuckets) {
      const oldestEntries = [...buckets.entries()].sort(
        (left, right) => left[1].lastRefill - right[1].lastRefill
      );
      for (let index = 0; index < oldestEntries.length - maxBuckets; index += 1) {
        buckets.delete(oldestEntries[index]![0]);
      }
    }
  };

  const cleanupTimer = setInterval(cleanupBuckets, cleanupIntervalMs);
  cleanupTimer.unref?.();

  return {
    check(key: string): boolean {
      const currentTime = now();
      let bucket = buckets.get(key);
      if (!bucket) {
        bucket = {
          tokens: maxPerSecond,
          lastRefill: currentTime
        };
        buckets.set(key, bucket);
      }

      const elapsedSeconds = (currentTime - bucket.lastRefill) / 1_000;
      bucket.tokens = Math.min(maxPerSecond, bucket.tokens + elapsedSeconds * maxPerSecond);
      bucket.lastRefill = currentTime;

      if (bucket.tokens < 1) {
        return false;
      }

      bucket.tokens -= 1;
      return true;
    },
    close(): void {
      clearInterval(cleanupTimer);
    },
    __testing: {
      bucketCount(): number {
        return buckets.size;
      },
      cleanupNow(): void {
        cleanupBuckets();
      }
    }
  };
}
