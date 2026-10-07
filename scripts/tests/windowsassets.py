import pathlib
import sys
import tempfile
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1] / 'build'))
import windows
import windowsmanifest as manifest


class Assets(unittest.TestCase):
  def test_arm64_link_groups_exclude_other_architectures_and_symbols(self):
    names = ['./usr/share/doc/virtio-win/virtio-win_license.txt']
    for group, files in manifest.DRIVERS.items():
      for name in files:
        for system in ('w11', '2k25'):
          names.append(f'./usr/share/virtio-win/drivers/by-driver/{group}/{system}/ARM64/{name}')
          names.append(f'./usr/share/virtio-win/drivers/by-os/ARM64/{system}/{name}')
        names.append(f'./usr/share/virtio-win/drivers/by-driver/{group}/w11/amd64/{name}')
    names.append('./usr/share/virtio-win/drivers/by-driver/vioscsi/w11/ARM64/vioscsi.pdb')
    selected = windows.members('\n'.join(names))
    self.assertEqual(len(selected), 53)
    self.assertTrue(all('amd64' not in name and not name.endswith('.pdb') for name in selected))
    with self.assertRaises(ValueError):
      windows.members('\n'.join(names + [names[1]]))

  def test_wrong_package_checksum_is_rejected_before_extraction(self):
    with tempfile.TemporaryDirectory() as temporary:
      root = pathlib.Path(temporary)
      package = root / 'drivers.rpm'
      package.write_bytes(b'wrong source')
      with self.assertRaisesRegex(ValueError, 'checksum'):
        windows.drivers(package, root / 'output')
      self.assertFalse((root / 'output').exists())

  def test_incomplete_manifest_and_symlinks_fail_before_publication(self):
    with tempfile.TemporaryDirectory() as temporary:
      root = pathlib.Path(temporary)
      with self.assertRaises(ValueError):
        manifest.write(root)
      self.assertFalse((root / 'manifest.json').exists())
      (root / 'alias').symlink_to('/usr/bin/true')
      with self.assertRaises(ValueError):
        manifest.write(root)


if __name__ == '__main__':
  unittest.main()
