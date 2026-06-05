import Foundation

// Where the VM's pieces live on the host, plus its resource sizing. Paths are
// resolved from the environment Hopper sets (HOPPER_HOME for state, and the
// helper's own directory for the bundled kernel/initrd), with sane defaults so
// the helper is also runnable standalone for development.

struct VMPaths {
  let home: URL  // ~/.hopper — state root (created if absent)
  let kernel: URL  // bundled vmlinuz
  let initrd: URL  // bundled initramfs
  let disk: URL  // persistent data disk (created sparse if absent)
  let socket: URL  // host-side docker.sock we expose to Hopper
  let log: URL  // guest console log
}

struct VMConfig {
  let paths: VMPaths
  let cpuCount: Int
  let memoryBytes: UInt64
  let diskBytes: UInt64
  let guestPort: UInt32  // vsock port the guest forwards dockerd on
  let agentPort: UInt32  // vsock port the guest agent answers reclaim/stats on

  static func fromEnvironment() -> VMConfig {
    let env = ProcessInfo.processInfo.environment
    let fm = FileManager.default

    let home = URL(
      fileURLWithPath: env["HOPPER_HOME"]
        ?? (fm.homeDirectoryForCurrentUser.appendingPathComponent(".hopper").path)
    )
    // Kernel + initrd are data: in dev they sit next to the executable
    // (.build/release/), but in a bundled .app they live in Contents/Resources/
    // — data files can't live under Contents/MacOS/ (codesign rejects unsigned
    // "code" there). Prefer next-to-exe, else the bundle's Resources dir.
    let exe = Bundle.main.executableURL ?? URL(fileURLWithPath: CommandLine.arguments[0])
    let exeDir = exe.deletingLastPathComponent()
    let resources =
      exeDir
      .deletingLastPathComponent()  // .../Contents/MacOS
      .deletingLastPathComponent()  // .../Contents
      .appendingPathComponent("Resources")
    let assets =
      fm.fileExists(atPath: exeDir.appendingPathComponent("vmlinuz").path)
      ? exeDir : resources
    let run = home.appendingPathComponent("run", isDirectory: true)
    try? fm.createDirectory(at: run, withIntermediateDirectories: true)

    let paths = VMPaths(
      home: home,
      kernel: assets.appendingPathComponent("vmlinuz"),
      initrd: assets.appendingPathComponent("initrd"),
      disk: home.appendingPathComponent("data.img"),
      socket: run.appendingPathComponent("docker.sock"),
      log: home.appendingPathComponent("console.log")
    )

    let cores = ProcessInfo.processInfo.activeProcessorCount
    let cpu = Int(env["HOPPER_CPUS"] ?? "") ?? max(2, cores / 2)
    let memGiB = UInt64(env["HOPPER_MEMORY_GIB"] ?? "") ?? 4
    let diskGiB = UInt64(env["HOPPER_DISK_GIB"] ?? "") ?? 60
    let port = UInt32(env["HOPPER_VSOCK_PORT"] ?? "") ?? 2375
    let agent = UInt32(env["HOPPER_AGENT_PORT"] ?? "") ?? 2377

    return VMConfig(
      paths: paths,
      cpuCount: cpu,
      memoryBytes: memGiB * 1024 * 1024 * 1024,
      diskBytes: diskGiB * 1024 * 1024 * 1024,
      guestPort: port,
      agentPort: agent
    )
  }
}
