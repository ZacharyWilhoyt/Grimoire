#!/usr/bin/env python3
"""Download a pinned official Linux amd64 Caddy binary for disposable tests."""
import hashlib
import io
import os
from pathlib import Path
import tarfile
import urllib.request

VERSION = '2.11.8'
SHA512 = 'c863699eee84c6b410573de2ec67af81d07eb054f9020bb98f3c6cb256fea4be2fd1acfbd39110c649c3338282593bba3d3ed6424d91b7f5675e8b877a8999e9'
root = Path('/tmp/grimoire-caddy-test')
root.mkdir(mode=0o700,exist_ok=True)
if root.is_symlink() or root.stat().st_uid != os.getuid():
    raise SystemExit('Refusing an untrusted temporary tools directory.')
url = f'https://github.com/caddyserver/caddy/releases/download/v{VERSION}/caddy_{VERSION}_linux_amd64.tar.gz'
archive = urllib.request.urlopen(url, timeout=60).read()
if hashlib.sha512(archive).hexdigest() != SHA512:
    raise SystemExit('Official Caddy artifact checksum mismatch; refusing to run it.')
with tarfile.open(fileobj=io.BytesIO(archive)) as bundle:
    binary = bundle.extractfile('caddy').read()
path = root / 'caddy'
path.write_bytes(binary)
path.chmod(0o755)
print(f'Verified Caddy {VERSION}: {path}')
