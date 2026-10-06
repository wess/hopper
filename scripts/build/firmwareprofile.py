"""Derive Hopper's profile without changing pinned upstream files."""

import re


def replace(text, old, new):
  if text.count(old) != 1:
    raise RuntimeError(f"Expected one upstream occurrence of {old}")
  return text.replace(old, new)


def configure(source):
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
