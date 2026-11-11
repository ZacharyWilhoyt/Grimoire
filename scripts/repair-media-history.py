#!/usr/bin/env python3
"""Relink Grimoire history after exact file renames in a Compose deployment."""
import argparse
from collections import Counter, defaultdict
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
from urllib.parse import parse_qs, unquote, urlsplit

ROOT = Path(__file__).resolve().parents[1]
SCAN_LIMIT = 100_000
REFERENCES = """
SELECT COALESCE(json_agg(r), '[]') FROM (
    SELECT p.source_id, s.path AS root, p.relative_path, p.file_key, m.extract_stamp
    FROM (SELECT source_id,file_key,relative_path FROM media_progress
          UNION SELECT source_id,file_key,relative_path FROM user_media) p
    JOIN sources s ON s.id=p.source_id
    LEFT JOIN media_metadata m ON m.source_id=p.source_id AND m.file_key=p.file_key
) r;
"""
REPAIR = """
BEGIN;
SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '30s';
CREATE TEMP TABLE repair_paths (
    source_id text, root text, old_key text PRIMARY KEY, new_key text UNIQUE, new_path text
) ON COMMIT DROP;
INSERT INTO repair_paths
SELECT * FROM jsonb_to_recordset(convert_from(decode('{mapping}', 'hex'), 'UTF8')::jsonb)
AS m(source_id text, root text, old_key text, new_key text, new_path text);
-- Hold the source mapping stable and prevent concurrent writes while merging.
SELECT s.id FROM sources s JOIN (SELECT DISTINCT source_id FROM repair_paths) m
    ON m.source_id=s.id ORDER BY s.id FOR SHARE OF s;
LOCK TABLE user_media, media_progress IN SHARE ROW EXCLUSIVE MODE;
DELETE FROM repair_paths m WHERE NOT EXISTS (
    SELECT 1 FROM sources s WHERE s.id=m.source_id AND s.path=m.root
);
WITH moved AS (
    DELETE FROM user_media u USING repair_paths m
    WHERE u.source_id=m.source_id AND u.file_key=m.old_key
    RETURNING u.user_id,m.new_key,u.source_id,m.new_path,u.saved_at,u.last_opened_at
), merged AS (
    INSERT INTO user_media(user_id,file_key,source_id,relative_path,saved_at,last_opened_at)
    SELECT * FROM moved
    ON CONFLICT (user_id,file_key) DO UPDATE SET
        saved_at=LEAST(user_media.saved_at,EXCLUDED.saved_at),
        last_opened_at=GREATEST(user_media.last_opened_at,EXCLUDED.last_opened_at)
    RETURNING 1
) SELECT 'Saved/history rows relinked: ' || count(*) FROM merged;
WITH moved AS (
    DELETE FROM media_progress p USING repair_paths m
    WHERE p.source_id=m.source_id AND p.file_key=m.old_key
    RETURNING p.user_id,m.new_key,p.source_id,m.new_path,p.position,p.total,p.locator,
              p.revision,p.updated_at,p.watched_seconds,p.completed_units,p.completed
), merged AS (
    INSERT INTO media_progress(user_id,file_key,source_id,relative_path,position,total,locator,
                               revision,updated_at,watched_seconds,completed_units,completed)
    SELECT * FROM moved
    ON CONFLICT (user_id,file_key) DO UPDATE SET
        position=CASE WHEN EXCLUDED.updated_at>media_progress.updated_at THEN EXCLUDED.position ELSE media_progress.position END,
        total=CASE WHEN EXCLUDED.updated_at>media_progress.updated_at THEN EXCLUDED.total ELSE media_progress.total END,
        locator=CASE WHEN EXCLUDED.updated_at>media_progress.updated_at THEN EXCLUDED.locator ELSE media_progress.locator END,
        revision=GREATEST(media_progress.revision,EXCLUDED.revision)+1,
        updated_at=GREATEST(media_progress.updated_at,EXCLUDED.updated_at),
        watched_seconds=GREATEST(media_progress.watched_seconds,EXCLUDED.watched_seconds),
        completed_units=ARRAY(SELECT DISTINCT unit FROM unnest(media_progress.completed_units || EXCLUDED.completed_units) AS unit ORDER BY unit),
        completed=media_progress.completed OR EXCLUDED.completed
    RETURNING 1
) SELECT 'Progress rows relinked: ' || count(*) FROM merged;
COMMIT;
"""


def file_key(source, path):
    return hashlib.sha256(f'{source}\0{path}'.encode()).hexdigest()


def fingerprint(info):
    return f'{info.st_size}:{info.st_mtime_ns}'


def scan(root, wanted):
    matches = defaultdict(list)
    pending = [(root, 0)]
    count = 0
    try:
        while pending:
            directory, depth = pending.pop()
            with os.scandir(directory) as entries:
                for entry in entries:
                    count += 1
                    if count > SCAN_LIMIT:
                        return {}
                    if entry.is_dir(follow_symlinks=False):
                        if depth >= 32:
                            return {}
                        pending.append((Path(entry.path), depth + 1))
                    elif entry.is_file(follow_symlinks=False):
                        path = Path(entry.path)
                        if path.suffix.lower() not in wanted:
                            continue
                        stamp = fingerprint(entry.stat(follow_symlinks=False))
                        if stamp in wanted[path.suffix.lower()]:
                            matches[(path.suffix.lower(), stamp)].append(path)
    except OSError:
        return {}
    return matches


def plan(rows, mounts):
    missing = defaultdict(list)
    for row in rows:
        source_root, relative = Path(row['root']), Path(row['relative_path'])
        if relative.is_absolute() or '..' in relative.parts or row['file_key'] != file_key(row['source_id'], row['relative_path']):
            continue
        mount = next((mount for mount in mounts if source_root.is_relative_to(mount[0])), None)
        if mount is None:
            continue
        destination, host = mount
        root = host / source_root.relative_to(destination)
        try:
            if root.resolve(strict=True) != root or not root.is_dir():
                continue
            old = root / relative
            old.lstat()
        except FileNotFoundError:
            if root.is_dir():
                missing[(row['source_id'], row['root'], root)].append((row, old))
        except (OSError, RuntimeError):
            continue

    repairs, unresolved = [], []
    for (source, source_root, root), items in missing.items():
        fingerprints = Counter((old.suffix.lower(), row['extract_stamp']) for row, old in items)
        wanted = defaultdict(set)
        for (suffix, stamp), count in fingerprints.items():
            if count == 1 and stamp and re.fullmatch(r'[1-9]\d*:[1-9]\d*', stamp):
                wanted[suffix].add(stamp)
        matches = scan(root, wanted) if wanted else {}
        for row, old in items:
            candidates = matches.get((old.suffix.lower(), row['extract_stamp']), [])
            if len(candidates) != 1:
                unresolved.append(row['relative_path'])
                continue
            new = candidates[0]
            # Recheck both sides after the scan, before constructing any DB updates.
            try:
                if old.exists() or old.is_symlink() or new.resolve(strict=True) != new or fingerprint(new.stat()) != row['extract_stamp']:
                    unresolved.append(row['relative_path'])
                    continue
            except OSError:
                unresolved.append(row['relative_path'])
                continue
            relative = new.relative_to(root).as_posix()
            repairs.append(dict(source_id=source, root=source_root, old_key=row['file_key'],
                                new_key=file_key(source, relative), new_path=relative))
    return repairs, unresolved


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--apply', action='store_true', help='Write the exact matches to the running database')
    args = parser.parse_args()
    compose = ['docker', 'compose', '--env-file', 'deploy/.env', '-f', 'deploy/compose.yaml']

    def run(command, sql=None):
        result = subprocess.run(command, cwd=ROOT, input=sql, text=True, capture_output=True, timeout=90)
        if result.returncode:
            raise SystemExit('Repair stopped because a deployment or database command failed.')
        return result.stdout.strip()

    uri = urlsplit(run([*compose, 'exec', '-T', 'api', 'printenv', 'DATABASE_URL']))
    database = unquote(uri.path.removeprefix('/'))
    if (uri.hostname != 'localhost' or parse_qs(uri.query) != {'host': ['/var/run/postgresql']}
            or not re.fullmatch(r'[A-Za-z0-9_]+', database)):
        raise SystemExit('Repair requires the running API to use the Compose PostgreSQL service.')
    psql = [*compose, 'exec', '-T', '--user', 'postgres', 'postgres', 'sh', '-c',
            'export PGPASSWORD="$POSTGRES_PASSWORD"; exec psql -X -qAt -v ON_ERROR_STOP=1 --no-password -h /var/run/postgresql -U "${POSTGRES_USER:-postgres}" -d "$1"',
            'repair-media-history', database]
    api = run([*compose, 'ps', '-q', 'api'])
    mounts = json.loads(run(['docker', 'inspect', '--format', '{{json .Mounts}}', api]))
    binds = sorted(
        [(Path(mount['Destination']), Path(mount['Source'])) for mount in mounts if mount['Type'] == 'bind'],
        key=lambda mount: len(mount[0].parts), reverse=True,
    )
    repairs, unresolved = plan(json.loads(run(psql, REFERENCES)), binds)
    print(f'Exact rename matches: {len(repairs)}. Unresolved paths: {len(unresolved)}.')
    for repair in repairs:
        print(f'  Match: {repair["new_path"]!r}')
    for path in unresolved:
        print(f'  Unresolved: {path!r}')
    if args.apply and repairs:
        result = run(psql, REPAIR.format(mapping=json.dumps(repairs).encode().hex()))
        print('\n'.join(line for line in result.splitlines() if line.startswith(('Saved/history', 'Progress'))))
    elif not args.apply:
        print('Dry run. Pass --apply to relink exact matches.')


if __name__ == '__main__':
    main()
