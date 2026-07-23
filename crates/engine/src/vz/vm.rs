//! Building the guest VM configuration on Apple's Virtualization framework.
//!
//! This replaces the Swift `hopperd`. Everything the sidecar did — boot a
//! minimal Linux guest, attach the persistent `/var/lib/docker` disk, bridge
//! the Docker socket over vsock — happens here in-process.
//!
//! The Objective-C calls are all `unsafe` by construction, so the parts that
//! can be decided without touching the framework (paths, sizing, the kernel
//! command line, the share set) are separated out and tested on their own.

use crate::vz::shares::Share;
use model::EngineResources;
use std::path::{Path, PathBuf};

/// Guest vsock ports, matching what the guest init script listens on.
pub const DOCKER_VSOCK_PORT: u32 = 2375;
pub const AGENT_VSOCK_PORT: u32 = 2377;

/// Where the guest assets and VM state live.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    pub kernel: PathBuf,
    pub initrd: PathBuf,
    pub disk: PathBuf,
    /// Where the guest's console output lands. `console=hvc0` on the kernel
    /// command line is useless without a device attached to receive it — a
    /// failed boot otherwise leaves nothing to read.
    pub console: PathBuf,
}

impl Layout {
    /// Resolve from the app bundle. The kernel and initrd are *data*, so they
    /// live in `Contents/Resources/`; codesign rejects unsigned executables
    /// under `Contents/MacOS/`.
    pub fn resolve(resources: &Path, state: &Path) -> Self {
        Self {
            kernel: resources.join("vmlinuz"),
            initrd: resources.join("initrd"),
            disk: state.join("docker.img"),
            console: state.join("console.log"),
        }
    }

    /// Build from an explicit kernel/initrd pair — bundled or downloaded into
    /// the cache — deriving the disk and console from the state directory.
    pub fn with_guest(kernel: PathBuf, initrd: PathBuf, state: &Path) -> Self {
        Self {
            kernel,
            initrd,
            disk: state.join("docker.img"),
            console: state.join("console.log"),
        }
    }

    /// What is missing, so startup can explain rather than fail opaquely.
    pub fn missing(&self, exists: impl Fn(&Path) -> bool) -> Vec<String> {
        let mut out = Vec::new();
        if !exists(&self.kernel) {
            out.push(format!("kernel image ({})", self.kernel.display()));
        }
        if !exists(&self.initrd) {
            out.push(format!("initial ramdisk ({})", self.initrd.display()));
        }
        // The data disk is created on first boot, so its absence is normal.
        out
    }
}

/// The kernel command line.
///
/// `console=hvc0` sends kernel output to the virtio console so a failed boot
/// leaves a readable log instead of a silent hang.
pub fn kernel_command_line(extra: &[String]) -> String {
    let mut parts = vec![
        "console=hvc0".to_string(),
        "root=/dev/ram0".to_string(),
        "rw".to_string(),
        "quiet".to_string(),
    ];
    parts.extend(extra.iter().filter(|e| !e.trim().is_empty()).cloned());
    parts.join(" ")
}

/// Clamp requested resources to what this machine can actually give.
///
/// The framework refuses a configuration outside its supported range, so
/// clamping here turns a settings typo into a working VM rather than an
/// engine that will not boot.
pub fn clamp(resources: EngineResources, host_cpus: u32, host_memory_bytes: u64) -> EngineResources {
    // Leave the host at least one core and 2 GiB, or the machine becomes
    // unusable while the VM runs.
    let max_cpus = host_cpus.saturating_sub(1).max(1);
    let reserved = 2 * 1024 * 1024 * 1024u64;
    let max_gib = host_memory_bytes.saturating_sub(reserved) / (1024 * 1024 * 1024);

    EngineResources {
        cpus: resources.cpus.clamp(1, max_cpus),
        memory_gib: resources.memory_gib.clamp(1, max_gib.max(1) as u32),
        // A disk smaller than a few GiB cannot hold a useful image set.
        disk_gib: resources.disk_gib.max(8),
    }
}

/// Bytes for the framework's `setMemorySize`.
pub fn memory_bytes(resources: &EngineResources) -> u64 {
    (resources.memory_gib as u64) * 1024 * 1024 * 1024
}

/// Create the sparse data disk if it is not there yet.
///
/// Sparse means the file reports its full size but only occupies what is
/// written, so a 60 GiB disk costs nothing until images land in it. The disk is
/// created once and never recreated: doing otherwise would silently discard
/// every image and volume the user has.
pub fn ensure_disk(path: &Path, size_gib: u32) -> std::io::Result<bool> {
    if path.exists() {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = std::fs::File::create(path)?;
    file.set_len((size_gib as u64) * 1024 * 1024 * 1024)?;
    Ok(true)
}

/// The kernel command-line arguments naming each share, so the guest knows
/// which tag to mount where. Without these the guest cannot mount anything
/// beyond what it hardcodes.
pub fn share_args(shares: &[Share]) -> Vec<String> {
    shares
        .iter()
        .map(|s| format!("hopper.share={}:{}", s.tag(), s.path.display()))
        .collect()
}

/// A description of the configuration, for logs and diagnostics.
pub fn describe(resources: &EngineResources, shares: &[Share]) -> String {
    let paths: Vec<String> = shares
        .iter()
        .map(|s| s.path.display().to_string())
        .collect();
    format!(
        "{} CPU, {} GiB memory, {} GiB disk, sharing: {}",
        resources.cpus,
        resources.memory_gib,
        resources.disk_gib,
        if paths.is_empty() {
            "nothing".to_string()
        } else {
            paths.join(", ")
        }
    )
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use objc2::rc::Retained;
    use objc2::AnyThread;
    use objc2_foundation::{NSArray, NSFileHandle, NSString, NSURL};
    use objc2_virtualization::*;

    fn url(path: &Path) -> Retained<NSURL> {
        NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()))
    }

    /// Whether this machine can run a VM at all.
    pub fn supported() -> bool {
        unsafe { VZVirtualMachine::isSupported() }
    }

    /// Whether this process actually holds `com.apple.security.virtualization`.
    ///
    /// `supported()` reports only hardware capability; the VM still cannot start
    /// without the entitlement, which lives on the signed `.app` and never on a
    /// plain `cargo run` dev binary. Checking it keeps the managed engine from
    /// advertising itself — and autostart from noisily failing — where it can
    /// never boot. Queried via `codesign` (the same check the release gate
    /// runs) once and cached for the process's life.
    pub fn entitled() -> bool {
        use std::sync::OnceLock;
        static ENTITLED: OnceLock<bool> = OnceLock::new();
        *ENTITLED.get_or_init(|| {
            let Ok(exe) = std::env::current_exe() else {
                return false;
            };
            std::process::Command::new("/usr/bin/codesign")
                .args(["-d", "--entitlements", "-", "--xml"])
                .arg(&exe)
                .output()
                .map(|out| {
                    String::from_utf8_lossy(&out.stdout)
                        .contains("com.apple.security.virtualization")
                })
                .unwrap_or(false)
        })
    }

    /// Assemble the full machine configuration.
    ///
    /// # Safety
    /// Calls into Virtualization.framework, which requires the
    /// `com.apple.security.virtualization` entitlement.
    pub unsafe fn build_configuration(
        layout: &Layout,
        resources: &EngineResources,
        shares: &[Share],
        extra_cmdline: &[String],
    ) -> Retained<VZVirtualMachineConfiguration> {
        let config = VZVirtualMachineConfiguration::new();

        let boot = VZLinuxBootLoader::initWithKernelURL(
            VZLinuxBootLoader::alloc(),
            &url(&layout.kernel),
        );
        boot.setInitialRamdiskURL(Some(&url(&layout.initrd)));
        boot.setCommandLine(&NSString::from_str(&kernel_command_line(extra_cmdline)));
        config.setBootLoader(Some(&boot));

        config.setCPUCount(resources.cpus as usize);
        config.setMemorySize(memory_bytes(resources));
        config.setPlatform(&VZGenericPlatformConfiguration::new());

        // The persistent /var/lib/docker disk.
        if let Ok(attachment) = VZDiskImageStorageDeviceAttachment::initWithURL_readOnly_error(
            VZDiskImageStorageDeviceAttachment::alloc(),
            &url(&layout.disk),
            false,
        ) {
            let block = VZVirtioBlockDeviceConfiguration::initWithAttachment(
                VZVirtioBlockDeviceConfiguration::alloc(),
                &attachment,
            );
            let storage: Retained<VZStorageDeviceConfiguration> = Retained::into_super(block);
            config.setStorageDevices(&NSArray::from_slice(&[storage.as_ref()]));
        }

        // A serial console fed by a file handle. Without this, everything the
        // kernel and init print is discarded and a failed boot is a silent
        // hang with nothing to diagnose from.
        if let Some(parent) = layout.console.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(file) = std::fs::File::create(&layout.console) {
            use std::os::fd::IntoRawFd;
            let fd = file.into_raw_fd();
            let handle = NSFileHandle::initWithFileDescriptor_closeOnDealloc(
                NSFileHandle::alloc(),
                fd,
                true,
            );
            let attachment = VZFileHandleSerialPortAttachment::initWithFileHandleForReading_fileHandleForWriting(
                VZFileHandleSerialPortAttachment::alloc(),
                None,
                Some(&handle),
            );
            let console = VZVirtioConsoleDeviceSerialPortConfiguration::new();
            console.setAttachment(Some(&Retained::into_super(attachment)));
            let port: Retained<VZSerialPortConfiguration> = Retained::into_super(console);
            config.setSerialPorts(&NSArray::from_slice(&[port.as_ref()]));
        }

        // vsock carries the Docker socket, the guest agent, and port forwards.
        let socket = VZVirtioSocketDeviceConfiguration::new();
        let socket_dev: Retained<VZSocketDeviceConfiguration> = Retained::into_super(socket);
        config.setSocketDevices(&NSArray::from_slice(&[socket_dev.as_ref()]));

        // NAT gives the guest outbound access for image pulls.
        let nat = VZNATNetworkDeviceAttachment::new();
        let net = VZVirtioNetworkDeviceConfiguration::new();
        net.setAttachment(Some(&nat));
        let net_dev: Retained<VZNetworkDeviceConfiguration> = Retained::into_super(net);
        config.setNetworkDevices(&NSArray::from_slice(&[net_dev.as_ref()]));

        let entropy = VZVirtioEntropyDeviceConfiguration::new();
        let entropy_dev: Retained<VZEntropyDeviceConfiguration> = Retained::into_super(entropy);
        config.setEntropyDevices(&NSArray::from_slice(&[entropy_dev.as_ref()]));

        // Ballooning returns idle guest memory to the host.
        let balloon = VZVirtioTraditionalMemoryBalloonDeviceConfiguration::new();
        let balloon_dev: Retained<VZMemoryBalloonDeviceConfiguration> =
            Retained::into_super(balloon);
        config.setMemoryBalloonDevices(&NSArray::from_slice(&[balloon_dev.as_ref()]));

        // Every configured host directory, each as its own virtiofs device.
        // The Swift build attached exactly one (the user's home), which is why
        // a bind mount outside it silently resolved to an empty directory.
        let mut sharing: Vec<Retained<VZDirectorySharingDeviceConfiguration>> = Vec::new();
        for share in shares {
            let dir = VZSharedDirectory::initWithURL_readOnly(
                VZSharedDirectory::alloc(),
                &url(&share.path),
                share.read_only,
            );
            let single = VZSingleDirectoryShare::initWithDirectory(
                VZSingleDirectoryShare::alloc(),
                &dir,
            );
            let device = VZVirtioFileSystemDeviceConfiguration::initWithTag(
                VZVirtioFileSystemDeviceConfiguration::alloc(),
                &NSString::from_str(&share.tag()),
            );
            device.setShare(Some(&Retained::into_super(single)));
            sharing.push(Retained::into_super(device));
        }
        let refs: Vec<&VZDirectorySharingDeviceConfiguration> =
            sharing.iter().map(|s| s.as_ref()).collect();
        config.setDirectorySharingDevices(&NSArray::from_slice(&refs));

        config
    }
}

#[cfg(target_os = "macos")]
pub use platform::{build_configuration, entitled, supported};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_layout_puts_guest_assets_in_resources_not_macos() {
        // codesign rejects unsigned "code" under Contents/MacOS, so the kernel
        // and initrd have to be data in Resources.
        let layout = Layout::resolve(Path::new("/A.app/Contents/Resources"), Path::new("/state"));
        assert!(layout.kernel.starts_with("/A.app/Contents/Resources"));
        assert!(layout.initrd.starts_with("/A.app/Contents/Resources"));
        assert!(layout.disk.starts_with("/state"));
    }

    #[test]
    fn missing_boot_assets_are_named_individually() {
        let layout = Layout::resolve(Path::new("/res"), Path::new("/state"));
        let missing = layout.missing(|_| false);
        assert_eq!(missing.len(), 2);
        assert!(missing[0].contains("kernel"));
        assert!(missing[1].contains("ramdisk"));
    }

    #[test]
    fn an_absent_data_disk_is_not_an_error_because_it_is_created_on_boot() {
        let layout = Layout::resolve(Path::new("/res"), Path::new("/state"));
        let missing = layout.missing(|p| p != layout.disk);
        assert!(missing.is_empty());
    }

    #[test]
    fn the_kernel_command_line_keeps_console_output_for_diagnosing_boots() {
        let cmdline = kernel_command_line(&[]);
        assert!(cmdline.contains("console=hvc0"));
    }

    #[test]
    fn extra_command_line_arguments_are_appended_and_blanks_dropped() {
        let cmdline = kernel_command_line(&["hopper.share=abc".into(), "  ".into()]);
        assert!(cmdline.ends_with("hopper.share=abc"));
        assert!(!cmdline.contains("  "));
    }

    #[test]
    fn resources_are_clamped_to_leave_the_host_usable() {
        let asked = EngineResources {
            cpus: 64,
            memory_gib: 512,
            disk_gib: 60,
        };
        // An 8-core, 16 GiB Mac.
        let got = clamp(asked, 8, 16 * 1024 * 1024 * 1024);
        assert_eq!(got.cpus, 7, "the host keeps a core");
        assert_eq!(got.memory_gib, 14, "the host keeps 2 GiB");
    }

    #[test]
    fn a_zero_request_is_raised_to_something_bootable() {
        let asked = EngineResources {
            cpus: 0,
            memory_gib: 0,
            disk_gib: 0,
        };
        let got = clamp(asked, 8, 16 * 1024 * 1024 * 1024);
        assert!(got.cpus >= 1);
        assert!(got.memory_gib >= 1);
        assert!(got.disk_gib >= 8, "a tiny disk cannot hold an image set");
    }

    #[test]
    fn a_single_core_host_still_yields_a_bootable_configuration() {
        let got = clamp(EngineResources::default(), 1, 4 * 1024 * 1024 * 1024);
        assert_eq!(got.cpus, 1);
    }

    #[test]
    fn a_tiny_memory_host_does_not_clamp_to_zero() {
        // saturating_sub would otherwise produce a 0 GiB VM, which cannot boot.
        let got = clamp(EngineResources::default(), 4, 1024 * 1024 * 1024);
        assert!(got.memory_gib >= 1);
    }

    #[test]
    fn memory_is_converted_to_bytes() {
        let r = EngineResources {
            cpus: 1,
            memory_gib: 4,
            disk_gib: 8,
        };
        assert_eq!(memory_bytes(&r), 4 * 1024 * 1024 * 1024);
    }

    #[test]
    fn the_data_disk_is_created_once_and_never_recreated() {
        let dir = std::env::temp_dir().join(format!("hoppervm{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let disk = dir.join("docker.img");

        assert!(ensure_disk(&disk, 8).unwrap(), "first call creates it");
        // Write a marker to stand in for a user's images and volumes.
        std::fs::write(dir.join("marker"), b"data").unwrap();

        assert!(
            !ensure_disk(&disk, 8).unwrap(),
            "recreating would silently discard every image and volume"
        );
        assert!(dir.join("marker").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_disk_is_sparse_so_a_large_size_costs_nothing_up_front() {
        let dir = std::env::temp_dir().join(format!("hoppersparse{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let disk = dir.join("docker.img");
        ensure_disk(&disk, 64).unwrap();

        let meta = std::fs::metadata(&disk).unwrap();
        assert_eq!(meta.len(), 64 * 1024 * 1024 * 1024);
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            // Blocks actually allocated, not the reported length.
            assert!(
                meta.blocks() * 512 < 1024 * 1024,
                "a fresh 64 GiB disk should occupy almost nothing"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_description_lists_the_shared_directories() {
        let shares = vec![Share {
            path: PathBuf::from("/opt/data"),
            read_only: false,
        }];
        let text = describe(&EngineResources::default(), &shares);
        assert!(text.contains("/opt/data"));
        assert!(text.contains("CPU"));
    }

    #[test]
    fn the_description_says_so_when_nothing_is_shared() {
        assert!(describe(&EngineResources::default(), &[]).contains("nothing"));
    }
}
