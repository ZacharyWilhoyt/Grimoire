import os
from pathlib import Path
import shutil
import stat
import subprocess
import tempfile
import unittest

SOURCE = Path(__file__).resolve().parents[2] / 'deploy' / 'setup.sh'


class DeploymentSetupTests(unittest.TestCase):
    def test_secrets_are_private_and_existing_configuration_is_preserved(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            script = root / 'setup.sh'
            shutil.copy(SOURCE, script)
            mock = root / 'openssl'
            mock.write_text('#!/bin/sh\nprintf "%s\\n" "' + 'ab' * 32 + '"\n')
            mock.chmod(0o755)
            env = dict(os.environ, PATH=str(root) + os.pathsep + os.environ['PATH'])
            result = subprocess.run([str(script)], env=env, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            contents = (root / '.env').read_text()
            self.assertEqual(stat.S_IMODE((root / '.env').stat().st_mode), 0o600)
            self.assertIn('GRIMOIRE_PROXY_TOKEN=' + 'ab' * 32, contents)
            self.assertNotIn('ab' * 32, result.stdout)
            second = subprocess.run([str(script)], env=env, capture_output=True, text=True)
            self.assertEqual(second.returncode, 0)
            self.assertEqual((root / '.env').read_text(), contents)

    def test_random_generation_failure_does_not_leave_configuration(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            script = root / 'setup.sh'
            shutil.copy(SOURCE, script)
            mock = root / 'openssl'
            mock.write_text('#!/bin/sh\nexit 1\n')
            mock.chmod(0o755)
            env = dict(os.environ, PATH=str(root) + os.pathsep + os.environ['PATH'])
            result = subprocess.run([str(script)], env=env, capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse((root / '.env').exists())
            self.assertFalse(list(root.glob('.env.setup.*')))
