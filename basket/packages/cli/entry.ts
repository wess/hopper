#!/usr/bin/env bun
import { addCommand } from "./add/index.ts";
import { cli } from "./command/index.ts";
import { buildCommand, bundleCommand, devCommand } from "./dev/index.ts";
import { docsCommand } from "./docs/index.ts";
import { doctorCommand } from "./doctor/index.ts";
import { initCommand } from "./init/index.ts";

cli("basket", [initCommand, devCommand, buildCommand, bundleCommand, addCommand, docsCommand, doctorCommand]);
