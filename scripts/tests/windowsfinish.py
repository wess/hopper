import hashlib
import pathlib
import sys
import tempfile
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1] / "build"))
import windowsfinish


class Helper(unittest.TestCase):
  def test_real_arm64_build_is_reproducible_and_ships_the_project_notice(self):
    project = pathlib.Path(__file__).resolve().parents[2]
    with tempfile.TemporaryDirectory() as temporary:
      root = pathlib.Path(temporary)
      digests = []
      for name in ("first", "second"):
        output = root / name
        windowsfinish.build(output)
        binary = output / "guest/hopperfinish.exe"
        data = binary.read_bytes()
        offset = int.from_bytes(data[60:64], "little")
        self.assertEqual(data[offset:offset + 6], b"PE\0\0\x64\xaa")
        self.assertLessEqual(len(data), 1 << 20)
        self.assertEqual(binary.stat().st_mode & 0o777, 0o644)
        self.assertEqual((output / "licenses/hopper.txt").read_bytes(), (project / "LICENSE").read_bytes())
        digests.append(hashlib.sha256(data).digest())
      self.assertEqual(digests[0], digests[1])


if __name__ == "__main__":
  unittest.main()
