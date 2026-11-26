"""Offline launcher settings regressions; never invokes a backend or game."""
import contextlib
import io
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import starport


class NativeRenderingTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.data = Path(self.temp.name)
        self.settings = self.data/'7952987A'/'storage'/'settings'/'settings.xml'
        self.settings.parent.mkdir(parents=True)
        self.original = (b'<?xml version="1.0"?>\r\n<rage__fwSystemSettingsCollection>\r\n'
                         b'  <graphics><gamma value="15" /><fsrQuality value="2" /></graphics>\r\n'
                         b'  <video><resolutionFullscreen><width value="3840" />'
                         b'</resolutionFullscreen></video>\r\n</rage__fwSystemSettingsCollection>\r\n')
        self.settings.write_bytes(self.original)

    def run_preset(self, mode='off'):
        with contextlib.redirect_stdout(io.StringIO()):
            return starport.configure_fsr(self.data, mode)

    def test_only_fsr_value_changes_and_exact_backup_survives_repeated_runs(self):
        record = self.run_preset()['files'][0]
        self.assertEqual(self.settings.read_bytes(), self.original.replace(b'value="2"', b'value="0"'))
        backup = Path(record['backup'])
        self.assertEqual(backup.read_bytes(), self.original)
        self.assertFalse(self.run_preset()['files'][0]['changed'])
        self.assertEqual(backup.read_bytes(), self.original)
        self.assertEqual(len(list(self.settings.parent.glob('*.bak'))), 1)

    def test_preserve_makes_no_writes(self):
        self.assertEqual(self.run_preset('preserve'), dict(mode='preserve', files=[]))
        self.assertEqual(self.settings.read_bytes(), self.original)
        self.assertEqual(list(self.settings.parent.iterdir()), [self.settings])

    def test_ambiguous_or_malformed_xml_is_rejected_without_modification(self):
        for source in (b'<broken', self.original.replace(b'</graphics>', b'<fsrQuality value="1" /></graphics>')):
            self.settings.write_bytes(source)
            with self.assertRaises((ValueError, starport.ET.ParseError)):
                self.run_preset()
            self.assertEqual(self.settings.read_bytes(), source)
            self.assertEqual(list(self.settings.parent.iterdir()), [self.settings])

    def test_failed_atomic_replace_preserves_original_and_backup(self):
        with patch.object(starport.os, 'replace', side_effect=OSError('simulated failure')):
            with self.assertRaises(OSError):
                self.run_preset()
        self.assertEqual(self.settings.read_bytes(), self.original)
        self.assertEqual(next(self.settings.parent.glob('*.bak')).read_bytes(), self.original)
        self.assertFalse(list(self.settings.parent.glob('*.tmp')))


if __name__ == '__main__':
    unittest.main()
