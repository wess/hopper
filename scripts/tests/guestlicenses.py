import json
import pathlib
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1] / 'build'))
import guestlicenses as licenses


class Notices(unittest.TestCase):
  def test_target_inventory_preserves_notices_and_excludes_unselected_dependencies(self):
    with tempfile.TemporaryDirectory() as directory:
      root = pathlib.Path(directory)
      crate = root / 'dependency'
      crate.mkdir()
      (crate / 'LICENSE-MIT').write_text('copyright <holder> & terms')
      (crate / 'LICENSE-UNICODE').write_text('Unicode terms')
      docs = root / 'share/doc/rust'
      docs.mkdir(parents=True)
      (docs / 'COPYRIGHT-library.html').write_text('<html><body>standard terms</body></html>')
      package = {'name': 'dependency', 'version': '1.2.3', 'source': 'registry+source',
                 'manifest_path': str(crate / 'Cargo.toml'), 'license': 'MIT AND Unicode-3.0'}
      metadata = {'packages': [package, dict(package, name='unused')]}
      def cargo(*args):
        self.assertIn('--locked', args)
        self.assertIn(licenses.TARGET, args)
        return json.dumps(metadata) if args[0] == 'metadata' else 'dependency v1.2.3\n'
      def rustc(args, **kwargs):
        return str(root) if '--print' in args else 'rustc 1.99.0 (test)'
      with patch.object(licenses, 'cargo', cargo), patch.object(
          licenses.subprocess, 'check_output', rustc):
        data = licenses.notices().decode()
        self.assertIn('copyright &lt;holder&gt; &amp; terms', data)
        self.assertIn('Unicode terms', data)
        self.assertIn('standard terms', data)
        self.assertIn('musl 1.2.5', data)
        self.assertIn(licenses.html.escape(licenses.MUSL_NOTICE.read_text()), data)
        self.assertNotIn('unused', data)
        with patch.object(licenses.subprocess, 'check_output', side_effect=lambda args, **kwargs:
            str(root) if '--print' in args else 'rustc 1.100.0 (unreviewed)'):
          with self.assertRaisesRegex(ValueError, 'Review musl linkage'):
            licenses.notices()
        (crate / 'LICENSE-UNICODE').unlink()
        with self.assertRaisesRegex(ValueError, 'Missing required Unicode license'):
          licenses.notices()
        (crate / 'LICENSE-MIT').unlink()
        with self.assertRaisesRegex(ValueError, 'Missing dependency license'):
          licenses.notices()

  def test_missing_selected_package_rejects_incomplete_inventory(self):
    def cargo(*args):
      return '{"packages": []}' if args[0] == 'metadata' else 'missing v1.0.0\n'
    with patch.object(licenses, 'cargo', cargo), self.assertRaisesRegex(ValueError, 'Incomplete'):
      licenses.notices()

  def test_reviewed_musl_notice_cannot_be_missing_or_changed(self):
    with tempfile.TemporaryDirectory() as directory:
      path = pathlib.Path(directory) / 'musl.txt'
      with patch.object(licenses, 'MUSL_NOTICE', path):
        with self.assertRaisesRegex(ValueError, 'Missing reviewed musl'):
          licenses.musl_notice()
        path.write_text('partial permission text')
        with self.assertRaisesRegex(ValueError, 'notice changed'):
          licenses.musl_notice()

  def test_project_license_is_required_and_included(self):
    with tempfile.TemporaryDirectory() as directory:
      root = pathlib.Path(directory)
      package = {'name': 'guest', 'version': '1.0.0', 'source': None, 'license': 'MIT'}
      metadata = {'workspace_root': str(root), 'packages': [package]}
      def cargo(*args):
        return json.dumps(metadata) if args[0] == 'metadata' else 'guest v1.0.0 (project)\n'
      docs = root / 'share/doc/rust'
      docs.mkdir(parents=True)
      (docs / 'COPYRIGHT-library.html').write_text('<body>standard terms</body>')
      with patch.object(licenses, 'cargo', cargo), patch.object(
          licenses.subprocess, 'check_output', side_effect=lambda args, **kwargs: str(root) if '--print' in args else 'rustc 1.99.0 (test)'):
        with self.assertRaisesRegex(ValueError, 'Missing project license'):
          licenses.notices()
        (root / 'LICENSE').write_text('project permission & copyright')
        self.assertIn(b'project permission &amp; copyright', licenses.notices())
        package['license'] = 'unexpected'
        with self.assertRaisesRegex(ValueError, 'Unexpected project license'):
          licenses.notices()


if __name__ == '__main__':
  unittest.main()
