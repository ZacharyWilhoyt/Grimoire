#!/usr/bin/env python3
"""Snapshot an existing Grimoire DB and restore only into a fresh container DB.

Never sources shell configuration, passes credentials as arguments, drops a
DB, changes the source DB, or switches the running application's connection.
"""
import argparse
import ast
import hashlib
import json
import os
from pathlib import Path
import re
import selectors
import subprocess
import sys
import uuid
from urllib.parse import parse_qsl, unquote, urlsplit

ROOT = Path(__file__).resolve().parents[1]
SECRET_FIELDS = ('GRIMOIRE_INTEGRATION_KEY', 'GRIMOIRE_ARR_ALLOWED_URLS')


class MigrationError(Exception):
    pass


def env_file(path):
    result = {}
    for line in Path(path).read_text().splitlines():
        line = line.strip()
        if not line or line.startswith('#'):
            continue
        if line.startswith('export '):
            line = line[7:]
        if '=' not in line:
            raise MigrationError('Unsupported environment-file syntax; expected KEY=value.')
        key, value = line.split('=', 1)
        key, value = key.strip(), value.strip()
        if value.startswith(('"', "'")):
            try:
                value = ast.literal_eval(value)
            except (SyntaxError, ValueError):
                raise MigrationError('Unsupported quoted environment value; use a resolved single-line value.') from None
            if not isinstance(value, str):
                raise MigrationError('Environment values must be strings.')
        else:
            value = re.split(r'\s+#', value, maxsplit=1)[0].rstrip()
        if '${' in value:
            raise MigrationError('Interpolated environment values are not supported; provide a resolved source config.')
        result[key] = value
    return result


def source_environment(uri):
    try:
        url = urlsplit(uri)
        if url.scheme not in ('postgres', 'postgresql') or url.fragment:
            raise ValueError()
        values = {'host': url.hostname or '', 'port': str(url.port or 5432), 'user': unquote(url.username or ''), 'password': unquote(url.password or ''), 'dbname': unquote(url.path.removeprefix('/'))}
        names = {'host': 'PGHOST', 'port': 'PGPORT', 'user': 'PGUSER', 'password': 'PGPASSWORD', 'dbname': 'PGDATABASE', 'sslmode': 'PGSSLMODE', 'sslrootcert': 'PGSSLROOTCERT', 'sslcert': 'PGSSLCERT', 'sslkey': 'PGSSLKEY', 'sslcrl': 'PGSSLCRL', 'connect_timeout': 'PGCONNECT_TIMEOUT', 'options': 'PGOPTIONS'}
        for key, value in parse_qsl(url.query):
            if key not in names:
                raise ValueError()
            values[key] = value
        if not values['dbname'] or not values['user']:
            raise ValueError()
    except ValueError:
        raise MigrationError('Invalid or unsupported source PostgreSQL URL; database and user must be explicit.') from None
    environment = {k: v for k, v in os.environ.items() if not k.startswith('PG')}
    environment.update({names[k]: v for k, v in values.items() if v})
    environment['PGCONNECT_TIMEOUT'] = '10'
    environment['PGAPPNAME'] = 'grimoire-readonly-migration'
    environment['PGOPTIONS'] = environment.get('PGOPTIONS', '') + ' -c default_transaction_read_only=on -c statement_timeout=60000'
    return environment


def ident(name):
    return '"' + name.replace('"', '""') + '"'


def counts_sql(tables):
    fields = []
    for table in tables:
        fields.extend(["'" + table.replace("'", "''") + "'", '(SELECT count(*) FROM public.' + ident(table) + ')'])
    return 'SELECT json_build_object(' + ','.join(fields) + ');'


class Snapshot:
    def __init__(self, environment):
        self.environment = environment
        self.process = None

    def __enter__(self):
        self.process = subprocess.Popen(['psql', '-X', '-qAt', '--no-password', '-v', 'ON_ERROR_STOP=1'], env=self.environment, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
        try:
            self.process.stdin.write('BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY;\n')
            self.id = self.query('SELECT pg_export_snapshot();')
            if not re.fullmatch(r'[0-9A-Fa-f-]+', self.id):
                raise MigrationError('The source database did not provide a valid read-only snapshot.')
            return self
        except Exception:
            self.close()
            raise

    def query(self, sql):
        self.process.stdin.write(sql + '\n')
        self.process.stdin.flush()
        with selectors.DefaultSelector() as selector:
            selector.register(self.process.stdout, selectors.EVENT_READ)
            if not selector.select(65):
                raise MigrationError('The source snapshot query timed out.')
        result = self.process.stdout.readline().strip()
        if not result:
            raise MigrationError('The source snapshot query failed. Check source connectivity and read permissions.')
        return result

    def close(self):
        if self.process:
            if self.process.poll() is None:
                self.process.terminate()
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait()
            self.process = None

    def __exit__(self, *_):
        self.close()  # PostgreSQL rolls back the read-only snapshot transaction.


def private_json(path, value):
    with path.open('x') as stream:
        json.dump(value, stream, indent=2)
        stream.write('\n')


def digest(path):
    value = hashlib.sha256()
    with path.open('rb') as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b''):
            value.update(chunk)
    return value.hexdigest()


def backup(args):
    config = env_file(args.source_env)
    if not config.get('DATABASE_URL'):
        raise MigrationError('The explicit source environment file needs DATABASE_URL.')
    environment = source_environment(config['DATABASE_URL'])
    destination = Path(args.output).resolve()
    destination.mkdir(mode=0o700, parents=True, exist_ok=False)
    try:
        with Snapshot(environment) as snapshot:
            tables = json.loads(snapshot.query("SELECT coalesce(json_agg(tablename ORDER BY tablename),'[]') FROM pg_tables WHERE schemaname='public';"))
            if not {'users', 'sources', '_sqlx_migrations'}.issubset(tables):
                raise MigrationError('The source does not look like a Grimoire database.')
            counts = json.loads(snapshot.query(counts_sql(tables)))
            key = config.get('GRIMOIRE_INTEGRATION_KEY', '')
            if counts.get('arr_connections', 0) and not re.fullmatch(r'[a-fA-F0-9]{64}', key):
                raise MigrationError('Existing Arr connections require the original GRIMOIRE_INTEGRATION_KEY in the source config.')
            archive = destination / 'control-plane.dump'
            with archive.open('xb') as output:
                result = subprocess.run(['pg_dump', '--format=custom', '--no-owner', '--no-privileges', '--no-password', '--exclude-schema=_sqlx_test', '--lock-wait-timeout=10s', '--snapshot', snapshot.id], env=environment, stdout=output, stderr=subprocess.DEVNULL)
            if result.returncode:
                raise MigrationError('Source dump failed. Check PostgreSQL client/server versions and source read permissions.')
            result = subprocess.run(['pg_restore', '--list', str(archive)], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            if result.returncode:
                raise MigrationError('The generated dump did not pass archive validation.')
            settings = {name: config[name] for name in SECRET_FIELDS if config.get(name)}
            if not re.fullmatch(r'[a-fA-F0-9]{64}', key):
                settings.pop('GRIMOIRE_INTEGRATION_KEY', None)
            private_json(destination / 'source-settings.private.json', settings)
            private_json(destination / 'manifest.json', {'version': 1, 'sha256': digest(archive), 'tables': tables, 'counts': counts})
        print('Created a consistent, read-only source backup at:', destination)
        print('Rows captured:', json.dumps({name: counts.get(name, 0) for name in ('users', 'sources', 'user_media', 'media_progress', 'media_metadata')}))
    except Exception:
        # Retain any partial output privately for inspection; never publish a success manifest.
        raise


class Container:
    def __init__(self, env_path, compose_path):
        self.command = ['docker', 'compose', '--env-file', str(env_path), '-f', str(compose_path)]

    def pg(self, arguments, data=None, stream=None):
        command = self.command + ['exec', '-T', '--user', 'postgres', 'postgres', 'sh', '-c', 'export PGPASSWORD="$POSTGRES_PASSWORD"; exec "$@"', 'grimoire-migration'] + arguments
        result = subprocess.run(command, input=data, stdin=stream, capture_output=True)
        if result.returncode:
            raise MigrationError('Container PostgreSQL operation failed. The source and existing destination databases were left in place.')
        return result.stdout.decode().strip()

    def sql(self, database, sql):
        return self.pg(['psql', '-X', '-qAt', '--no-password', '-v', 'ON_ERROR_STOP=1', '-h', '/var/run/postgresql', '-U', 'postgres', '-d', database], data=sql.encode())


def dotenv_value(value):
    return "'" + value.replace("'", "\\'") + "'"


def restore(args):
    folder = Path(args.backup).resolve()
    archive = folder / 'control-plane.dump'
    manifest = json.loads((folder / 'manifest.json').read_text())
    if manifest.get('version') != 1 or digest(archive) != manifest.get('sha256'):
        raise MigrationError('The backup archive does not match its manifest.')
    env_path = Path(args.deploy_env).resolve()
    existing = env_file(env_path)
    if not existing.get('GRIMOIRE_DB_PASSWORD'):
        raise MigrationError('The destination deployment config lacks its application database password.')
    name = 'grimoire_import_' + uuid.uuid4().hex[:16]
    container = Container(env_path, Path(args.compose).resolve())
    # Only CREATE; no DROP, --clean, overwrites or active-connection changes.
    container.sql('postgres', 'CREATE DATABASE ' + ident(name) + ' OWNER grimoire;')
    with archive.open('rb') as stream:
        container.pg(['pg_restore', '--no-owner', '--no-privileges', '--role=grimoire', '--single-transaction', '--exit-on-error', '--no-password', '-h', '/var/run/postgresql', '-U', 'postgres', '-d', name], stream=stream)
    actual = json.loads(container.sql(name, counts_sql(manifest['tables'])))
    if actual != manifest['counts']:
        raise MigrationError('Restored row counts differ from the source snapshot. The new database was retained; no cutover occurred.')
    settings = json.loads((folder / 'source-settings.private.json').read_text())
    candidate = env_path.parent / ('.env.import-' + name.removeprefix('grimoire_import_'))
    with candidate.open('x') as output:
        output.write(env_path.read_text().rstrip() + '\n\n# Verified imported database; not activated automatically.\n')
        output.write('GRIMOIRE_DATABASE_NAME=' + name + '\n')
        for key in SECRET_FIELDS:
            if settings.get(key):
                output.write(key + '=' + dotenv_value(settings[key]) + '\n')
    if os.geteuid() == 0:
        owner = env_path.stat()
        os.chown(candidate, owner.st_uid, owner.st_gid)
    result_path = folder / 'restore-result.json'
    private_json(result_path, {'database': name, 'verified_counts': actual, 'candidate_env': str(candidate)})
    if os.geteuid() == 0:
        owner = folder.stat()
        os.chown(result_path, owner.st_uid, owner.st_gid)
    print('Restored and verified a NEW database:', name)
    print('Existing databases, the source, and the running API connection were preserved.')
    print('Private candidate deployment config:', candidate)
    print('Cutover requires a source-write pause/final snapshot and API recreation with that config; see docs/DATABASE_MIGRATION.md.')


def main():
    os.umask(0o077)
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    first = commands.add_parser('backup')
    first.add_argument('--source-env', type=Path, default=ROOT / 'backend/.env')
    first.add_argument('--output', type=Path, required=True)
    second = commands.add_parser('restore')
    second.add_argument('--backup', type=Path, required=True)
    second.add_argument('--deploy-env', type=Path, default=ROOT / 'deploy/.env')
    second.add_argument('--compose', type=Path, default=ROOT / 'deploy/compose.yaml')
    args = parser.parse_args()
    try:
        backup(args) if args.command == 'backup' else restore(args)
    except MigrationError as error:
        print(str(error), file=sys.stderr)
        print('No automatic cutover or deletion was performed.', file=sys.stderr)
        return 1
    except (OSError, ValueError, subprocess.SubprocessError):
        # No exception repr: connection URLs and config values must stay private.
        print('Migration did not complete. No automatic cutover or deletion was performed.', file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
