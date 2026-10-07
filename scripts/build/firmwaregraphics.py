"""Provide a reserved, directly writable framebuffer on Hopper's GPU."""

from firmwareprofile import replace


def configure(source):
  target = source / "HopperPkg/gpu"
  target.mkdir(exist_ok=True)
  upstream = source / "OvmfPkg/VirtioGpuDxe"
  inf = (upstream / "VirtioGpu.inf").read_text()
  for old, new in {"Commands.c": "commands.c", "DriverBinding.c": "driver.c",
                   "Gop.c": "gop.c", "VirtioGpu.h": "gpu.h"}.items():
    inf = replace(inf, "  " + old, "  " + new)
  (target / "gpu.inf").write_text(inf)

  header = (upstream / "VirtioGpu.h").read_text()
  header = replace(header, "#include <Protocol/GraphicsOutput.h>",
                   "#include <Protocol/GraphicsOutput.h>\n#include <Protocol/PciIo.h>")
  header = replace(header, "#include <Library/UefiLib.h>",
                   "#include <Library/UefiLib.h>\n#include <Library/UefiBootServicesTableLib.h>")
  header = replace(header, "  VIRTIO_DEVICE_PROTOCOL      *VirtIo;",
                   "  VIRTIO_DEVICE_PROTOCOL      *VirtIo;\n"
                   "  EFI_PCI_IO_PROTOCOL         *PciIo;")
  (target / "gpu.h").write_text(header)

  driver = (upstream / "DriverBinding.c").read_text()
  driver = replace(driver, '#include "VirtioGpu.h"', '#include "gpu.h"')
  driver = replace(driver, "    VgpuDev->VirtIo = VirtIo;", """    VgpuDev->VirtIo = VirtIo;
    Status = gBS->HandleProtocol (ControllerHandle, &gEfiPciIoProtocolGuid,
                                  (VOID **)&VgpuDev->PciIo);
    if (EFI_ERROR (Status)) {
      goto FreeVgpuDev;
    }
    {
      UINT32 Signature[2];
      Status = VgpuDev->PciIo->Mem.Read (VgpuDev->PciIo, EfiPciIoWidthUint32,
                                        0, 0x3800, 2, Signature);
      if (EFI_ERROR (Status) || Signature[0] != 0x48504642 || Signature[1] != 1) {
        Status = EFI_UNSUPPORTED;
        goto FreeVgpuDev;
      }
    }""")
  (target / "driver.c").write_text(driver)

  commands = (upstream / "Commands.c").read_text()
  commands = replace(commands, '#include "VirtioGpu.h"', '#include "gpu.h"')
  start = commands.index("  EFI_STATUS  Status;", commands.index("VirtioGpuAllocateZeroAndMapBackingStore ("))
  end = commands.index("\n}\n", start)
  commands = commands[:start] + """  EFI_STATUS Status;
  EFI_PHYSICAL_ADDRESS Address;

  // windows retains this memory after ExitBootServices, without a Virtio driver.
  if (NumberOfPages == 0 || NumberOfPages > 32768) {
    return EFI_OUT_OF_RESOURCES;
  }
  Address = 0;
  Status = gBS->AllocatePages (AllocateAnyPages, EfiReservedMemoryType,
                               NumberOfPages, &Address);
  if (EFI_ERROR (Status)) {
    return Status;
  }
  *HostAddress = (VOID *)(UINTN)Address;
  *DeviceAddress = Address;
  *Mapping = NULL;
  ZeroMem (*HostAddress, EFI_PAGES_TO_SIZE (NumberOfPages));
  return EFI_SUCCESS;""" + commands[end:]
  start = commands.index("  VgpuDev->VirtIo->UnmapSharedBuffer", commands.index("VirtioGpuUnmapAndFreeBackingStore ("))
  end = commands.index("\n}\n", start)
  commands = commands[:start] + """  EFI_STATUS Status;
  Status = gBS->FreePages ((EFI_PHYSICAL_ADDRESS)(UINTN)HostAddress, NumberOfPages);
  ASSERT_EFI_ERROR (Status);""" + commands[end:]
  (target / "commands.c").write_text(commands)

  gop = (upstream / "Gop.c").read_text()
  gop = replace(gop, '#include "VirtioGpu.h"', '#include "gpu.h"')
  gop = replace(gop, "  // No direct framebuffer access is supported, only Blt() is.",
                "  // reserved backing remains directly writable after the firmware exits.")
  gop = gop.replace("PixelBltOnly", "PixelBlueGreenRedReserved8BitPerColor")
  gop = replace(gop, "  VgpuGop->BackingStoreMap = NewBackingStoreMap;", """  VgpuGop->BackingStoreMap = NewBackingStoreMap;
  VgpuGop->GopMode.FrameBufferBase = (EFI_PHYSICAL_ADDRESS)(UINTN)NewBackingStore;
  VgpuGop->GopMode.FrameBufferSize = GopModeInfo->HorizontalResolution *
                                    GopModeInfo->VerticalResolution * sizeof (UINT32);
  {
    UINT32 Layout[6];
    Layout[0] = (UINT32)VgpuGop->GopMode.FrameBufferBase;
    Layout[1] = (UINT32)(VgpuGop->GopMode.FrameBufferBase >> 32);
    Layout[2] = GopModeInfo->HorizontalResolution;
    Layout[3] = GopModeInfo->VerticalResolution;
    Layout[4] = GopModeInfo->PixelsPerScanLine;
    Layout[5] = 1;
    Status = VgpuGop->ParentBus->PciIo->Mem.Write (VgpuGop->ParentBus->PciIo,
               EfiPciIoWidthUint32, 0, 0x3808, 6, Layout);
    ASSERT_EFI_ERROR (Status);
    DEBUG ((DEBUG_INFO, "Hopper GOP linear framebuffer %ux%u reserved\\n", Layout[2], Layout[3]));
  }""")
  gop = replace(gop, "  ASSERT (VgpuGop->BackingStore != NULL);", """  ASSERT (VgpuGop->BackingStore != NULL);
  if (DisableHead) {
    UINT32 Disable = 0;
    VgpuGop->ParentBus->PciIo->Mem.Write (VgpuGop->ParentBus->PciIo,
      EfiPciIoWidthUint32, 0, 0x381C, 1, &Disable);
  }
  VgpuGop->GopMode.FrameBufferBase = 0;
  VgpuGop->GopMode.FrameBufferSize = 0;""")
  (target / "gop.c").write_text(gop)
