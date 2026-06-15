// Build the compose argument vector. It always starts with "compose"; runner.ts
// strips that for the standalone / v1 binaries. Global flags
// (-f / -p / --env-file / --profile) precede the subcommand, per the compose CLI.

import type { ComposeAction, ComposeOptions, ComposeTarget } from "../../../shared/types.ts";

export const composeArgs = (
  action: ComposeAction,
  target: ComposeTarget = {},
  options: ComposeOptions = {},
): string[] => {
  const args = ["compose"];
  for (const f of target.files ?? []) {
    if (f.trim()) args.push("-f", f.trim());
  }
  if (target.project?.trim()) args.push("-p", target.project.trim());
  if (target.envFile?.trim()) args.push("--env-file", target.envFile.trim());
  for (const p of options.profiles ?? []) {
    if (p.trim()) args.push("--profile", p.trim());
  }

  switch (action) {
    case "up":
      args.push("up", "-d");
      if (options.removeOrphans ?? true) args.push("--remove-orphans");
      if (options.build) args.push("--build");
      if (options.forceRecreate) args.push("--force-recreate");
      break;
    case "down":
      args.push("down");
      if (options.volumes) args.push("--volumes");
      if (options.removeOrphans) args.push("--remove-orphans");
      if (options.rmi) args.push("--rmi", options.rmi);
      break;
    case "remove":
      // Full teardown: containers + networks + named volumes + orphans.
      args.push("down", "--volumes", "--remove-orphans");
      break;
    case "start":
      args.push("start");
      break;
    case "stop":
      args.push("stop");
      break;
    case "restart":
      args.push("restart");
      break;
  }
  return args;
};
