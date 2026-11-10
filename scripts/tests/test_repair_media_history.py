import importlib.util
import os
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest

SPEC = importlib.util.spec_from_file_location(
    'repair_media_history', Path(__file__).resolve().parents[1] / 'repair-media-history.py')
repair = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(repair)


class RepairPlanTests(unittest.TestCase):
    def test_matches_move_under_the_same_mount_only(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            file = root / 'new episode.mkv'
            file.write_bytes(b'test media')
            row = dict(source_id='series', root='/media', relative_path='old episode.mkv',
                       file_key=repair.file_key('series', 'old episode.mkv'),
                       extract_stamp=repair.fingerprint(file.stat()))
            mounts = [(Path('/media'), root)]
            plans, unresolved = repair.plan([row], mounts)
            self.assertEqual(unresolved, [])
            self.assertEqual(plans[0]['new_path'], file.name)
            self.assertEqual(plans[0]['root'], '/media')

            alias = root / 'symlink.mkv'
            alias.symlink_to(file)
            self.assertEqual(len(repair.plan([row], mounts)[0]), 1)
            duplicate = root / 'duplicate.mkv'
            duplicate.write_bytes(file.read_bytes())
            stamp = file.stat().st_mtime_ns
            os.utime(duplicate, ns=(stamp, stamp))
            self.assertEqual(repair.plan([row], mounts), ([], [row['relative_path']]))

    def test_existing_path_and_missing_fingerprint_are_never_relinked(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            file = root / 'original.mkv'
            file.write_bytes(b'test media')
            row = dict(source_id='series', root='/media', relative_path=file.name,
                       file_key=repair.file_key('series', file.name),
                       extract_stamp=repair.fingerprint(file.stat()))
            mounts = [(Path('/media'), root)]
            self.assertEqual(repair.plan([row], mounts), ([], []))
            file.rename(root / 'new.mkv')
            row['extract_stamp'] = None
            self.assertEqual(repair.plan([row], mounts), ([], [file.name]))
