import { command } from "../command/index.ts";

export const devCommand = command("dev", {
  description: "Run the app via butter dev (must be in a basket project)",
  run: async () => {
    const proc = Bun.spawn(["bunx", "butter", "dev"], {
      stdin: "inherit",
      stdout: "inherit",
      stderr: "inherit",
      env: { ...process.env },
    });
    await proc.exited;
    process.exit(proc.exitCode ?? 0);
  },
});

export const buildCommand = command("build", {
  description: "Compile to a single binary via butter compile",
  run: async () => {
    const proc = Bun.spawn(["bunx", "butter", "compile"], {
      stdin: "inherit",
      stdout: "inherit",
      stderr: "inherit",
      env: { ...process.env },
    });
    await proc.exited;
    process.exit(proc.exitCode ?? 0);
  },
});

export const bundleCommand = command("bundle", {
  description: "Bundle the compiled binary into an OS-native package",
  run: async () => {
    const proc = Bun.spawn(["bunx", "butter", "bundle"], {
      stdin: "inherit",
      stdout: "inherit",
      stderr: "inherit",
      env: { ...process.env },
    });
    await proc.exited;
    process.exit(proc.exitCode ?? 0);
  },
});
