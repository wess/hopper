import { existsSync, mkdirSync, unlinkSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

export type AutoLaunchOptions = {
  readonly appId: string;
  readonly displayName?: string;
  readonly args?: readonly string[];
  readonly exePath?: string;
};

const SAFE = /^[a-zA-Z0-9._-]+$/;

const validate = (id: string): void => {
  if (!SAFE.test(id)) throw new Error("autolaunch: appId must match [a-zA-Z0-9._-]");
};

const macPlistPath = (id: string): string => join(homedir(), "Library", "LaunchAgents", `${id}.plist`);

const linuxDesktopPath = (id: string): string => join(homedir(), ".config", "autostart", `${id}.desktop`);

const macPlist = (id: string, exe: string, args: readonly string[]): string => {
  const argLines = [exe, ...args]
    .map((a) => `    <string>${a.replace(/&/g, "&amp;").replace(/</g, "&lt;")}</string>`)
    .join("\n");
  return `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>${id}</string>
  <key>ProgramArguments</key>
  <array>
${argLines}
  </array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><false/>
</dict>
</plist>
`;
};

const linuxDesktop = (id: string, name: string, exe: string, args: readonly string[]): string => {
  const cmd = [exe, ...args].map((a) => (a.includes(" ") ? `"${a}"` : a)).join(" ");
  return `[Desktop Entry]
Type=Application
Name=${name}
Exec=${cmd}
X-GNOME-Autostart-enabled=true
Hidden=false
`;
};

const resolveExe = (opts: AutoLaunchOptions): string => opts.exePath ?? process.execPath;

export const enable = async (opts: AutoLaunchOptions): Promise<void> => {
  validate(opts.appId);
  const exe = resolveExe(opts);
  const args = opts.args ?? [];

  if (process.platform === "darwin") {
    mkdirSync(join(homedir(), "Library", "LaunchAgents"), { recursive: true });
    const path = macPlistPath(opts.appId);
    writeFileSync(path, macPlist(opts.appId, exe, args));
    await Bun.$`launchctl unload ${path}`.quiet().nothrow();
    await Bun.$`launchctl load ${path}`.quiet();
    return;
  }

  if (process.platform === "linux") {
    mkdirSync(join(homedir(), ".config", "autostart"), { recursive: true });
    writeFileSync(linuxDesktopPath(opts.appId), linuxDesktop(opts.appId, opts.displayName ?? opts.appId, exe, args));
    return;
  }

  if (process.platform === "win32") {
    const value = [exe, ...args].map((a) => `"${a.replace(/"/g, '""')}"`).join(" ");
    await Bun.$`reg add HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run /v ${opts.appId} /t REG_SZ /d ${value} /f`.quiet();
    return;
  }

  throw new Error(`autolaunch: unsupported platform ${process.platform}`);
};

export const disable = async (appId: string): Promise<void> => {
  validate(appId);

  if (process.platform === "darwin") {
    const path = macPlistPath(appId);
    if (existsSync(path)) {
      await Bun.$`launchctl unload ${path}`.quiet().nothrow();
      unlinkSync(path);
    }
    return;
  }

  if (process.platform === "linux") {
    const path = linuxDesktopPath(appId);
    if (existsSync(path)) unlinkSync(path);
    return;
  }

  if (process.platform === "win32") {
    await Bun.$`reg delete HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run /v ${appId} /f`.quiet().nothrow();
    return;
  }

  throw new Error(`autolaunch: unsupported platform ${process.platform}`);
};

export const isEnabled = async (appId: string): Promise<boolean> => {
  validate(appId);
  if (process.platform === "darwin") return existsSync(macPlistPath(appId));
  if (process.platform === "linux") return existsSync(linuxDesktopPath(appId));
  if (process.platform === "win32") {
    const out = await Bun.$`reg query HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run /v ${appId}`.quiet().nothrow();
    return out.exitCode === 0 && out.stdout.toString().includes(appId);
  }
  return false;
};
