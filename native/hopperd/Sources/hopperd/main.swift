import Foundation

// hopperd entry point. Reads line-delimited JSON commands on stdin and drives
// the Linux guest, replying on stdout. The main run loop stays alive for the
// Virtualization.framework callbacks; stdin is read on a background thread.

let config = VMConfig.fromEnvironment()
let vm = GuestVM(config: config)

func socketIfRunning(_ state: String) -> String? {
  state == "running" ? config.paths.socket.path : nil
}

func handle(_ command: Command) {
  switch command.cmd {
  case "ping":
    Wire.send(Reply(ok: true, state: vm.state(), detail: "pong", socket: nil))

  case "status":
    let state = vm.state()
    Wire.send(Reply(ok: true, state: state, detail: state, socket: socketIfRunning(state)))

  case "start":
    vm.start(
      onExit: { state in
        Wire.send(Reply(ok: false, state: state, detail: "guest exited", socket: nil))
      },
      completion: { ok, detail in
        Wire.log(ok ? "engine started" : "start failed: \(detail)")
        Wire.send(
          Reply(
            ok: ok,
            state: ok ? "running" : "error",
            detail: detail,
            socket: ok ? config.paths.socket.path : nil
          )
        )
      }
    )

  case "stop":
    vm.stop { ok, detail in
      Wire.send(Reply(ok: ok, state: "stopped", detail: detail, socket: nil))
    }

  case "reclaim":
    vm.agent("reclaim") { json in
      Wire.send(Reply(ok: json != nil, state: vm.state(), detail: "reclaim", socket: nil, data: json))
    }

  case "stats":
    vm.agent("stats") { json in
      Wire.send(Reply(ok: json != nil, state: vm.state(), detail: "stats", socket: nil, data: json))
    }

  default:
    Wire.send(Reply(ok: false, state: vm.state(), detail: "unknown command: \(command.cmd)", socket: nil))
  }
}

let reader = Thread {
  let decoder = JSONDecoder()
  while let line = readLine(strippingNewline: true) {
    if line.isEmpty { continue }
    guard
      let data = line.data(using: .utf8),
      let command = try? decoder.decode(Command.self, from: data)
    else {
      Wire.send(Reply(ok: false, state: "error", detail: "bad command frame", socket: nil))
      continue
    }
    handle(command)
  }
  // stdin closed — the parent (Hopper) is gone; take the VM down and exit.
  vm.stop { _, _ in exit(0) }
}
reader.start()

RunLoop.main.run()
