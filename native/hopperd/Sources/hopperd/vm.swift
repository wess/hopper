import Foundation
import Virtualization

// The Linux guest. The OO shape here (an NSObject delegate, a retained
// VZVirtualMachine driven on a dedicated queue) is mandated by
// Virtualization.framework — VM operations must happen on the queue the
// machine was created with.
final class GuestVM: NSObject, VZVirtualMachineDelegate {
  private let config: VMConfig
  private let queue = DispatchQueue(label: "io.wess.hopper.vm")
  private var machine: VZVirtualMachine?
  private var bridge: SocketBridge?
  private var onExit: ((String) -> Void)?
  // Retained so the console-capture pipe + log handle stay alive for the VM.
  private var consolePipe: Pipe?
  private var consoleLog: FileHandle?

  init(config: VMConfig) {
    self.config = config
  }

  // Ensure the persistent data disk exists, created sparse at the configured
  // size (APFS keeps it thin until written; the guest grows into it).
  private func ensureDisk() throws {
    let fm = FileManager.default
    if fm.fileExists(atPath: config.paths.disk.path) { return }
    fm.createFile(atPath: config.paths.disk.path, contents: nil)
    let handle = try FileHandle(forWritingTo: config.paths.disk)
    try handle.truncate(atOffset: config.diskBytes)
    try handle.close()
  }

  private func clampMemory(_ requested: UInt64) -> UInt64 {
    let lo = VZVirtualMachineConfiguration.minimumAllowedMemorySize
    let hi = VZVirtualMachineConfiguration.maximumAllowedMemorySize
    return min(max(requested, lo), hi)
  }

  private func buildConfiguration() throws -> VZVirtualMachineConfiguration {
    try ensureDisk()

    // Fail clearly if the boot assets weren't bundled, rather than letting
    // Virtualization throw a vague error.
    let fm = FileManager.default
    guard fm.fileExists(atPath: config.paths.kernel.path) else {
      throw NSError(
        domain: "hopperd", code: 1,
        userInfo: [
          NSLocalizedDescriptionKey:
            "guest kernel missing at \(config.paths.kernel.path) — needs a raw uncompressed arm64 Image"
        ])
    }
    guard fm.fileExists(atPath: config.paths.initrd.path) else {
      throw NSError(
        domain: "hopperd", code: 2,
        userInfo: [NSLocalizedDescriptionKey: "guest initrd missing at \(config.paths.initrd.path)"])
    }

    // VZLinuxBootLoader needs a RAW, uncompressed arm64 Image. A gzipped
    // vmlinuz is the most common cause of a vague "invalid configuration".
    if let handle = try? FileHandle(forReadingFrom: config.paths.kernel) {
      let head = try? handle.read(upToCount: 2)
      try? handle.close()
      if let head, head.count == 2, head[0] == 0x1f, head[1] == 0x8b {
        throw NSError(
          domain: "hopperd", code: 3,
          userInfo: [
            NSLocalizedDescriptionKey:
              "guest kernel at \(config.paths.kernel.path) is gzip-compressed — Virtualization needs a raw uncompressed arm64 Image (see native/guest/readme.md)"
          ])
      }
    }

    let vz = VZVirtualMachineConfiguration()

    let boot = VZLinuxBootLoader(kernelURL: config.paths.kernel)
    boot.initialRamdiskURL = config.paths.initrd
    // The OS lives in the initramfs (shipped, read-only). /dev/vda is the
    // persistent data disk; the guest init formats it on first boot and mounts
    // it at /var/lib/docker. So no root= — the kernel runs /init from initrd.
    // (No `quiet` — we want boot output on hvc0 captured to console.log.)
    boot.commandLine = "console=hvc0 loglevel=7"
    vz.bootLoader = boot

    let cpuLo = VZVirtualMachineConfiguration.minimumAllowedCPUCount
    let cpuHi = VZVirtualMachineConfiguration.maximumAllowedCPUCount
    vz.cpuCount = min(max(config.cpuCount, cpuLo), cpuHi)
    vz.memorySize = clampMemory(config.memoryBytes)

    // Console -> capture guest output through a pipe and persist it to
    // console.log (a readability handler flushes each chunk), so the boot is
    // visible even on a windowed launch where stderr is discarded.
    FileManager.default.createFile(atPath: config.paths.log.path, contents: nil)
    let logHandle = try FileHandle(forWritingTo: config.paths.log)
    let pipe = Pipe()
    pipe.fileHandleForReading.readabilityHandler = { handle in
      let data = handle.availableData
      if !data.isEmpty { try? logHandle.write(contentsOf: data) }
    }
    consoleLog = logHandle
    consolePipe = pipe
    let console = VZVirtioConsoleDeviceSerialPortConfiguration()
    console.attachment = VZFileHandleSerialPortAttachment(
      fileHandleForReading: nil,
      fileHandleForWriting: pipe.fileHandleForWriting
    )
    vz.serialPorts = [console]

    // Persistent data disk.
    let attachment = try VZDiskImageStorageDeviceAttachment(
      url: config.paths.disk,
      readOnly: false
    )
    vz.storageDevices = [VZVirtioBlockDeviceConfiguration(attachment: attachment)]

    // NAT networking so containers can reach the internet.
    let net = VZVirtioNetworkDeviceConfiguration()
    net.attachment = VZNATNetworkDeviceAttachment()
    vz.networkDevices = [net]

    // Entropy, a memory balloon (dynamic memory, OrbStack-style), and vsock
    // (how we reach the guest's forwarded dockerd socket).
    vz.entropyDevices = [VZVirtioEntropyDeviceConfiguration()]
    vz.memoryBalloonDevices = [VZVirtioTraditionalMemoryBalloonDeviceConfiguration()]
    vz.socketDevices = [VZVirtioSocketDeviceConfiguration()]

    // Rosetta lets amd64 images run on Apple Silicon without full emulation.
    var rosettaEnabled = false
    if VZLinuxRosettaDirectoryShare.availability == .installed {
      let rosetta = VZVirtioFileSystemDeviceConfiguration(tag: "rosetta")
      rosetta.share = try VZLinuxRosettaDirectoryShare()
      vz.directorySharingDevices = [rosetta]
      rosettaEnabled = true
    }

    Wire.log(
      "vm config: cpu=\(vz.cpuCount) mem=\(vz.memorySize) rosetta=\(rosettaEnabled) "
        + "kernel=\(config.paths.kernel.lastPathComponent) initrd=\(config.paths.initrd.lastPathComponent)")

    // validate() throws a generic error; wrap it so the log says it was the
    // configuration (vs a later boot failure) and includes any reason VZ gives.
    do {
      try vz.validate()
    } catch {
      throw NSError(
        domain: "hopperd", code: 4,
        userInfo: [
          NSLocalizedDescriptionKey:
            "Virtualization rejected the VM configuration: \(GuestVM.describe(error))"
        ])
    }
    return vz
  }

  // Bring the VM up and start forwarding the Docker socket. `onExit` fires if
  // the guest later stops on its own.
  func start(onExit: @escaping (String) -> Void, completion: @escaping (Bool, String) -> Void) {
    self.onExit = onExit
    queue.async {
      do {
        let vz = try self.buildConfiguration()
        let vm = VZVirtualMachine(configuration: vz, queue: self.queue)
        vm.delegate = self
        self.machine = vm
        vm.start { result in
          switch result {
          case .success:
            self.startBridge()
            completion(true, "running")
          case .failure(let error):
            completion(false, GuestVM.describe(error))
          }
        }
      } catch {
        completion(false, GuestVM.describe(error))
      }
    }
  }

  private func startBridge() {
    guard let device = machine?.socketDevices.first as? VZVirtioSocketDevice else {
      Wire.log("no vsock device on the running VM")
      return
    }
    let bridge = SocketBridge(
      socketPath: config.paths.socket.path,
      guestPort: config.guestPort,
      device: device,
      queue: queue
    )
    self.bridge = bridge
    bridge.start()
  }

  func stop(completion: @escaping (Bool, String) -> Void) {
    queue.async {
      self.bridge?.stop()
      self.bridge = nil
      guard let vm = self.machine, vm.canStop else {
        completion(true, "stopped")
        return
      }
      vm.stop { error in
        self.machine = nil
        if let error = error {
          completion(false, error.localizedDescription)
        } else {
          completion(true, "stopped")
        }
      }
    }
  }

  // One-shot request to the in-guest agent: dial its vsock port, write a
  // command line, read the single JSON reply line. Used for reclaim/stats.
  func agent(_ command: String, completion: @escaping (String?) -> Void) {
    queue.async {
      guard let device = self.machine?.socketDevices.first as? VZVirtioSocketDevice else {
        completion(nil)
        return
      }
      device.connect(toPort: self.config.agentPort) { result in
        switch result {
        case .failure:
          completion(nil)
        case .success(let connection):
          let fd = connection.fileDescriptor
          let request = Array(command.utf8) + [0x0a]
          _ = request.withUnsafeBytes { write(fd, $0.baseAddress, $0.count) }
          // Read the reply off-queue so we don't block the VM queue.
          Thread.detachNewThread {
            var data = Data()
            var buf = [UInt8](repeating: 0, count: 4096)
            while true {
              let n = read(fd, &buf, buf.count)
              if n <= 0 { break }
              data.append(contentsOf: buf[0..<n])
              if buf[0..<n].contains(0x0a) { break }
            }
            self.queue.async { connection.close() }
            let text = String(data: data, encoding: .utf8)?
              .trimmingCharacters(in: .whitespacesAndNewlines)
            completion(text?.isEmpty == false ? text : nil)
          }
        }
      }
    }
  }

  // Read the current run state on the VM's queue (required by the framework).
  func state() -> String {
    queue.sync {
      switch machine?.state {
      case .some(.running): return "running"
      case .some(.starting): return "starting"
      case .some(.paused), .some(.pausing), .some(.resuming): return "paused"
      case .some(.stopping), .some(.stopped): return "stopped"
      case .some(.error): return "error"
      default: return "stopped"
      }
    }
  }

  // Compose a useful one-line detail from an NSError — domain, code, message,
  // failure reason, and any underlying error — so VZ failures like "not
  // entitled to use the Virtualization framework" surface in full.
  static func describe(_ error: Error) -> String {
    let ns = error as NSError
    var parts = ["\(ns.domain) \(ns.code): \(ns.localizedDescription)"]
    if let reason = ns.localizedFailureReason { parts.append(reason) }
    if let underlying = ns.userInfo[NSUnderlyingErrorKey] as? NSError {
      parts.append("underlying \(underlying.domain) \(underlying.code): \(underlying.localizedDescription)")
    }
    return parts.joined(separator: " — ")
  }

  // MARK: VZVirtualMachineDelegate

  func guestDidStop(_ virtualMachine: VZVirtualMachine) {
    bridge?.stop()
    machine = nil
    onExit?("stopped")
  }

  func virtualMachine(_ virtualMachine: VZVirtualMachine, didStopWithError error: Error) {
    Wire.log("guest stopped with error: \(GuestVM.describe(error))")
    bridge?.stop()
    machine = nil
    onExit?("error")
  }
}
