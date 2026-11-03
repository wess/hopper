import json
import pathlib
import struct
import sys
import tempfile
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1] / 'build'))
import guestmanifest as manifest


def elf():
  data = bytearray(128)
  data[:7] = b'\x7fELF\x02\x01\x01'
  struct.pack_into('<HHIQQ', data, 16, 2, 183, 1, 0x400078, 64)
  struct.pack_into('<HHH', data, 52, 64, 56, 1)
  struct.pack_into('<IIQQQQQQ', data, 64, 1, 5, 120, 0x400078, 0, 8, 8, 4096)
  return data


class Assets(unittest.TestCase):
  def test_static_arm64_entry_and_segment_bounds(self):
    manifest.executable(elf())
    for offset, form, value in (
      (18, '<H', 62), (54, '<H', 55), (56, '<H', 129),
      (64, '<I', 2), (64, '<I', 3), (68, '<I', 4),
      (72, '<Q', 129), (96, '<Q', 9), (24, '<Q', 0),
    ):
      data = elf()
      struct.pack_into(form, data, offset, value)
      with self.subTest(offset=offset, value=value), self.assertRaises(ValueError):
        manifest.executable(data)
    for data in (b'', elf()[:100], bytearray(1024 * 1024 + 1)):
      with self.assertRaises(ValueError):
        manifest.executable(data)

  def test_exact_inventory_checksum_and_symlink_rejection(self):
    with tempfile.TemporaryDirectory() as directory:
      root = pathlib.Path(directory)
      binary = root / 'hopperguest'
      binary.write_bytes(elf())
      (root / 'hopperguest.service').write_text('[Service]\nExecStart=/usr/libexec/hopperguest\n')
      (root / 'licenses.html').write_text('<html>license notices</html>')
      path = root / 'manifest.json'
      path.write_text(json.dumps(manifest.contents(root)))
      manifest.verify(root)
      notices = root / 'licenses.html'
      notices.write_text('<html>changed notices</html>')
      with self.assertRaisesRegex(ValueError, 'mismatch'):
        manifest.verify(root)
      notices.write_text('<html>license notices</html>')
      data = elf()
      data[-1] = 1
      binary.write_bytes(data)
      with self.assertRaisesRegex(ValueError, 'mismatch'):
        manifest.verify(root)
      binary.write_bytes(elf())
      extra = root / 'unexpected'
      extra.write_bytes(b'x')
      with self.assertRaises(ValueError):
        manifest.verify(root)
      extra.unlink()
      binary.unlink()
      binary.symlink_to(path)
      with self.assertRaises(ValueError):
        manifest.verify(root)

  def test_license_notices_are_required_bounded_regular_text(self):
    with tempfile.TemporaryDirectory() as directory:
      root = pathlib.Path(directory)
      (root / 'hopperguest').write_bytes(elf())
      (root / 'hopperguest.service').write_text('[Service]')
      notices = root / 'licenses.html'
      with self.assertRaises(ValueError):
        manifest.contents(root)
      for data in (b'', b'\0', b'x' * (4 * 1024 * 1024 + 1)):
        notices.write_bytes(data)
        with self.assertRaises(ValueError):
          manifest.contents(root)
      notices.unlink()
      notices.symlink_to(root / 'hopperguest.service')
      with self.assertRaises(ValueError):
        manifest.contents(root)


if __name__ == '__main__':
  unittest.main()
