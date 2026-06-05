// Orchestrate a Docker Desktop → Hopper migration: networks → volumes → images
// → containers, so a recreated container finds its deps already present. Each
// phase is ISOLATED — one phase's failure is reported but doesn't abort the
// rest — and per-item failures within a phase are reported and skipped. Strictly
// non-destructive: the source engine is only ever read.

import type { MigrationPlan, MigrationProgress } from "../../shared/types.ts";
import type { Endpoint } from "../docker/endpoint.ts";
import { migrateContainers } from "./containers.ts";
import type { Emit } from "./helpers.ts";
import { migrateImages } from "./images.ts";
import { migrateNetworks } from "./networks.ts";
import { resolveEnds } from "./scan.ts";
import { migrateVolumes } from "./volumes.ts";

const fail = (emit: Emit, error: string): { ok: false; error: string } => {
  emit({ phase: "error", item: "", done: 0, total: 0, message: error, error, finished: true });
  return { ok: false, error };
};

// Run one phase in isolation — a thrown error is reported and the migration
// continues to the next phase (images/containers don't depend on volumes, etc.).
const phase = async (
  emit: Emit,
  name: MigrationProgress["phase"],
  run: () => Promise<void>,
): Promise<void> => {
  try {
    await run();
  } catch (e) {
    emit({
      phase: name,
      item: "",
      done: 0,
      total: 0,
      message: `${name} phase failed`,
      error: String(e),
    });
  }
};

export const migrate = async (
  plan: MigrationPlan,
  emit: Emit,
): Promise<{ ok: boolean; error?: string }> => {
  // Pin to the exact source the UI scanned (if provided) so a daemon coming up
  // or down between scan and run can't silently switch which engine we read.
  const ends = await resolveEnds(plan.source as Endpoint | undefined);
  if ("error" in ends) return fail(emit, ends.error);

  const { source, dest } = ends;
  await phase(emit, "networks", () => migrateNetworks(source, dest, plan.networks, emit));
  await phase(emit, "volumes", () => migrateVolumes(source, dest, plan.volumes, emit));
  await phase(emit, "images", () => migrateImages(source, dest, plan.images, emit));
  await phase(emit, "containers", () => migrateContainers(source, dest, plan.containers, emit));

  emit({
    phase: "done",
    item: "",
    done: 0,
    total: 0,
    message: "Migration complete",
    finished: true,
  });
  return { ok: true };
};
