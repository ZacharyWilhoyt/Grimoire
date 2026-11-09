#!/usr/bin/env python3
"""Enable the optional controller using explicitly approved API media mounts."""
import argparse
import concurrent.futures
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
DEPLOY = ROOT / 'deploy'
ENV = DEPLOY / '.env'
RUNTIME = DEPLOY / 'runtime/arr-controller'
APPS = ('sonarr', 'radarr', 'lidarr')


def run(*command, capture=False):
    return subprocess.run(command, text=True, check=True, stdout=subprocess.PIPE if capture else None).stdout


def write_atomic(path, data, mode=0o600):
    fd, temporary = tempfile.mkstemp(dir=path.parent, prefix='.env.setup.' if path == ENV else '.arr-')
    try:
        with os.fdopen(fd, 'w') as stream:
            os.fchmod(stream.fileno(), mode)
            stream.write(data)
        os.replace(temporary, path)
    finally:
        Path(temporary).unlink(missing_ok=True)


def approved_image(kind):
    repository = f'lscr.io/linuxserver/{kind}'
    reference = repository + ':latest'
    result = subprocess.run(['docker', 'image', 'inspect', '--format', '{{json .RepoDigests}}', reference], capture_output=True, text=True)
    if result.returncode:
        run('docker', 'pull', reference)
        result = subprocess.run(['docker', 'image', 'inspect', '--format', '{{json .RepoDigests}}', reference], capture_output=True, text=True, check=True)
    return next(digest for digest in json.loads(result.stdout) if digest.startswith(repository + '@sha256:'))


def main():
    os.umask(0o077)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--media-root', action='append', required=True, help='Exact read-only bind mount destination in the running API container; repeat as needed')
    parser.add_argument('--max-instances', type=int, choices=range(1, 49), default=12, metavar='1–48')
    parser.add_argument('--prepare-only', action='store_true', help='Prepare operator policy, media volumes and Compose override without changing running services')
    parser.add_argument('--skip-build', action='store_true', help='Use images already built from this checkout')
    args = parser.parse_args()
    base = ['docker', 'compose', '--env-file', str(ENV), '-f', str(DEPLOY / 'compose.yaml')]
    cid = run(*base, 'ps', '-q', 'api', capture=True).strip()
    if not cid:
        parser.error('Start the base Grimoire deployment first.')
    # Inspect contains environment secrets. Only mount paths and the project label are used.
    api = json.loads(run('docker', 'inspect', cid, capture=True))[0]
    project = api['Config']['Labels']['com.docker.compose.project']
    mounts = {mount['Destination']: mount for mount in api['Mounts'] if mount['Type'] == 'bind' and not mount['RW']}
    roots, volumes = [], []
    for destination in sorted(set(args.media_root)):
        mount = mounts.get(destination)
        if mount is None or destination == '/' or destination.startswith(('/run/', '/var/run/', '/etc/', '/state/', '/proc/', '/sys/', '/dev/')):
            parser.error('Each media root must be an existing read-only API directory bind mount outside reserved system paths.')
        host = Path(mount['Source']).resolve(strict=True)
        if host == Path('/') or not host.is_dir():
            parser.error('Media mounts must point to specific host directories.')
        key = hashlib.sha256(os.fsencode(host)).hexdigest()[:24]
        volume = f'{project}_arr_media_{key}'
        run('docker', 'volume', 'create', '--driver', 'local', '--opt', 'type=none', '--opt', 'o=bind', '--opt', 'device=' + str(host), volume, capture=True)
        approved = json.loads(run('docker', 'volume', 'inspect', volume, capture=True))[0]
        if approved['Driver'] != 'local' or approved['Options'] != {'type': 'none', 'o': 'bind', 'device': str(host)}:
            parser.error('A media volume already exists with different options.')
        roots.append({'container_path': destination, 'volume': volume})
        volumes.append({'type': 'bind', 'source': str(host), 'target': destination, 'read_only': True, 'bind': {'create_host_path': False}})
    with concurrent.futures.ThreadPoolExecutor(max_workers=3) as pool:
        images = dict(zip(APPS, pool.map(approved_image, APPS)))
    RUNTIME.mkdir(parents=True, mode=0o700, exist_ok=True)
    policy = {'images': images, 'media_roots': roots, 'network': project + '_arr', 'state_volume': project + '_arr_state', 'max_instances': args.max_instances}
    write_atomic(RUNTIME / 'policy.json', json.dumps(policy, indent=2) + '\n', mode=0o644)
    override = RUNTIME / 'compose.mounts.json'
    write_atomic(override, json.dumps({'services': {'arr-controller': {'volumes': volumes}}}, indent=2) + '\n')
    compose = base + ['-f', str(override)]
    run(*compose, 'config', '--quiet')
    if args.prepare_only:
        print('Managed Arr policy and media mounts prepared.')
        return
    if not args.skip_build:
        run(*compose, 'build', 'api', 'arr-controller')
    run(*compose, 'up', '-d', '--no-deps', '--force-recreate', 'arr-controller')
    deadline = time.monotonic() + 25
    while True:
        ready = subprocess.run(compose + ['exec', '-T', '--user', '10001:10001', 'arr-controller', 'python3', '-c', "from controller import UnixHTTP; client=UnixHTTP('/run/grimoire-arr/controller.sock', 2); client.request('GET', '/v1/health'); assert client.getresponse().status == 200"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        if not ready.returncode:
            break
        if time.monotonic() > deadline:
            raise SystemExit('Controller did not become ready. Check its private logs. API settings were preserved.')
        time.sleep(1)
    original = ENV.read_text()
    lines = [line for line in original.splitlines() if not line.startswith('GRIMOIRE_ARR_CONTROLLER_SOCKET=')]
    write_atomic(ENV, '\n'.join(lines) + '\nGRIMOIRE_ARR_CONTROLLER_SOCKET=/run/grimoire-arr/controller.sock\n')
    try:
        run(*base, 'up', '-d', '--no-deps', 'api')
    except subprocess.CalledProcessError:
        write_atomic(ENV, original)
        run(*base, 'up', '-d', '--no-deps', 'api')
        raise
    print('Managed Arr enabled. Open an owned source → Optional Arr connections → Create managed Arr instance.')


if __name__ == '__main__':
    main()
