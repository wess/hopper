import re
import subprocess
import sys


def verify(path, msi=False):
  result = subprocess.run(
    ["acpiexec", "-b", "resources _SB.PCI0; resources _SB.RES0", path],
    capture_output=True, text=True, timeout=30, check=True,
  )
  output = result.stdout + result.stderr
  try:
    root, reserved = output.split("Device: \\_SB.PCI0", 1)[1].split("Device: \\_SB.RES0", 1)
    routes = re.findall(
      r"Address\s*:\s*([0-9A-F]+)\s+Pin\s*:\s*([0-9A-F]+)\s+"
      r"Source\s*:\s*\[NULL NAMESTRING\]\s+Source Index\s*:\s*([0-9A-F]+)", root,
    )
    expected = [((device << 16) | 0xffff, pin, 33 + (device + pin) % 4)
      for device in range(32) for pin in range(4)]
    if [tuple(int(value, 16) for value in route) for route in routes] != expected:
      raise ValueError("PCI interrupt routes do not match the native bus")
    fields = [
      (root, "Resource Type", "Bus Number Range"),
      (root, "Address Minimum", "0000"),
      (root, "Address Maximum", "0000"),
      (root, "Address Length", "0001"),
      (root, "Resource Type", "Memory Range"),
      (root, "Address Minimum", "20000000"),
      (root, "Address Maximum", "2FFFFFFF"),
      (root, "Address Length", "10000000"),
      (reserved, "Address", "10000000"),
      (reserved, "Address Length", "00100000"),
    ]
    for section, field, value in fields:
      if not re.search(rf"{field}\s*:\s*{value}\s*$", section, re.MULTILINE):
        raise ValueError(f"Missing decoded resource: {field} = {value}")
    frames = re.findall(r"Address\s*:\s*30000000\s+Address Length\s*:\s*([0-9A-F]+)", reserved)
    if frames != (["00001000"] if msi else []):
      raise ValueError("MSI frame reservation does not match the selected topology")
    if "AcpiGetCurrentResources failed" in output or "AcpiGetIrqRoutingTable failed" in output:
      raise ValueError("ACPICA rejected the PCI resources")
  except (ValueError, IndexError):
    print(output, file=sys.stderr)
    raise
  print("ACPICA verified PCI resources, ECAM/MSI reservations and 128 interrupt routes")


if __name__ == "__main__":
  verify(sys.argv[1], "--msi" in sys.argv[2:])
