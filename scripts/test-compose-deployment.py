#!/usr/bin/env python3
"""CI-only disposable Compose smoke test; never points at a user's deployment."""
import http.cookiejar
import json
import os
import re
from pathlib import Path
import ssl
import subprocess
import tempfile
import time
import urllib.request

if os.environ.get('GITHUB_ACTIONS') != 'true':
    raise SystemExit('This disposable deployment test runs only in GitHub Actions.')
import pexpect

COMPOSE = ['docker', 'compose', '-p', 'grimoire-security-ci', '--env-file', 'deploy/.env', '-f', 'deploy/compose.yaml']
PASSWORD = 'disposable grimoire deployment test passphrase'


def compose(*args):
    return subprocess.run(COMPOSE + list(args), check=True, capture_output=True).stdout


def wait(work):
    deadline = time.monotonic() + 90
    while True:
        try:
            return work()
        except (OSError, subprocess.CalledProcessError, AssertionError):
            if time.monotonic() > deadline:
                raise
            time.sleep(.5)


phase = 'starting the database'
try:
    compose('up', '-d', 'postgres')
    phase = 'bootstrapping the disposable administrator'
    child = pexpect.spawn(COMPOSE[0], COMPOSE[1:] + ['run', '--rm', 'api', 'bootstrap-admin', 'ci_admin'], encoding='utf-8', timeout=120)
    child.expect('Password:'); child.sendline(PASSWORD)
    child.expect('Confirm password:'); child.sendline(PASSWORD)
    child.expect(pexpect.EOF); child.close()
    assert child.exitstatus == 0, 'Disposable administrator bootstrap failed'
    phase = 'starting application and proxy services'
    compose('up', '-d')
    phase = 'reading the local CA as the unprivileged proxy user'
    root = wait(lambda: compose('exec', '--user', '10001:10001', '-T', 'caddy', 'cat', '/data/caddy/pki/authorities/local/root.crt'))
    context = ssl.create_default_context(cadata=root.decode())
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), urllib.request.HTTPSHandler(context=context), urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))
    csrf = ''

    def request(path, data=None, method=None, headers=None):
        all_headers = {'Origin': 'https://localhost', **(headers or {})}
        if data is not None:
            all_headers.update({'Content-Type': 'application/json', 'X-CSRF-Token': csrf})
        req = urllib.request.Request('https://localhost' + path, data=json.dumps(data).encode() if data is not None else None, method=method, headers=all_headers)
        with opener.open(req, timeout=4) as response:
            return response.read()

    phase = 'HTTPS readiness through the proxy'
    wait(lambda: request('/api/health'))
    assert b'<div id="root"' in request('/')
    phase = 'HTTPS login and administrator APIs'
    session = json.loads(request('/api/auth/login', {'username': 'ci_admin', 'password': PASSWORD}))
    csrf = session['csrf_token']
    assert session['user']['is_admin']
    tls = json.loads(request('/api/admin/security/tls'))
    assert tls['managed'] and tls['online'] and tls['settings'] is None
    policy = json.loads(request('/api/admin/security/ip'))
    request('/api/admin/security/ip', {'expected_revision': policy['revision'], 'policy': {'default_action': 'deny', 'allow': [policy['current_ip']], 'deny': ['0.0.0.0/0', '::/0']}}, method='PUT')
    assert b'<div id="root"' in request('/', headers={'X-Grimoire-Client-IP': '198.51.100.9', 'X-Grimoire-Proxy-Token': 'forged'})
    config = json.loads(compose('config', '--format', 'json'))
    assert not config['services']['api'].get('ports') and not config['services']['postgres'].get('ports')
    assert config['services']['postgres']['network_mode'] == 'none'
    phase = 'restart and persistent session/IP rules'
    compose('restart', 'api', 'caddy')
    me = wait(lambda: json.loads(request('/api/auth/me')))
    assert me['user']['username'] == 'ci_admin'
    assert json.loads(request('/api/admin/security/ip'))['policy']['default_action'] == 'deny'
    print('Compose smoke passed: bootstrap, HTTPS frontend/API, private Caddy control, IP enforcement and restart/session persistence.')
except Exception as error:
    def diagnostic(text):
        text = re.sub(r'[a-fA-F0-9]{64}', '[redacted hex]', text).replace(PASSWORD, '[test password]')
        text = re.sub(r'-----BEGIN [^-]+-----.*?-----END [^-]+-----', '[redacted PEM]', text, flags=re.S)
        text = text.replace('%', '%25').replace('\r', '%0D').replace('\n', '%0A')
        print('::error title=Disposable deployment diagnostics::' + text[-10000:])
    diagnostic('Phase: ' + phase + '; ' + str(error))
    if isinstance(error, subprocess.CalledProcessError) and error.stderr:
        diagnostic(error.stderr.decode(errors='replace')[-3000:])
    for service in ('api', 'caddy', 'postgres'):
        try:
            diagnostic(service + ': ' + compose('logs', '--tail', '12', service).decode(errors='replace')[-4000:])
        except subprocess.CalledProcessError:
            pass
    raise
finally:
    # Only this CI-only project is disposable. Never use this against a real deployment.
    compose('down', '-v', '--remove-orphans')
