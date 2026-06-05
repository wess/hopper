// Exponential backoff with a cap, used to respawn a crashed engine helper
// without hammering. Pure so the policy is unit-tested independently of timers.

export type BackoffOptions = {
  readonly baseMs?: number;
  readonly maxMs?: number;
  readonly factor?: number;
};

// Delay before retry `attempt` (1-based). attempt<=0 yields 0.
export const backoffDelay = (attempt: number, opts: BackoffOptions = {}): number => {
  const base = opts.baseMs ?? 500;
  const max = opts.maxMs ?? 30_000;
  const factor = opts.factor ?? 2;
  if (attempt <= 0) return 0;
  return Math.min(base * factor ** (attempt - 1), max);
};

// Whether another restart should be attempted.
export const shouldRetry = (attempt: number, maxAttempts: number): boolean => attempt < maxAttempts;
