// Workspace matching — pure predicates that decide whether a resource falls
// inside the active workspace scope. A null workspace is the built-in "all"
// scope and matches everything.

import type { Container, Workspace } from "../../shared/types.ts";

const safeRegex = (pattern: string): RegExp | null => {
  try {
    return new RegExp(pattern, "i");
  } catch {
    return null; // invalid regex → treat the predicate as absent
  }
};

export const matchesWorkspace = (c: Container, ws: Workspace | null): boolean => {
  if (!ws) return true;
  if (ws.composeProjects && ws.composeProjects.length > 0) {
    if (!c.composeProject || !ws.composeProjects.includes(c.composeProject)) return false;
  }
  if (ws.namePattern?.trim()) {
    const re = safeRegex(ws.namePattern);
    if (re && !re.test(c.name) && !re.test(c.image)) return false;
  }
  return true;
};

// Images carry no compose project, so only the name pattern scopes them.
export const imageMatchesWorkspace = (
  repoTags: readonly string[],
  ws: Workspace | null,
): boolean => {
  if (!ws?.namePattern?.trim()) return true;
  const re = safeRegex(ws.namePattern);
  if (!re) return true;
  return repoTags.some((t) => re.test(t));
};
