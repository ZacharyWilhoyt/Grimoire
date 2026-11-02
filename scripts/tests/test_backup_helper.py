"""Safety checks use fake PostgreSQL tools; no server or credentials required."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / 'backup-control-plane.sh'


class BackupHelperTests(unittest.TestCase):
    def setUp(self):
        self.workspace = tempfile.TemporaryDirectory()
        self.addCleanup(self.workspace.cleanup)
        self.root = Path(self.workspace.name)
        self.bin = self.root / 'bin'
        self.bin.mkdir()
        self.output = self.root / 'backup.dump'
        self.env = dict(os.environ, PATH=f'{self.bin}:{os.environ["PATH"]}', PGSERVICE='test_profile')
        self.tool('pg_dump', "printf 'synthetic archive'\n")
        self.tool('pg_restore', 'exit 0\n')

    def tool(self, name, body):
        path = self.bin / name
        path.write_text('#!/bin/sh\n' + body)
        path.chmod(0o700)

    def run_backup(self, output=None):
        return subprocess.run(['bash', str(SCRIPT), str(output or self.output)], env=self.env, capture_output=True, text=True)

    def test_private_file_and_existing_backup_is_never_overwritten(self):
        self.assertEqual(self.run_backup().returncode, 0)
        self.assertEqual(self.output.stat().st_mode & 0o777, 0o600)
        original = self.output.read_bytes()
        self.assertNotEqual(self.run_backup().returncode, 0)
        self.assertEqual(self.output.read_bytes(), original)
        self.assertFalse(list(self.root.glob('.grimoire-backup.*')))

    def test_failed_dump_or_archive_validation_publishes_nothing(self):
        for tool in ['pg_dump', 'pg_restore']:
            with self.subTest(tool=tool):
                self.tool(tool, 'exit 1\n')
                self.assertNotEqual(self.run_backup().returncode, 0)
                self.assertFalse(self.output.exists())
                self.assertFalse(list(self.root.glob('.grimoire-backup.*')))
                self.tool(tool, "printf 'synthetic archive'\n" if tool == 'pg_dump' else 'exit 0\n')

    def test_missing_explicit_service_or_writable_directory_is_rejected(self):
        del self.env['PGSERVICE']
        self.assertNotEqual(self.run_backup().returncode, 0)
        self.env['PGSERVICE'] = 'test_profile'
        unsafe = self.root / 'unsafe'
        unsafe.mkdir(mode=0o777)
        unsafe.chmod(0o777)
        self.assertNotEqual(self.run_backup(unsafe / 'backup.dump').returncode, 0)
        self.assertFalse((unsafe / 'backup.dump').exists())

    def test_symlink_target_is_not_followed(self):
        target = self.root / 'existing'
        target.write_text('keep me')
        self.output.symlink_to(target)
        self.assertNotEqual(self.run_backup().returncode, 0)
        self.assertEqual(target.read_text(), 'keep me')

    def test_publication_race_does_not_treat_new_directory_as_destination(self):
        self.env['DUMP_RACE_TARGET'] = str(self.output)
        self.tool('pg_dump', 'mkdir "$DUMP_RACE_TARGET"\nprintf synthetic\n')
        self.assertNotEqual(self.run_backup().returncode, 0)
        self.assertTrue(self.output.is_dir())
        self.assertEqual(list(self.output.iterdir()), [])
        self.assertFalse(list(self.root.glob('.grimoire-backup.*')))

    def test_database_uri_credentials_are_not_passed_as_arguments_or_printed(self):
        arguments = self.root / 'arguments.txt'
        self.env['DUMP_ARGS'] = str(arguments)
        self.env['DATABASE_URL'] = 'postgresql://user:private_test_secret@host/database'
        self.tool('pg_dump', 'printf "%s\\n" "$@" > "$DUMP_ARGS"\nprintf synthetic\n')
        result = self.run_backup()
        self.assertEqual(result.returncode, 0)
        self.assertNotIn('private_test_secret', result.stdout + result.stderr + arguments.read_text())


if __name__ == '__main__':
    unittest.main()
