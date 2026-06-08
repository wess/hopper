// Account / credential IPC: registry sign-in (so push/pull need no `docker
// login` CLI) and GitHub connection (search limits + private repos + GHCR).

import { handle } from "@basket/ipc";
import * as ch from "../../shared/channels.ts";
import { githubConnect, githubDisconnect, githubStatus } from "./github.ts";
import { registryConnections, registryLogin, registryLogout } from "./registry.ts";

export { githubToken } from "./github.ts";

export const registerAuthHandlers = (): void => {
  handle(ch.registryLogin, (input) => registryLogin(input));
  handle(ch.registryLogout, async ({ server }) => {
    await registryLogout(server);
  });
  handle(ch.registryConnections, () => registryConnections());

  handle(ch.githubConnect, ({ token }) => githubConnect(token));
  handle(ch.githubDisconnect, () => githubDisconnect());
  handle(ch.githubStatus, () => githubStatus());
};
