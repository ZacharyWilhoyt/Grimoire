#!/usr/bin/env python3
"""Provision disposable Arr instances on an isolated Docker network."""
from contextlib import closing
import http.client
import json
import os
from pathlib import Path
import re
import secrets
import subprocess
import tempfile
import time
import uuid
from urllib.parse import urlencode

IMAGE = os.environ.get('GRIMOIRE_ARR_TEST_IMAGE', 'grimoire-arr-controller-test')
APPS = {'sonarr': 8989, 'radarr': 7878, 'lidarr': 8686}
CLIENT = """
import sys
from controller import UnixHTTP
request = UnixHTTP('/run/grimoire-arr/controller.sock', timeout=6)
request.request(sys.argv[1], sys.argv[2], body=sys.stdin.buffer.read(), headers={'Content-Type':'application/json'})
response = request.getresponse()
print(response.status)
sys.stdout.buffer.flush()
print(response.read().decode())
"""


def docker(*args, data=None, check=True):
    result = subprocess.run(['docker', *args], input=data, capture_output=True, text=True, timeout=90)
    if check and result.returncode:
        raise RuntimeError('Docker test command failed: ' + args[0])
    return result.stdout.strip()


def wait(work, timeout=240):
    deadline = time.monotonic() + timeout
    while True:
        try:
            return work()
        except (AssertionError, OSError, RuntimeError, http.client.HTTPException):
            if time.monotonic() > deadline:
                raise
            time.sleep(2)


def main():
    suffix = uuid.uuid4().hex[:10]
    name = 'grimoire-arr-test-' + suffix
    state = name + '-state'
    media_volume = name + '-media'
    source = str(uuid.uuid4())
    instances = [(str(uuid.uuid4()), kind, write, secrets.token_hex(32)) for kind, write in
                 [('sonarr', False), ('sonarr', True), ('radarr', False), ('lidarr', False)]]
    password = secrets.token_urlsafe(24)

    def request(method, route, payload=None, expected=200):
        out = docker('exec', '-i', '-u', '10001:10001', name, 'python3', '-c', CLIENT, method, route,
                     data=json.dumps(payload) if payload is not None else '')
        status, body = out.split('\n', 1)
        value = json.loads(body)
        if int(status) != expected:
            raise RuntimeError(f'Controller HTTP {status}: {value.get("error", "unexpected response")}')
        return value

    def status(instance):
        query = urlencode([('source', source), ('id', instance)])
        data = request('GET', '/v1/instances?' + query)
        return next(item for item in data['instances'] if item['id'] == instance)

    def expect(instance, wanted):
        data = status(instance)
        if data['status'] == 'failed':
            raise SystemExit(f"{data['kind']} setup failed: {data['stage']} — {data['error']}")
        assert data['status'] == wanted, data['status']
        return data

    try:
        with tempfile.TemporaryDirectory(prefix='grimoire-arr-fixture-') as temp:
            directory = Path(temp)
            media = directory / 'media'
            media.mkdir(mode=0o777)
            media.chmod(0o777)
            (media / 'nested').mkdir(mode=0o777)
            (media / 'nested').chmod(0o777)
            (media / 'escape').symlink_to('/tmp')
            policy = directory / 'policy.json'
            policy.write_text(json.dumps({'images': {kind: f'lscr.io/linuxserver/{kind}:latest' for kind in APPS},
                                          'media_roots': [{'container_path': '/fixture', 'volume': media_volume}],
                                          'network': name, 'state_volume': state, 'max_instances': 4}))
            policy.chmod(0o644)
            docker('network', 'create', name)
            docker('volume', 'create', state)
            docker('volume', 'create', '--driver', 'local', '--opt', 'type=none', '--opt', 'o=bind', '--opt', 'device=' + str(media), media_volume)
            docker('run', '-d', '--name', name, '--network', name, '--read-only', '--cap-drop', 'ALL',
                   '--cap-add', 'CHOWN', '--cap-add', 'SETUID', '--cap-add', 'SETGID', '--security-opt', 'no-new-privileges',
                   '-v', '/var/run/docker.sock:/var/run/docker.sock:ro', '-v', state + ':/state',
                   '--tmpfs', '/run/grimoire-arr', '-v', str(policy) + ':/etc/grimoire-arr/policy.json:ro',
                   '-v', str(media) + ':/fixture:ro', IMAGE)
            wait(lambda: request('GET', '/v1/health'), timeout=25)
            for instance, kind, write, key in instances:
                payload = {'id': instance, 'source_id': source, 'kind': kind, 'media_path': '/fixture/nested' if write else '/fixture',
                           'api_key': key, 'username': 'fixture_admin', 'password': password, 'writable_media': write}
                request('POST', '/v1/instances', payload | {'media_path': '/fixture/escape'}, expected=400)
                request('POST', '/v1/instances', payload)
            for instance, kind, write, key in instances:
                info = wait(lambda: expect(instance, 'running'))
                name_in_docker = 'grimoire-arr-' + instance
                inspected = json.loads(docker('inspect', name_in_docker))[0]
                mount = next(mount for mount in inspected['Mounts'] if mount['Destination'] == '/media')
                assert mount['RW'] == write
                assert all(binding['HostIp'] == '127.0.0.1' for values in inspected['NetworkSettings']['Ports'].values() for binding in values)
                def authenticated_status():
                    with closing(http.client.HTTPConnection('127.0.0.1', info['local_port'], timeout=5)) as conn:
                        conn.request('GET', '/api', headers={'X-Api-Key': key})
                        response = conn.getresponse()
                        assert response.status == 200
                        api = json.loads(response.read())['current']
                        conn.request('GET', f'/api/{api}/system/status', headers={'X-Api-Key': key})
                        response = conn.getresponse()
                        assert response.status == 200
                        assert json.loads(response.read())['appName'].lower() == kind
                        conn.request('GET', f'/api/{api}/system/status')
                        response = conn.getresponse()
                        assert response.status in (401, 403)
                        response.read()
                        conn.request('POST', '/login?returnUrl=%2F', body=urlencode({
                            'username': 'fixture_admin', 'password': password, 'rememberMe': 'false',
                        }), headers={'Content-Type': 'application/x-www-form-urlencoded'})
                        response = conn.getresponse()
                        assert response.status in (302, 303), 'Arr forms login did not redirect'
                        cookie = '; '.join(value.split(';', 1)[0] for key, value in response.getheaders() if key.lower() == 'set-cookie')
                        response.read()
                        assert cookie, 'Arr forms login did not set a session cookie'
                        conn.request('GET', '/', headers={'Cookie': cookie})
                        response = conn.getresponse()
                        assert response.status == 200, 'Arr UI did not accept the configured login'
                        response.read()
                wait(authenticated_status, timeout=45)
                print(f'{kind} {info["version"]}: authenticated API and UI, correct media access.', flush=True)
            instance = instances[0][0]
            request('POST', f'/v1/instances/{instance}/stop', {'source_id': 'wrong-source'}, expected=400)
            request('POST', f'/v1/instances/{instance}/stop', {'source_id': source})
            wait(lambda: expect(instance, 'stopped'), timeout=30)
            request('POST', f'/v1/instances/{instance}/start', {'source_id': source})
            wait(lambda: expect(instance, 'running'), timeout=30)
            request('POST', f'/v1/instances/{instance}/restart', {'source_id': source})
            restarted = wait(lambda: expect(instance, 'running'), timeout=30)
            inspected = json.loads(docker('inspect', 'grimoire-arr-' + instance))[0]
            assert restarted['local_port'] == int(inspected['NetworkSettings']['Ports']['8989/tcp'][0]['HostPort'])
            request('POST', f'/v1/instances/{instance}/remove', {'source_id': source})
            wait(lambda: expect(instance, 'removed'), timeout=30)
            docker('exec', '-u', '10001:10001', name, 'test', '-f', f'/state/instances/{instance}/config/config.xml')
            docker('restart', name)
            wait(lambda: expect(instances[1][0], 'running'), timeout=30)
            print('Verified multiple instances, source isolation, stop/start/restart/removal, retained settings and controller restart.')
    except BaseException:
        print(docker('logs', '--tail', '16', name, check=False))
        for instance, _, _, key in instances:
            out = docker('logs', '--tail', '12', 'grimoire-arr-' + instance, check=False)
            print(re.sub(r'[A-Fa-f0-9]{32,}', '[redacted]', out.replace(password, '[redacted]').replace(key, '[redacted]')))
        raise
    finally:
        docker('rm', '-f', name, check=False)
        for instance, _, _, _ in instances:
            docker('rm', '-f', 'grimoire-arr-' + instance, check=False)
        docker('volume', 'rm', state, check=False)
        docker('volume', 'rm', media_volume, check=False)
        docker('network', 'rm', name, check=False)


if __name__ == '__main__':
    main()
