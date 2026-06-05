// swift-tools-version:6.0
import PackageDescription

// hopperd — Hopper's native macOS engine helper. Boots a Linux guest on
// Apple's Virtualization.framework, runs dockerd inside, and bridges the
// guest's Docker socket to the host so Hopper can talk to it. Driven over a
// line-delimited JSON control protocol on stdio by the TS `vz` provider.
let package = Package(
  name: "hopperd",
  platforms: [.macOS(.v14)],
  targets: [
    .executableTarget(
      name: "hopperd",
      path: "Sources/hopperd",
      // Systems code with manual threading; v5 mode keeps strict-concurrency
      // out of the POSIX socket plumbing.
      swiftSettings: [.swiftLanguageMode(.v5)],
      linkerSettings: [.linkedFramework("Virtualization")]
    )
  ]
)
