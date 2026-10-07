"""Filesystem checks for private Windows deployment media inputs."""

import importlib.util
from pathlib import Path
import tempfile
import unittest


source = Path(__file__).resolve().parents[1] / "build/windowsprobe.py"
spec = importlib.util.spec_from_file_location("builder", source)
builder = importlib.util.module_from_spec(spec)
spec.loader.exec_module(builder)


class Deployment(unittest.TestCase):
  def bundle(self, root):
    root.chmod(0o700)
    for name in builder.DEPLOYMENT:
      path = root / name
      path.write_bytes(b"synthetic unused deployment data")
      path.chmod(0o600)

  def test_private_complete_bundle_and_missing_file(self):
    with tempfile.TemporaryDirectory() as folder:
      root = Path(folder)
      self.bundle(root)
      self.assertEqual(set(builder.deployment_files(root)), set(builder.DEPLOYMENT))
      (root / "deploy.cmd").unlink()
      with self.assertRaises(FileNotFoundError):
        builder.deployment_files(root)

  def test_public_files_symlinks_and_oversized_payload_are_rejected(self):
    with tempfile.TemporaryDirectory() as folder:
      root = Path(folder)
      self.bundle(root)
      path = root / "unattend.xml"
      path.chmod(0o644)
      with self.assertRaises(ValueError):
        builder.deployment_files(root)
      path.unlink()
      path.symlink_to(root / "deploy.cmd")
      with self.assertRaises(ValueError):
        builder.deployment_files(root)
      path.unlink()
      path.write_bytes(b"x" * (256 * 1024 + 1))
      path.chmod(0o600)
      with self.assertRaises(ValueError):
        builder.deployment_files(root)

  def test_public_directory_and_directory_symlink_are_rejected(self):
    with tempfile.TemporaryDirectory() as folder:
      root = Path(folder)
      self.bundle(root)
      root.chmod(0o755)
      with self.assertRaises(ValueError):
        builder.deployment_files(root)
      root.chmod(0o700)
      alias = root / "alias"
      alias.symlink_to(root, target_is_directory=True)
      with self.assertRaises(ValueError):
        builder.deployment_files(alias)

  def test_invalid_bundle_never_creates_media_output(self):
    with tempfile.TemporaryDirectory() as folder:
      root = Path(folder)
      self.bundle(root)
      (root / "deploy.cmd").unlink()
      output = root / "output"
      with self.assertRaises(FileNotFoundError):
        builder.build(root / "installer", root / "drivers", output, "unused", root)
      self.assertFalse(output.exists())


if __name__ == "__main__":
  unittest.main()
