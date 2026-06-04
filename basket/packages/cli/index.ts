export { addCommand } from "./add/index.ts";
export type { CommandDef, FlagDef, ParsedArgs } from "./command/index.ts";
export { cli, command, flag, parseArgs } from "./command/index.ts";
export { buildCommand, bundleCommand, devCommand } from "./dev/index.ts";
export { docsCommand } from "./docs/index.ts";
export { doctorCommand } from "./doctor/index.ts";
export { initCommand } from "./init/index.ts";
export type { ScaffoldOptions } from "./init/scaffold.ts";
export { scaffold } from "./init/scaffold.ts";
