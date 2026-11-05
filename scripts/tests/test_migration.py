import importlib.util
import json
import os
from pathlib import Path
import tempfile
import types
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).resolve().parents[1] / 'migrate-control-plane.py'
spec = importlib.util.spec_from_file_location('migration', SCRIPT)
migration = importlib.util.module_from_spec(spec)
spec.loader.exec_module(migration)


class MigrationTests(unittest.TestCase):
    def test_source_config_is_parsed_without_shell_execution_or_ambient_database(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / 'source.env'
            path.write_text("DATABASE_URL='postgresql://app:secret%24%28text%29@127.0.0.1/grimoire'\nGRIMOIRE_ARR_ALLOWED_URLS='[\"http://localhost:8989/\"]'\n")
            data = migration.env_file(path)
            with patch.dict(os.environ, {'PGDATABASE': 'wrong', 'PGSERVICE': 'wrong'}):
                env = migration.source_environment(data['DATABASE_URL'])
            self.assertEqual(env['PGDATABASE'], 'grimoire')
            self.assertEqual(env['PGPASSWORD'], 'secret$(text)')
            self.assertNotIn('PGSERVICE', env)
            self.assertIn('default_transaction_read_only=on', env['PGOPTIONS'])
            self.assertEqual(json.loads(data['GRIMOIRE_ARR_ALLOWED_URLS']), ['http://localhost:8989/'])

    def test_restore_creates_only_a_fresh_database_and_does_not_activate_config(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            backup = root / 'backup'; backup.mkdir()
            archive = backup / 'control-plane.dump'; archive.write_bytes(b'synthetic archive')
            counts = {'users': 2, 'sources': 2}
            (backup / 'manifest.json').write_text(json.dumps({'version': 1, 'sha256': migration.digest(archive), 'tables': list(counts), 'counts': counts}))
            (backup / 'source-settings.private.json').write_text(json.dumps({'GRIMOIRE_INTEGRATION_KEY': 'a' * 64}))
            env = root / '.env'; original = 'GRIMOIRE_DB_PASSWORD=dummy\n'; env.write_text(original)
            calls = []
            class Container:
                def __init__(self, *_): pass
                def sql(self, db, sql):
                    calls.append((db, sql))
                    return json.dumps(counts)
                def pg(self, arguments, **_): calls.append(arguments)
            with patch.object(migration, 'Container', Container), patch('builtins.print'):
                migration.restore(types.SimpleNamespace(backup=backup, deploy_env=env, compose=root / 'compose.yaml'))
            self.assertTrue(calls[0][1].startswith('CREATE DATABASE "grimoire_import_'))
            self.assertFalse(any('DROP' in str(call) or '--clean' in str(call) for call in calls))
            self.assertIn('--single-transaction', calls[1])
            self.assertEqual(env.read_text(), original)
            result = json.loads((backup / 'restore-result.json').read_text())
            candidate = Path(result['candidate_env'])
            self.assertIn('GRIMOIRE_DATABASE_NAME=grimoire_import_', candidate.read_text())
            self.assertIn('a' * 64, candidate.read_text())

    def test_modified_archive_is_rejected_before_docker_is_used(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / 'control-plane.dump').write_bytes(b'changed')
            (root / 'manifest.json').write_text(json.dumps({'version': 1, 'sha256': 'wrong'}))
            with patch.object(migration, 'Container') as container:
                with self.assertRaises(migration.MigrationError):
                    migration.restore(types.SimpleNamespace(backup=root))
                container.assert_not_called()

    def test_interpolated_or_executable_config_is_not_evaluated(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / 'source.env'
            path.write_text('DATABASE_URL=${OTHER_SECRET}\n')
            with self.assertRaises(migration.MigrationError): migration.env_file(path)
            path.write_text('DATABASE_URL="x" + __import__("os").system("false")\n')
            with self.assertRaises(migration.MigrationError): migration.env_file(path)
