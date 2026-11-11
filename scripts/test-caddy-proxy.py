#!/usr/bin/env python3
"""Loopback-only Caddy smoke test. Never requests public ACME certificates."""
import base64
import hashlib
import http.client
import http.server
import json
import os
from pathlib import Path
import socket
import ssl
import subprocess
import tempfile
import threading
import time

TOKEN = '0' * 64
BINARY = os.environ['GRIMOIRE_TEST_CADDY_BIN']


def wait_for(work):
    deadline = time.monotonic() + 15
    while True:
        try:
            return work()
        except (OSError, ssl.SSLError, AssertionError, http.client.HTTPException):
            if time.monotonic() >= deadline:
                raise
            time.sleep(.1)


def free_port():
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0))
        return listener.getsockname()[1]


class Backend(http.server.BaseHTTPRequestHandler):
    protocol_version = 'HTTP/1.1'

    def log_message(self, *_):
        pass

    def do_POST(self):
        assert self.path == '/reload'
        assert self.headers.get('X-Grimoire-Proxy-Token') == TOKEN
        self.server.on_reload()
        content = b'Applied during an active proxied request'
        self.send_response(200)
        self.send_header('Content-Length', str(len(content)))
        self.end_headers()
        self.wfile.write(content)

    def do_GET(self):
        assert self.headers.get('X-Grimoire-Client-IP') == '127.0.0.1'
        assert self.headers.get('X-Grimoire-Proxy-Token') == TOKEN
        if self.path == '/ws':
            accept = base64.b64encode(hashlib.sha1((self.headers['Sec-WebSocket-Key'] + '258EAFA5-E914-47DA-95CA-C5AB0DC85B11').encode()).digest()).decode()
            self.send_response(101)
            self.send_header('Upgrade', 'websocket')
            self.send_header('Connection', 'Upgrade')
            self.send_header('Sec-WebSocket-Accept', accept)
            self.end_headers()
            self.wfile.write(b'\x81\x02OK')
            self.wfile.flush()
            self.close_connection = True
            return
        if self.path == '/range':
            assert self.headers.get('Range') == 'bytes=2-4'
            self.send_response(206)
            self.send_header('Content-Range', 'bytes 2-4/10')
            content = b'234'
        else:
            self.send_response(200)
            content = b'Grimoire proxy smoke'
        self.send_header('Content-Length', str(len(content)))
        self.send_header('Cache-Control', 'private, no-store')
        self.end_headers()
        self.wfile.write(content)


with tempfile.TemporaryDirectory(prefix='grimoire-caddy-smoke-') as temp:
    root = Path(temp)
    backend = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Backend)
    threading.Thread(target=backend.serve_forever, daemon=True).start()
    http_port, https_port = free_port(), free_port()
    control, storage = root / 'admin.sock', root / 'storage'
    file = root / 'Caddyfile'
    common = f'''{{
 admin unix/{control}
 storage file_system {{
  root {storage}
 }}
 skip_install_trust
 http_port {http_port}
 https_port {https_port}
 default_bind 127.0.0.1
 servers {{
  strict_sni_host
  0rtt off
 }}
 AUTOMATION
}}
https://localhost:{https_port} {{
 TLS
 reverse_proxy 127.0.0.1:{backend.server_port} {{
  header_up X-Grimoire-Client-IP {{http.request.remote.host}}
  header_up X-Grimoire-Proxy-Token {{$GRIMOIRE_PROXY_TOKEN}}
  header_up -Forwarded
 }}
}}
'''
    file.write_text(common.replace('AUTOMATION', '').replace('TLS', 'tls internal'))
    environment = dict(os.environ, XDG_CONFIG_HOME=str(root / 'config'), GRIMOIRE_PROXY_TOKEN=TOKEN)
    log = (root / 'process.log').open('wb')
    process = None

    def start(resume=False):
        args = [BINARY, 'run', '--config', str(file), '--adapter', 'caddyfile']
        if resume:
            args.append('--resume')
        return subprocess.Popen(args, env=environment, stdout=log, stderr=log)

    def api(path, data=None):
        connection = http.client.HTTPConnection('localhost', timeout=3)
        def connect():
            connection.sock = socket.socket(socket.AF_UNIX)
            connection.sock.settimeout(3)
            connection.sock.connect(str(control))
        connection.connect = connect
        connection.request('POST' if data is not None else 'GET', path, body=data, headers={'Content-Type': 'application/json', 'Cache-Control':'must-revalidate'})
        response = connection.getresponse()
        status, body = response.status, response.read()
        connection.close()
        return status, body

    def request(path='/', method='GET'):
        context = ssl.create_default_context(cafile=str(storage / 'pki/authorities/local/root.crt'))
        connection = http.client.HTTPSConnection('localhost', https_port, timeout=3, context=context)
        connection.request(method, path, headers={'Range': 'bytes=2-4', 'X-Grimoire-Client-IP': '203.0.113.99', 'X-Grimoire-Proxy-Token': 'forged'})
        response = connection.getresponse()
        result = response.status, response.getheader('Content-Range'), response.read()
        connection.close()
        return result

    try:
        process = start()
        wait_for(lambda: request())
        assert request('/range') == (206, 'bytes 2-4/10', b'234')
        status, current = api('/config/')
        assert status == 200 and json.loads(current)['admin']['listen'] == f'unix/{control}'
        servers = json.loads(current)['apps']['http']['servers'].values()
        assert all(server['allow_0rtt'] is False for server in servers if server.get('routes'))
        cert = storage / 'certificates/local/localhost/localhost.crt'
        key = cert.with_suffix('.key')
        before = cert.read_bytes(), key.read_bytes()
        # Disable automation explicitly, reusing the already issued certificate/key.
        file.write_text(common.replace('AUTOMATION', 'auto_https disable_certs').replace('TLS', f'tls {cert} {key}'))
        adapted = subprocess.run([BINARY, 'adapt', '--config', str(file), '--adapter', 'caddyfile'], env=environment, capture_output=True, check=True)
        manual_config = json.loads(adapted.stdout)
        manual_config['@id'] = 'grimoire-smoke-manual'
        manual_bytes = json.dumps(manual_config).encode()
        def reload_manual():
            assert api('/load', manual_bytes)[0] == 200
            # Repopulate explicit file certificates after old managed-cache cleanup.
            assert api('/load', manual_bytes)[0] == 200
        backend.on_reload = reload_manual
        assert request('/reload', method='POST')[0] == 200
        assert json.loads(api('/config/')[1])['@id'] == 'grimoire-smoke-manual'
        assert request()[0] == 200
        assert before == (cert.read_bytes(), key.read_bytes())
        invalid_config = json.loads(adapted.stdout)
        server = next(iter(invalid_config['apps']['http']['servers'].values()))
        server['routes'][0]['handle'] = [{'handler': 'not_a_real_module'}]
        invalid = json.dumps(invalid_config).encode()
        assert api('/load', invalid)[0] == 400
        assert request()[0] == 200
        # Check HTTP redirects without following them or relying on browser exceptions.
        connection = http.client.HTTPConnection('127.0.0.1', http_port, timeout=3)
        connection.request('GET', '/', headers={'Host': f'localhost:{http_port}'})
        response = connection.getresponse()
        assert response.status == 308 and response.getheader('Location').startswith('https://localhost'), (response.status, response.getheaders())
        response.read(); connection.close()
        context = ssl.create_default_context(cafile=str(storage / 'pki/authorities/local/root.crt'))
        with socket.create_connection(('127.0.0.1', https_port), timeout=3) as plain:
            with context.wrap_socket(plain, server_hostname='localhost') as stream:
                key64 = base64.b64encode(b'0123456789abcdef').decode()
                stream.sendall(f'GET /ws HTTP/1.1\r\nHost: localhost:{https_port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key64}\r\nSec-WebSocket-Version: 13\r\n\r\n'.encode())
                received = b''
                while b'\r\n\r\n' not in received:
                    received += stream.recv(4096)
                assert b'101 Switching Protocols' in received
                while b'\x81\x02OK' not in received:
                    received += stream.recv(4096)
        process.terminate(); process.wait(timeout=5)
        process = start(resume=True)
        wait_for(lambda: request())
        assert before == (cert.read_bytes(), key.read_bytes())
        # Operator reset must recover local HTTPS after a persisted manual site.
        file.write_text(common.replace('AUTOMATION', 'auto_https ignore_loaded_certs').replace('TLS', 'tls internal'))
        recovered = subprocess.run([BINARY, 'adapt', '--config', str(file), '--adapter', 'caddyfile'], env=environment, capture_output=True, check=True)
        recovery_config = json.loads(recovered.stdout)
        recovery_config['@id'] = 'grimoire-smoke-recovered'
        recovery_bytes = json.dumps(recovery_config).encode()
        assert api('/load', recovery_bytes)[0] == 200
        wait_for(lambda: request())
        assert before == (cert.read_bytes(), key.read_bytes())
        # Forced reset of the same configuration must remain usable too.
        assert api('/load', recovery_bytes)[0] == 200
        assert request()[0] == 200
        # An unreachable public issuer must not remove local recovery, including
        # after Caddy resumes the saved combined configuration. Never contact ACME.
        public = f"""
https://video.example.com:{https_port} {{
 tls {{
  issuer acme {{
   dir http://127.0.0.1:9/directory
  }}
 }}
 respond "public fixture"
}}
"""
        file.write_text(common.replace('AUTOMATION', 'auto_https ignore_loaded_certs').replace('TLS', 'tls internal') + public)
        combined = subprocess.run([BINARY, 'adapt', '--config', str(file), '--adapter', 'caddyfile'], env=environment, capture_output=True, check=True)
        assert api('/load', combined.stdout)[0] == 200
        wait_for(lambda: request())
        process.terminate(); process.wait(timeout=5)
        process = start(resume=True)
        wait_for(lambda: request())
        assert request('/range') == (206, 'bytes 2-4/10', b'234')
        # Manual public certificates must not globally disable the local issuer.
        public_cert, public_key = root / 'public.crt', root / 'public.key'
        subprocess.run(['openssl', 'req', '-x509', '-newkey', 'ec', '-pkeyopt', 'ec_paramgen_curve:P-256', '-nodes', '-subj', '/CN=video.example.com', '-addext', 'subjectAltName=DNS:video.example.com', '-days', '1', '-keyout', str(public_key), '-out', str(public_cert)], check=True, capture_output=True)
        public_manual = f"""
https://video.example.com:{https_port} {{
 tls {public_cert} {public_key}
 respond "manual public fixture"
}}
"""
        file.write_text(common.replace('AUTOMATION', '').replace('TLS', 'tls internal') + public_manual)
        adapted_manual = subprocess.run([BINARY, 'adapt', '--config', str(file), '--adapter', 'caddyfile'], env=environment, capture_output=True, check=True)
        config_manual = json.loads(adapted_manual.stdout)
        for server in config_manual['apps']['http']['servers'].values():
            server.setdefault('automatic_https', {})['skip_certificates'] = ['video.example.com']
        encoded_manual = json.dumps(config_manual).encode()
        assert api('/load', encoded_manual)[0] == 200
        assert api('/load', encoded_manual)[0] == 200
        wait_for(lambda: request())
        process.terminate(); process.wait(timeout=5)
        process = start(resume=True)
        wait_for(lambda: request())
        assert before == (cert.read_bytes(), key.read_bytes())
        print('Caddy smoke passed: HTTPS, forwarding-header replacement, ranges, redirects, WebSockets, disabled automation, rejected reload, restart persistence, localhost TLS recovery and unavailable public issuer.')
    except Exception:
        log.flush()
        print((root / 'process.log').read_text()[-9000:])
        raise
    finally:
        if process and process.poll() is None:
            process.terminate(); process.wait(timeout=5)
        backend.shutdown()
        log.close()
