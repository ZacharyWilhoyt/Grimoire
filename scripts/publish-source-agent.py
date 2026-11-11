#!/usr/bin/env python3
"""Publish a docker image save OCI archive to Grimoire's read-only registry."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import tarfile
from tempfile import NamedTemporaryFile

INDEX_TYPES = {'application/vnd.oci.image.index.v1+json',
               'application/vnd.docker.distribution.manifest.list.v2+json'}


def publish(archive, root, tag):
    def validated_digest(descriptor):
        digest = descriptor['digest']
        if not re.fullmatch(r'sha256:[a-f0-9]{64}', digest):
            raise ValueError('Invalid OCI digest')
        return digest

    def transfer(descriptor, folder, parse=False):
        digest = validated_digest(descriptor)
        target = root / folder / digest
        target.parent.mkdir(mode=0o755, parents=True, exist_ok=True)
        with archive.extractfile(f'blobs/sha256/{digest[7:]}') as source:
            hasher = hashlib.sha256()
            with NamedTemporaryFile(dir=target.parent, delete=False) as output:
                temporary = Path(output.name)
                try:
                    while chunk := source.read(1024 * 1024):
                        hasher.update(chunk)
                        output.write(chunk)
                    if output.tell() != descriptor['size'] or hasher.hexdigest() != digest[7:]:
                        raise ValueError(f'OCI blob integrity check failed for {digest}')
                    output.close()
                    temporary.chmod(0o644)
                    temporary.replace(target)
                finally:
                    temporary.unlink(missing_ok=True)
        return json.loads(target.read_bytes()) if parse else target

    def select(descriptor):
        if descriptor['mediaType'] not in INDEX_TYPES:
            return descriptor
        digest = validated_digest(descriptor)
        with archive.extractfile(f'blobs/sha256/{digest[7:]}') as source:
            data = source.read(1_048_577)
        if len(data) > 1_048_576 or len(data) != descriptor['size'] or hashlib.sha256(data).hexdigest() != digest[7:]:
            raise ValueError('Invalid OCI image index')
        index = json.loads(data)
        return select(next(item for item in index['manifests']
                           if item.get('platform', {}).get('os') == 'linux'
                           and item['platform']['architecture'] == 'amd64'))

    with archive.extractfile('index.json') as source:
        index = json.load(source)
    if len(index['manifests']) != 1:
        raise ValueError('Export exactly one tagged Source Agent image')
    descriptor = select(index['manifests'][0])
    manifest = transfer(descriptor, 'manifests', parse=True)
    config = transfer(manifest['config'], 'blobs', parse=True)
    if (config['os'], config['architecture']) != ('linux', 'amd64'):
        raise ValueError('This registry release expects a Linux amd64 image')
    for layer in manifest['layers']:
        transfer(layer, 'blobs')
    original = root / 'manifests' / descriptor['digest']
    for name in (tag, 'latest'):
        with NamedTemporaryFile(dir=original.parent, delete=False) as output:
            temporary = Path(output.name)
            try:
                output.write(original.read_bytes())
                output.close()
                temporary.chmod(0o644)
                temporary.replace(original.with_name(name))
            finally:
                temporary.unlink(missing_ok=True)
    return descriptor['digest']


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('archive', type=Path)
    parser.add_argument('tag')
    args = parser.parse_args()
    if not re.fullmatch(r'[0-9a-f]{7,40}', args.tag):
        parser.error('Use the source commit SHA as the release tag')
    with tarfile.open(args.archive) as archive:
        print(publish(archive, Path(__file__).resolve().parents[1] / 'deploy/runtime/registry', args.tag))
