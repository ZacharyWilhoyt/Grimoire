#!/usr/bin/env python3
"""Private, fixed-template Arr controller. Never accepts Docker configuration."""
import concurrent.futures
import http.client
import json
import os
from pathlib import Path
import re
import socket
import socketserver
import threading
import time
from urllib.parse import parse_qs, urlencode, urlsplit
import uuid
import xml.etree.ElementTree as ET
from http.server import BaseHTTPRequestHandler

APPS = {'sonarr': 8989, 'radarr': 7878, 'lidarr': 8686}
STATE = Path('/state')
SOCKET = Path('/run/grimoire-arr/controller.sock')
LABEL = 'org.grimoire.arr'


class Failure(Exception):
    pass


def identifier(value):
    try:
        return str(uuid.UUID(value))
    except (ValueError, TypeError, AttributeError):
        raise Failure('Invalid instance identifier.') from None


def source_identifier(value):
    if not isinstance(value, str) or not re.fullmatch(r'[A-Za-z0-9_-]{1,128}', value):
        raise Failure('Invalid source identifier.')
    return value


def private_json(path, value):
    temporary = path.with_suffix('.tmp')
    with temporary.open('w') as stream:
        json.dump(value, stream)
        stream.flush()
        os.fsync(stream.fileno())
    os.replace(temporary, path)


class UnixHTTP(http.client.HTTPConnection):
    def __init__(self, path, timeout=30):
        super().__init__('localhost', timeout=timeout)
        self.path = path

    def connect(self):
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.sock.settimeout(self.timeout)
        self.sock.connect(self.path)


class Engine:
    def request(self, method, path, body=None, allow_missing=False, pull=False):
        connection = UnixHTTP('/var/run/docker.sock', timeout=120 if pull else 35)
        try:
            connection.request(method, '/v1.47' + path,
                               body=json.dumps(body).encode() if body is not None else None,
                               headers={'Content-Type': 'application/json'})
            response = connection.getresponse()
            if allow_missing and response.status == 404:
                response.read(65536)
                return None
            if response.status == 304 and method == 'POST' and path.split('?')[0].endswith(('/start', '/stop')):
                return {}
            if not 200 <= response.status < 300:
                raise Failure('Docker operation failed. Inspect private operator logs and available resources.')
            if pull:
                while line := response.readline(65536):
                    if len(line) >= 65536:
                        raise Failure('Image pull response exceeded its limit.')
                    try:
                        if json.loads(line).get('error'):
                            raise Failure('Approved image could not be pulled.')
                    except ValueError:
                        raise Failure('Invalid image pull response.') from None
                return {}
            data = response.read(4 * 1024 * 1024 + 1)
            if len(data) > 4 * 1024 * 1024:
                raise Failure('Docker response exceeded its limit.')
            return json.loads(data) if data else {}
        finally:
            connection.close()


def media_mount(policy, requested):
    if not isinstance(requested, str) or not requested.startswith('/'):
        raise Failure('Select an absolute registered media directory.')
    path = Path(requested).resolve(strict=True)
    if not path.is_dir():
        raise Failure('The selected media directory is unavailable.')
    for root in policy['media_roots']:
        container_root = Path(root['container_path']).resolve(strict=True)
        if path.is_relative_to(container_root):
            # Docker resolves a volume subpath beneath a fixed operator-approved
            # root. It must never interpret a client-provided host bind path.
            relative = path.relative_to(container_root)
            options = {'NoCopy': True}
            if relative.parts:
                options['Subpath'] = relative.as_posix()
            return {'Type': 'volume', 'Source': root['volume'], 'Target': '/media',
                    'ReadOnly': True, 'VolumeOptions': options}
    raise Failure('This directory is outside operator-approved media mounts.')


class Controller:
    def __init__(self, policy, engine=None):
        self.policy = policy
        self.engine = engine or Engine()
        self.lock = threading.RLock()
        self.pool = concurrent.futures.ThreadPoolExecutor(max_workers=2)
        self.running = set()
        self.installation = (STATE / 'installation').read_text().strip() if (STATE / 'installation').exists() else str(uuid.uuid4())
        (STATE / 'installation').write_text(self.installation)

    def path(self, instance):
        return STATE / 'instances' / identifier(instance)

    def record(self, instance, source):
        path = self.path(instance) / 'instance.json'
        if not path.is_file():
            raise Failure('Managed instance not found.')
        record = json.loads(path.read_text())
        if record['source_id'] != source:
            raise Failure('Managed instance not found.')
        return record

    def save(self, record):
        private_json(self.path(record['id']) / 'instance.json', record)

    def inspect(self, record):
        data = self.engine.request('GET', '/containers/' + record['container'] + '/json', allow_missing=True)
        if data is not None:
            labels = data.get('Config', {}).get('Labels', {})
            expected = {LABEL: self.installation, LABEL + '.id': record['id'], LABEL + '.source': record['source_id']}
            if any(labels.get(k) != v for k, v in expected.items()):
                raise Failure('Container ownership does not match this controller.')
        return data

    def summary(self, record):
        status = record['status']
        local_port = None
        if status == 'ready':
            data = self.inspect(record)
            status = 'missing' if data is None else 'running' if data['State']['Running'] else 'stopped'
            if status == 'running':
                bindings = data['NetworkSettings']['Ports'].get(str(APPS[record['kind']]) + '/tcp') or []
                local_port = int(bindings[0]['HostPort']) if bindings else None
        fields = ('id', 'source_id', 'kind', 'stage', 'error', 'version', 'writable_media')
        return {key: record.get(key) for key in fields} | {'status': status, 'local_port': local_port}

    def list(self, source, instances):
        with self.lock:
            records = []
            for instance in instances:
                path = self.path(instance) / 'instance.json'
                if path.is_file():
                    record = json.loads(path.read_text())
                    if record['source_id'] == source:
                        records.append(record)
        return [self.summary(record) for record in records]

    def create(self, body):
        if set(body) != {'id', 'source_id', 'kind', 'media_path', 'api_key', 'username', 'password', 'writable_media'}:
            raise Failure('Unsupported instance fields.')
        instance, source = identifier(body['id']), source_identifier(body['source_id'])
        if body['kind'] not in APPS or not re.fullmatch(r'[a-f0-9]{64}', body['api_key']):
            raise Failure('Invalid approved app or credential format.')
        if not isinstance(body['writable_media'], bool):
            raise Failure('Write access must be explicitly selected.')
        if not isinstance(body['username'], str) or not re.fullmatch(r'[A-Za-z0-9_-]{3,64}', body['username']):
            raise Failure('Use a 3–64 character Arr username.')
        if not isinstance(body['password'], str) or not 12 <= len(body['password']) <= 128:
            raise Failure('Use a 12–128 character Arr password.')
        mount = media_mount(self.policy, body['media_path'])
        mount['ReadOnly'] = not body['writable_media']
        with self.lock:
            folder = self.path(instance)
            if (folder / 'instance.json').exists():
                old = self.record(instance, source)
                if old['kind'] != body['kind'] or old['mount'] != mount:
                    raise Failure('Existing instance mappings cannot be replaced.')
                if old['status'] not in ('failed', 'missing'):
                    return self.summary(old)
                old['password'] = body['password']
                old['username'] = body['username']
                old['api_key'] = body['api_key']
                old['status'] = 'provisioning'
                old['error'] = None
                self.save(old)
                self.submit(old, 'provision')
                return self.summary(old)
            if len(list((STATE / 'instances').glob('*/instance.json'))) >= self.policy.get('max_instances', 12):
                active = sum(json.loads(p.read_text())['status'] != 'removed' for p in (STATE / 'instances').glob('*/instance.json'))
                if active >= self.policy.get('max_instances', 12):
                    raise Failure('The operator-managed instance limit has been reached.')
            (folder / 'config').mkdir(parents=True, mode=0o700)
            config = ET.Element('Config')
            for key, value in {'ApiKey': body['api_key'], 'BindAddress': '*', 'Port': str(APPS[body['kind']]), 'EnableSsl': 'False', 'LaunchBrowser': 'False', 'AuthenticationMethod': 'Forms', 'AuthenticationRequired': 'Enabled', 'UpdateMechanism': 'Docker'}.items():
                ET.SubElement(config, key).text = value
            ET.ElementTree(config).write(folder / 'config/config.xml', encoding='utf-8', xml_declaration=True)
            record = {k: body[k] for k in ('id', 'source_id', 'kind', 'username', 'password', 'api_key', 'writable_media')}
            record.update(id=instance, container='grimoire-arr-' + instance, mount=mount, status='provisioning', stage='Queued', error=None)
            self.save(record)
            self.submit(record, 'provision')
            return self.summary(record)

    def submit(self, record, action):
        if record['id'] in self.running:
            raise Failure('This instance already has an operation in progress.')
        self.running.add(record['id'])
        self.pool.submit(self.work, record['id'], record['source_id'], action)

    def action(self, instance, source, action):
        if action not in ('start', 'stop', 'restart', 'remove'):
            raise Failure('Unsupported instance action.')
        with self.lock:
            record = self.record(instance, source)
            if record['status'] == 'removed':
                return self.summary(record)
            if action != 'remove' and record['status'] != 'ready':
                raise Failure('Finish or retry setup before controlling this instance.')
            if instance in self.running:
                raise Failure('This instance already has an operation in progress.')
            record['pending_action'] = action
            record['status'] = 'removing' if action == 'remove' else 'changing'
            record['stage'] = action.capitalize() + ' requested'
            self.save(record)
            self.submit(record, action)
            return self.summary(record)

    def template(self, record, publish):
        kind, instance = record['kind'], record['id']
        port = str(APPS[kind]) + '/tcp'
        host = {'ReadonlyRootfs': True, 'CapDrop': ['ALL'], 'SecurityOpt': ['no-new-privileges:true'],
                'Memory': 1024 * 1024 * 1024, 'NanoCpus': 1000000000, 'PidsLimit': 256,
                'RestartPolicy': {'Name': 'unless-stopped'}, 'NetworkMode': self.policy['network'],
                'LogConfig': {'Type': 'json-file', 'Config': {'max-size': '10m', 'max-file': '2'}},
                'Tmpfs': {'/run': 'rw,exec,nosuid,nodev,size=32m,uid=10001,gid=10001', '/tmp': 'rw,nosuid,nodev,size=256m,uid=10001,gid=10001'},
                'Mounts': [record['mount'], {'Type': 'volume', 'Source': self.policy['state_volume'], 'Target': '/config', 'VolumeOptions': {'Subpath': 'instances/' + instance + '/config'}}]}
        if publish:
            host['PortBindings'] = {port: [{'HostIp': '127.0.0.1', 'HostPort': ''}]}
        return {'Image': self.policy['images'][kind], 'User': '10001:10001', 'Env': ['TZ=Etc/UTC'],
                'Labels': {LABEL: self.installation, LABEL + '.id': instance, LABEL + '.source': record['source_id']},
                'ExposedPorts': {port: {}}, 'HostConfig': host}

    def arr(self, record, method, path, body=None):
        key = record['api_key']
        connection = http.client.HTTPConnection(record['container'], APPS[record['kind']], timeout=8)
        try:
            connection.request(method, path, body=json.dumps(body) if body is not None else None,
                               headers={'X-Api-Key': key, 'Content-Type': 'application/json'})
            response = connection.getresponse()
            data = response.read(1024 * 1024 + 1)
            if not 200 <= response.status < 300 or len(data) > 1024 * 1024:
                raise Failure('Arr setup could not be completed. Check app compatibility and available resources.')
            return json.loads(data) if data else {}
        finally:
            connection.close()

    def set_stage(self, record, stage):
        with self.lock:
            record['stage'] = stage
            self.save(record)

    def wait_for_arr(self, record):
        deadline = time.monotonic() + 180
        while True:
            try:
                discovery = self.arr(record, 'GET', '/api')
                version = discovery.get('current')
                if version not in ('v1', 'v3', 'v5'):
                    raise Failure('Unsupported Arr API version.')
                prefix = '/api/' + version
                status = self.arr(record, 'GET', prefix + '/system/status')
                if str(status.get('appName', '')).lower() != record['kind']:
                    raise Failure('The container did not identify as the approved Arr app.')
                record['version'] = str(status.get('version', ''))[:80]
                return prefix
            except (OSError, ValueError, http.client.HTTPException, Failure):
                data = self.inspect(record)
                if data and data.get('RestartCount', 0) >= 3 and not data['State']['Running']:
                    raise Failure('Arr exited repeatedly during startup. Check private container logs.') from None
                if time.monotonic() > deadline:
                    raise Failure('Arr startup timed out. Check private container logs and retry setup.') from None
                time.sleep(2)

    def provision(self, record):
        self.set_stage(record, 'Preparing approved image')
        image = self.policy['images'][record['kind']]
        if self.engine.request('GET', '/images/' + image + '/json', allow_missing=True) is None:
            self.engine.request('POST', '/images/create?' + urlencode({'fromImage': image}), pull=True)
        data = self.inspect(record)
        if data is None:
            self.engine.request('POST', '/containers/create?' + urlencode({'name': record['container']}), self.template(record, False))
        self.engine.request('POST', '/containers/' + record['container'] + '/start')
        self.set_stage(record, 'Starting app and configuring login')
        prefix = self.wait_for_arr(record)
        host = self.arr(record, 'GET', prefix + '/config/host')
        host.update(username=record['username'], password=record['password'], passwordConfirmation=record['password'], authenticationMethod='forms', authenticationRequired='enabled')
        self.arr(record, 'PUT', prefix + '/config/host', host)
        self.set_stage(record, 'Publishing authenticated local setup UI')
        # Publish only after Arr has credentials; never expose its first-run wizard.
        self.engine.request('POST', '/containers/' + record['container'] + '/stop?t=10')
        self.engine.request('DELETE', '/containers/' + record['container'] + '?v=false')
        self.engine.request('POST', '/containers/create?' + urlencode({'name': record['container']}), self.template(record, True))
        self.engine.request('POST', '/containers/' + record['container'] + '/start')
        self.wait_for_arr(record)
        record.pop('password', None)
        record.pop('api_key', None)

    def work(self, instance, source, action):
        record = None
        try:
            with self.lock:
                record = self.record(instance, source)
            if action == 'provision':
                self.provision(record)
            else:
                data = self.inspect(record)
                if action == 'remove':
                    if data:
                        self.engine.request('POST', '/containers/' + record['container'] + '/stop?t=10')
                        self.engine.request('DELETE', '/containers/' + record['container'] + '?v=false')
                    record.pop('password', None)
                    record.pop('api_key', None)
                elif data is None:
                    raise Failure('Managed container is missing. Remove its connection and create a replacement.')
                else:
                    self.engine.request('POST', '/containers/' + record['container'] + '/' + action + ('?t=10' if action != 'start' else ''))
            record.update(status='removed' if action == 'remove' else 'ready', stage='Ready' if action != 'remove' else 'Removed; configuration retained', error=None)
        except Exception as error:
            # Raw Docker/Arr responses may contain secrets. Only fixed errors escape.
            if record is not None:
                record.pop('password', None)
                record.pop('api_key', None)
                record.update(status='failed', stage='Operation failed', error=str(error) if isinstance(error, Failure) else 'Operation failed. Check private operator logs and retry.')
        finally:
            with self.lock:
                if record is not None:
                    record.pop('pending_action', None)
                    self.save(record)
                self.running.discard(instance)

    def resume(self):
        for path in (STATE / 'instances').glob('*/instance.json'):
            record = json.loads(path.read_text())
            if record['status'] in ('provisioning', 'removing', 'changing'):
                if record['status'] == 'changing':
                    action = record.get('pending_action')
                    if action in ('start', 'stop', 'restart'):
                        self.submit(record, action)
                    else:
                        record.update(status='failed', error='Operation interrupted. Inspect the instance before retrying.')
                        self.save(record)
                else:
                    self.submit(record, 'remove' if record['status'] == 'removing' else 'provision')


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass  # No request bodies, API keys, passwords or raw downstream errors in logs.

    def handle_request(self):
        try:
            path = urlsplit(self.path)
            if self.command == 'GET' and path.path == '/v1/health':
                result = {'online': True}
            elif self.command == 'GET' and path.path == '/v1/instances':
                query = parse_qs(path.query, max_num_fields=13)
                source = source_identifier(query.get('source', [None])[0])
                instances = [identifier(value) for value in query.get('id', [])]
                result = {'instances': self.server.controller.list(source, instances)}
            elif self.command == 'POST' and path.path == '/v1/instances':
                length = int(self.headers.get('Content-Length', '0'))
                if not 0 < length <= 16384:
                    raise Failure('Invalid request size.')
                result = self.server.controller.create(json.loads(self.rfile.read(length)))
            elif self.command == 'POST' and (match := re.fullmatch(r'/v1/instances/([a-f0-9-]+)/([a-z]+)', path.path)):
                length = int(self.headers.get('Content-Length', '0'))
                if not 0 < length <= 1024:
                    raise Failure('Invalid request size.')
                body = json.loads(self.rfile.read(length))
                if set(body) != {'source_id'}:
                    raise Failure('Unsupported action fields.')
                result = self.server.controller.action(identifier(match[1]), source_identifier(body['source_id']), match[2])
            else:
                raise Failure('Unsupported controller operation.')
            payload, status = json.dumps(result).encode(), 200
        except Exception as error:
            payload, status = json.dumps({'error': str(error) if isinstance(error, Failure) else 'Controller operation failed.'}).encode(), 400
        self.send_response(status)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    do_GET = handle_request
    do_POST = handle_request


class Server(socketserver.ThreadingMixIn, socketserver.UnixStreamServer):
    daemon_threads = True


def main():
    os.umask(0o077)
    if os.getuid() == 0:
        for folder in (STATE, SOCKET.parent):
            folder.mkdir(parents=True, exist_ok=True)
            os.chown(folder, 0, 0)
            os.chmod(folder, 0o700)
            os.chown(folder, 10001, 10001)
        os.setgroups([os.stat('/var/run/docker.sock').st_gid])
        os.setgid(10001)
        os.setuid(10001)
    policy = json.loads(Path('/etc/grimoire-arr/policy.json').read_text())
    controller = Controller(policy)
    controller.engine.request('GET', '/version')
    (STATE / 'instances').mkdir(exist_ok=True, mode=0o700)
    SOCKET.unlink(missing_ok=True)
    with Server(str(SOCKET), Handler) as server:
        server.controller = controller
        controller.resume()
        print('Managed Arr controller ready on its private Unix socket.', flush=True)
        server.serve_forever()


if __name__ == '__main__':
    main()
