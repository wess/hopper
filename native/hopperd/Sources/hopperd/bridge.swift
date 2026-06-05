import Foundation
import Virtualization

// Bridges a host unix socket to the guest's dockerd, which is forwarded onto a
// vsock port inside the VM. Hopper connects to the host socket exactly as it
// would to /var/run/docker.sock; each accepted connection is spliced to a
// fresh vsock connection into the guest. This is the piece that makes the VM's
// engine look local.
final class SocketBridge {
  private let socketPath: String
  private let guestPort: UInt32
  private let device: VZVirtioSocketDevice
  private let queue: DispatchQueue
  private var listenFD: Int32 = -1
  private var running = false

  init(socketPath: String, guestPort: UInt32, device: VZVirtioSocketDevice, queue: DispatchQueue) {
    self.socketPath = socketPath
    self.guestPort = guestPort
    self.device = device
    self.queue = queue
  }

  func start() {
    unlink(socketPath)
    let fd = socket(AF_UNIX, SOCK_STREAM, 0)
    guard fd >= 0 else {
      Wire.log("socket() failed: \(String(cString: strerror(errno)))")
      return
    }
    var addr = sockaddr_un()
    addr.sun_family = sa_family_t(AF_UNIX)
    let pathBytes = socketPath.utf8CString
    guard pathBytes.count <= MemoryLayout.size(ofValue: addr.sun_path) else {
      Wire.log("socket path too long: \(socketPath)")
      close(fd)
      return
    }
    withUnsafeMutablePointer(to: &addr.sun_path) { raw in
      raw.withMemoryRebound(to: CChar.self, capacity: pathBytes.count) { dst in
        pathBytes.withUnsafeBufferPointer { src in
          dst.update(from: src.baseAddress!, count: pathBytes.count)
        }
      }
    }
    let len = socklen_t(MemoryLayout<sockaddr_un>.size)
    let bound = withUnsafePointer(to: &addr) {
      $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { bind(fd, $0, len) }
    }
    guard bound == 0, listen(fd, 64) == 0 else {
      Wire.log("bind/listen failed: \(String(cString: strerror(errno)))")
      close(fd)
      return
    }
    listenFD = fd
    running = true
    let thread = Thread { [weak self] in self?.acceptLoop() }
    thread.stackSize = 1 << 20
    thread.start()
  }

  func stop() {
    running = false
    if listenFD >= 0 {
      close(listenFD)
      listenFD = -1
    }
    unlink(socketPath)
  }

  private func acceptLoop() {
    while running {
      let client = accept(listenFD, nil, nil)
      if client < 0 {
        if running { continue }
        break
      }
      handleClient(client)
    }
  }

  private func handleClient(_ clientFD: Int32) {
    // VM/socket-device calls must run on the VM's queue.
    queue.async {
      self.device.connect(toPort: self.guestPort) { result in
        switch result {
        case .success(let connection):
          let conn = Splice(clientFD: clientFD, connection: connection, queue: self.queue)
          let guestFD = connection.fileDescriptor
          self.pump(from: clientFD, to: guestFD) { conn.ended(partner: guestFD) }
          self.pump(from: guestFD, to: clientFD) { conn.ended(partner: clientFD) }
        case .failure(let error):
          Wire.log("vsock connect failed: \(error.localizedDescription)")
          close(clientFD)
        }
      }
    }
  }

  // Copy bytes one direction until EOF/error, then signal `end`.
  private func pump(from src: Int32, to dst: Int32, end: @escaping () -> Void) {
    let thread = Thread {
      let cap = 65536
      let buf = UnsafeMutableRawPointer.allocate(byteCount: cap, alignment: 1)
      defer {
        buf.deallocate()
        end()
      }
      while true {
        let n = read(src, buf, cap)
        if n <= 0 { break }
        var off = 0
        while off < n {
          let w = write(dst, buf + off, n - off)
          if w <= 0 { break }
          off += w
        }
        if off < n { break }
      }
    }
    thread.stackSize = 1 << 20
    thread.start()
  }
}

// Tracks the two pump directions of one client<->guest splice and tears the
// connection down exactly once, after both directions have finished.
private final class Splice {
  private let clientFD: Int32
  private let connection: VZVirtioSocketConnection
  private let queue: DispatchQueue
  private let lock = NSLock()
  private var remaining = 2
  private var closed = false

  init(clientFD: Int32, connection: VZVirtioSocketConnection, queue: DispatchQueue) {
    self.clientFD = clientFD
    self.connection = connection
    self.queue = queue
  }

  // One direction ended; propagate EOF to the partner by half-closing only its
  // WRITE side (SHUT_WR, not SHUT_RDWR) so the opposite direction keeps
  // flowing — otherwise an attach/exec client that stops sending (no stdin)
  // would cut off the container's still-arriving stdout. When both directions
  // are done, close both endpoints (the VZ connection owns the guest fd).
  func ended(partner: Int32) {
    shutdown(partner, SHUT_WR)
    lock.lock()
    remaining -= 1
    let done = remaining == 0 && !closed
    if done { closed = true }
    lock.unlock()
    guard done else { return }
    close(clientFD)
    queue.async { self.connection.close() }
  }
}
