import Foundation

// The control protocol between Hopper (TS host) and hopperd. One JSON object
// per line on stdin (commands) and stdout (replies/events). Keeping it line
// framed means the TS side can drive it with a trivial reader.

struct Command: Decodable {
  let cmd: String  // "start" | "stop" | "status" | "ping"
}

// Sent in reply to a command, and emitted unsolicited when the VM changes
// state (e.g. the guest shuts down on its own).
struct Reply: Encodable {
  let ok: Bool
  let state: String  // "running" | "stopped" | "starting" | "error"
  let detail: String
  // Where the host can reach the forwarded Docker socket, when running.
  let socket: String?
  // Raw JSON from the guest agent (reclaim/stats). Omitted when nil.
  var data: String? = nil
}

enum Wire {
  // Emit one reply line to stdout, flushed.
  static func send(_ reply: Reply) {
    guard let data = try? JSONEncoder().encode(reply) else { return }
    FileHandle.standardOutput.write(data)
    FileHandle.standardOutput.write(Data([0x0a]))  // newline
  }

  // The persistent log a GUI user can read after a failed start (stderr is
  // discarded for a windowed launch).
  static func logPath() -> String {
    let env = ProcessInfo.processInfo.environment
    let home =
      env["HOPPER_HOME"]
      ?? FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".hopper").path
    try? FileManager.default.createDirectory(
      atPath: home, withIntermediateDirectories: true)
    return home + "/hopperd.log"
  }

  // Log diagnostics to stderr AND a persistent file (stdout stays clean for
  // the JSON protocol).
  static func log(_ message: String) {
    let line = Data("[hopperd] \(message)\n".utf8)
    FileHandle.standardError.write(line)
    let path = logPath()
    if let handle = FileHandle(forWritingAtPath: path) {
      handle.seekToEndOfFile()
      handle.write(line)
      try? handle.close()
    } else {
      try? line.write(to: URL(fileURLWithPath: path))
    }
  }
}
