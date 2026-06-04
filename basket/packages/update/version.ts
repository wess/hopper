export const parseVersion = (v: string): number[] =>
  v
    .replace(/^v/i, "")
    .split(".")
    .map((n) => Number.parseInt(n, 10) || 0);

export const compareVersions = (a: string, b: string): -1 | 0 | 1 => {
  const A = parseVersion(a);
  const B = parseVersion(b);
  const len = Math.max(A.length, B.length);
  for (let i = 0; i < len; i++) {
    const av = A[i] ?? 0;
    const bv = B[i] ?? 0;
    if (av > bv) return 1;
    if (av < bv) return -1;
  }
  return 0;
};

export const isNewer = (remote: string, current: string): boolean => compareVersions(remote, current) === 1;
