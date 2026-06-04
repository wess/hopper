// Small display formatters shared across views.

export const bytes = (n: number): string => {
  if (n < 0) return "—";
  if (n === 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.min(units.length - 1, Math.floor(Math.log(n) / Math.log(1024)));
  const v = n / 1024 ** i;
  return `${v >= 100 || i === 0 ? Math.round(v) : v.toFixed(1)} ${units[i]}`;
};

export const percent = (n: number): string => `${n.toFixed(n >= 10 ? 0 : 1)}%`;

// Relative time from a unix-seconds timestamp.
export const ago = (unixSeconds: number): string => {
  if (!unixSeconds) return "—";
  const diff = Date.now() / 1000 - unixSeconds;
  if (diff < 60) return "just now";
  const mins = Math.floor(diff / 60);
  if (mins < 60) return `${mins}m ago`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  if (days < 30) return `${days}d ago`;
  const months = Math.floor(days / 30);
  if (months < 12) return `${months}mo ago`;
  return `${Math.floor(months / 12)}y ago`;
};

// Relative time from an ISO-ish string (volumes/networks `CreatedAt`).
export const agoIso = (iso: string): string => {
  if (!iso) return "—";
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return iso;
  return ago(t / 1000);
};

// Short docker id — strips the `sha256:` prefix and clips to 12 chars.
export const shortId = (id: string): string => id.replace(/^sha256:/, "").slice(0, 12);

// First repo tag, or a short id fallback for dangling images.
export const imageName = (repoTags: readonly string[], id: string): string =>
  repoTags[0] ?? `<none>:${shortId(id)}`;

export const formatPorts = (
  ports: readonly { privatePort: number; publicPort?: number; type: string }[],
): string => {
  const seen = new Set<string>();
  const parts: string[] = [];
  for (const p of ports) {
    const s = p.publicPort
      ? `${p.publicPort}:${p.privatePort}/${p.type}`
      : `${p.privatePort}/${p.type}`;
    if (!seen.has(s)) {
      seen.add(s);
      parts.push(s);
    }
  }
  return parts.join(", ");
};
