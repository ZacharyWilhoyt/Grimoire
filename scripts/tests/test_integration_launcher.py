import os
from pathlib import Path
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / 'test-backend-integration.sh'


class IntegrationLauncherTests(unittest.TestCase):
    def test_missing_database_is_rejected_before_cargo(self):
        env = dict(os.environ)
        env.pop('DATABASE_URL', None)
        result = subprocess.run([str(SCRIPT)], env=env, capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Set DATABASE_URL explicitly', result.stderr)

    def test_application_database_is_rejected_without_leaking_password(self):
        env = dict(os.environ, DATABASE_URL='postgresql://app:sensitive_dummy_password@localhost/grimoire')
        result = subprocess.run([str(SCRIPT)], env=env, capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn('sensitive_dummy_password', result.stdout + result.stderr)
        self.assertIn('Refusing integration tests', result.stderr)

    def test_explicit_test_database_reaches_cargo(self):
        with tempfile.TemporaryDirectory() as temp:
            cargo = Path(temp) / 'cargo'
            cargo.write_text('#!/bin/sh\nprintf "%s\\n" "$@"\n')
            cargo.chmod(0o755)
            env = dict(os.environ, DATABASE_URL='postgresql://test:dummy@localhost/grimoire_tests', PATH=temp + os.pathsep + os.environ['PATH'])
            result = subprocess.run([str(SCRIPT), 'security::tls'], env=env, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn('--locked', result.stdout)
            self.assertIn('security::tls', result.stdout)
            self.assertIn('--ignored', result.stdout)
