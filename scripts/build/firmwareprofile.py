"""Derive Hopper's profile without changing pinned upstream files."""

import re


def replace(text, old, new):
  if text.count(old) != 1:
    raise RuntimeError(f"Expected one upstream occurrence of {old}")
  return text.replace(old, new)


def configure(source):
  from firmwaregraphics import configure as graphics

  target = source / "HopperPkg"
  target.mkdir(exist_ok=True)
  profile = (source / "ArmVirtPkg/ArmVirtQemu.dsc").read_text()
  for field, value in {
    "PLATFORM_NAME": "Hopper",
    "PLATFORM_GUID": "c7179281-6978-4488-af8c-499e572f09ca",
    "OUTPUT_DIRECTORY": "Build/Hopper-AArch64",
    "FLASH_DEFINITION": "HopperPkg/firmware.fdf",
  }.items():
    profile, count = re.subn(
      rf"^(\s*{field}\s*=\s*)[^\n]+", rf"\g<1>{value}", profile, flags=re.MULTILINE,
    )
    if count != 1:
      raise RuntimeError(f"Expected one upstream {field} definition")
  for library in ["QemuFwCfgMmioDxeLib", "QemuFwCfgMmioPeiLib"]:
    profile = replace(profile, f"OvmfPkg/Library/QemuFwCfgLib/{library}.inf",
                      "OvmfPkg/Library/QemuFwCfgLib/QemuFwCfgLibNull.inf")
  profile = replace(profile, "!include ArmVirtPkg/ArmVirt.dsc.inc",
                    "!include HopperPkg/common.dsc.inc")
  profile = replace(profile, "[PcdsFixedAtBuild.common]",
                    "[PcdsFixedAtBuild.common]\n"
                    "  gArmVirtTokenSpaceGuid.PcdCloudHvAcpiRsdpBaseAddress|0x40200000")

  common = (source / "ArmVirtPkg/ArmVirt.dsc.inc").read_text()
  common = replace(common, "ArmVirtPkg/Library/PlatformPeiLib/PlatformPeiLib.inf",
                   "HopperPkg/pei/PlatformPeiLib.inf")
  main = (source / "ArmVirtPkg/ArmVirtQemuFvMain.fdf.inc").read_text()
  profile = replace(profile, "  OvmfPkg/VirtioGpuDxe/VirtioGpu.inf",
                    "  OvmfPkg/VirtioGpuDxe/VirtioGpu.inf\n"
                    "  OvmfPkg/VirtioInputDxe/VirtioInput.inf")
  main = replace(main, "  INF OvmfPkg/VirtioGpuDxe/VirtioGpu.inf",
                 "  INF OvmfPkg/VirtioGpuDxe/VirtioGpu.inf\n"
                 "  INF OvmfPkg/VirtioInputDxe/VirtioInput.inf")
  for old, new in {
    "OvmfPkg/PlatformHasAcpiDtDxe/PlatformHasAcpiDtDxe.inf":
      "ArmVirtPkg/CloudHvPlatformHasAcpiDtDxe/CloudHvHasAcpiDtDxe.inf",
    "OvmfPkg/AcpiPlatformDxe/AcpiPlatformDxe.inf":
      "ArmVirtPkg/CloudHvAcpiPlatformDxe/CloudHvAcpiPlatformDxe.inf",
  }.items():
    profile = replace(profile, old, new)
    main = replace(main, old, new)
  fdf = (source / "ArmVirtPkg/ArmVirtQemu.fdf").read_text()
  for old, new in {
    "!include VarStore.fdf.inc": "!include ArmVirtPkg/VarStore.fdf.inc",
    "!include ArmVirtQemuFvMain.fdf.inc": "!include HopperPkg/main.fdf.inc",
    "!include ArmVirtRules.fdf.inc": "!include ArmVirtPkg/ArmVirtRules.fdf.inc",
  }.items():
    fdf = replace(fdf, old, new)
  for name, text in {"firmware.dsc": profile, "common.dsc.inc": common,
                     "firmware.fdf": fdf, "main.fdf.inc": main}.items():
    (target / name).write_text(text)

  boot = target / "boot"
  boot.mkdir(exist_ok=True)
  upstream_boot = source / "OvmfPkg/Library/PlatformBootManagerLibLight"
  inf = (upstream_boot / "PlatformBootManagerLib.inf").read_text()
  inf = replace(inf, "  PlatformBm.c", "  platform.c")
  inf = replace(inf, "  PlatformBm.h", "  platform.h")
  inf = replace(inf, "  QemuKernel.c", "  kernel.c")
  inf = replace(inf, "[Protocols]", "[Protocols]\n  gEfiSimpleTextInProtocolGuid")
  (boot / "boot.inf").write_text(inf)
  (boot / "platform.h").write_bytes((upstream_boot / "PlatformBm.h").read_bytes())
  kernel = (upstream_boot / "QemuKernel.c").read_text()
  kernel = replace(kernel, '#include "PlatformBm.h"', '#include "platform.h"')
  (boot / "kernel.c").write_text(kernel)
  code = (upstream_boot / "PlatformBm.c").read_text()
  code = replace(code, '#include "PlatformBm.h"', '#include "platform.h"')
  code = replace(code, "STATIC\nVOID\nEFIAPI\nSetupVirtioSerial (", """STATIC
BOOLEAN
EFIAPI
IsVirtioPciInput (IN EFI_HANDLE Handle, IN CONST CHAR16 *ReportText)
{
  return IsVirtioPci (Handle, ReportText, 18);
}

STATIC
VOID
EFIAPI
AddInput (IN EFI_HANDLE Handle, IN CONST CHAR16 *ReportText)
{
  EFI_DEVICE_PATH_PROTOCOL *Path;
  EFI_STATUS Status;

  Path = DevicePathFromHandle (Handle);
  if (Path == NULL) {
    return;
  }
  Status = EfiBootManagerUpdateConsoleVariable (ConIn, Path, NULL);
  DEBUG ((DEBUG_VERBOSE, "%a: %s: adding to ConIn: %r\\n", __func__, ReportText, Status));
}

STATIC
VOID
EFIAPI
SetupVirtioSerial (""")
  code = replace(code, "  FilterAndProcess (&gEfiGraphicsOutputProtocolGuid, NULL, AddOutput);",
                 "  FilterAndProcess (&gEfiGraphicsOutputProtocolGuid, NULL, AddOutput);\n"
                 "  FilterAndProcess (&gEfiPciIoProtocolGuid, IsVirtioPciInput, Connect);\n"
                 "  FilterAndProcess (&gEfiSimpleTextInProtocolGuid, NULL, AddInput);")
  (boot / "platform.c").write_text(code)
  profile = replace(profile,
                    "OvmfPkg/Library/PlatformBootManagerLibLight/PlatformBootManagerLib.inf",
                    "HopperPkg/boot/boot.inf")
  (target / "firmware.dsc").write_text(profile)

  pei = target / "pei"
  pei.mkdir(exist_ok=True)
  upstream = source / "ArmVirtPkg/Library/PlatformPeiLib"
  (pei / "PlatformPeiLib.inf").write_bytes((upstream / "PlatformPeiLib.inf").read_bytes())
  code = (upstream / "PlatformPeiLib.c").read_text()
  code = replace(code, "  FdtSize  = FdtTotalSize (Base)",
                 "  // reserve the host's handoff until DXE copies its ACPI tables.\n"
                 "  BuildMemoryAllocationHob (0x40200000, 0x10000, EfiReservedMemoryType);\n\n"
                 "  FdtSize  = FdtTotalSize (Base)")
  (pei / "PlatformPeiLib.c").write_text(code)
  graphics(source)
  for name in ["firmware.dsc", "main.fdf.inc"]:
    file = target / name
    text = replace(file.read_text(), "OvmfPkg/VirtioGpuDxe/VirtioGpu.inf",
                    "HopperPkg/gpu/gpu.inf")
    file.write_text(text)
