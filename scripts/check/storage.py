import pathlib
import struct
import subprocess
import sys


def create(path):
  with path.open("xb") as image:
    image.truncate(32 * 1024 * 1024)
  subprocess.run(
    ["/sbin/newfs_msdos", "-F", "16", "-s", "65536", "-v", "HOPPER", str(path)],
    check=True, timeout=30,
  )
  payload = b"Hopper native storage verified\r\n"
  with path.open("r+b") as image:
    header = image.read(512)
    sector = struct.unpack_from("<H", header, 11)[0]
    cluster = header[13]
    reserved = struct.unpack_from("<H", header, 14)[0]
    copies = header[16]
    entries = struct.unpack_from("<H", header, 17)[0]
    fat = struct.unpack_from("<H", header, 22)[0]
    if sector != 512 or len(payload) > sector * cluster or not copies or not entries or not fat:
      raise ValueError("Unexpected FAT16 fixture geometry")
    root = (reserved + copies * fat) * sector
    data = root + ((entries * 32 + sector - 1) // sector) * sector
    for index in range(copies):
      image.seek((reserved + index * fat) * sector + 4)
      image.write(b"\xff\xff")
    for index in range(entries):
      image.seek(root + index * 32)
      if image.read(1) == b"\0":
        record = bytearray(32)
        record[:11] = b"HOPPER  TXT"
        record[11] = 0x20
        struct.pack_into("<H", record, 26, 2)
        struct.pack_into("<I", record, 28, len(payload))
        image.seek(root + index * 32)
        image.write(record)
        break
    else:
      raise ValueError("FAT16 fixture has no empty directory slot")
    image.seek(data)
    image.write(payload)
  print(path)


if __name__ == "__main__":
  create(pathlib.Path(sys.argv[1]).resolve())
