import { command } from "../command/index.ts";

const which = async (cmd: string): Promise<string | undefined> => {
  const proc = Bun.spawn(["which", cmd], { stdout: "pipe", stderr: "ignore" });
  await proc.exited;
  if (proc.exitCode !== 0) return undefined;
  return (await new Response(proc.stdout).text()).trim();
};

export const doctorCommand = command("doctor", {
  description: "Check that bun and butter are available",
  run: async () => {
    const bun = await which("bun");
    console.log(`bun:    ${bun ?? "❌ not found"}`);

    const bunxButter = Bun.spawn(["bunx", "butter", "doctor"], {
      stdin: "ignore",
      stdout: "inherit",
      stderr: "inherit",
    });
    await bunxButter.exited;
  },
});
