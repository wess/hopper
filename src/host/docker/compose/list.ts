// Reconstruct compose projects (stacks) from container labels. This works
// against any engine without a running compose CLI — every compose-managed
// container carries `com.docker.compose.*` labels we group on.

import type {
  ComposeProject,
  ComposeService,
  ComposeStackStatus,
  Container,
} from "../../../shared/types.ts";
import * as containers from "../containers.ts";

const CONFIG_FILES_LABEL = "com.docker.compose.project.config_files";
const WORKING_DIR_LABEL = "com.docker.compose.project.working_dir";

const aggregate = (
  services: readonly ComposeService[],
): { status: ComposeStackStatus; running: number } => {
  const running = services.filter((s) => s.state === "running").length;
  const status: ComposeStackStatus =
    running === 0 ? "stopped" : running === services.length ? "running" : "partial";
  return { status, running };
};

const toProject = (name: string, members: readonly Container[]): ComposeProject => {
  const services: ComposeService[] = members
    .map((c) => ({
      service: c.composeService ?? c.name,
      containerId: c.id,
      containerName: c.name,
      state: c.state,
      status: c.status,
      image: c.image,
      ports: c.ports,
    }))
    .sort((a, b) => a.service.localeCompare(b.service));

  const { status, running } = aggregate(services);
  const configFiles = (
    members.find((c) => c.labels[CONFIG_FILES_LABEL])?.labels[CONFIG_FILES_LABEL] ?? ""
  )
    .split(",")
    .map((f) => f.trim())
    .filter(Boolean);
  const workingDir =
    members.find((c) => c.labels[WORKING_DIR_LABEL])?.labels[WORKING_DIR_LABEL] ?? null;

  return { name, status, running, total: services.length, services, configFiles, workingDir };
};

export const listProjects = async (): Promise<ComposeProject[]> => {
  const all = await containers.list(true);
  const groups = new Map<string, Container[]>();
  for (const c of all) {
    if (!c.composeProject) continue;
    const arr = groups.get(c.composeProject) ?? [];
    arr.push(c);
    groups.set(c.composeProject, arr);
  }
  return [...groups.entries()]
    .map(([name, members]) => toProject(name, members))
    .sort((a, b) => a.name.localeCompare(b.name));
};
