#!/usr/bin/env python3
"""The files the whole-list Grim run (wire parity item 93) needs on a hosted runner, from their official sources.

This is for the fork's own wire-grim workflow only: the project's tools never download the game. Everything lands
in the runner's temp folder, is checked against a pinned checksum, and is never committed or uploaded.

- Minecraft 26.3 from Mojang (piston-meta and its CDN) in the official launcher's layout (`--mc`): the version
  JSON research-lock.json accepts, the client jar, the libraries, the asset index and the assets other than
  sounds and music (no client plays sound here). The server jar goes to `--servers`/server.jar.
- Paper 26.3-253 and 1.21.11-132 (fill.papermc.io), Grim 2.3.75-8ec9b7d and ViaVersion 5.12.3-SNAPSHOT+1119
  (Modrinth's CDN) into `--servers`, the files the local runs used (same sha256 / sha512); Intave and ProtocolLib
  (Modrinth, ProtocolLib's GitHub release) for the Intave runs.
"""
import argparse
import concurrent.futures
import hashlib
import json
import sys  # noqa: F401 - sys.exit below
import time
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
AGENT = 'sunrise-wire-ci'
MANIFEST = 'https://piston-meta.mojang.com/mc/game/version_manifest_v2.json'
SERVERS = [
    ('paper-26.3-253.jar', 'sha256', 'f9df8e19f4bdee97dbded48095391ea69c64342a4e19624db4f5e0d0136dfe0b',
     'https://fill-data.papermc.io/v1/objects/f9df8e19f4bdee97dbded48095391ea69c64342a4e19624db4f5e0d0136dfe0b/'
     'paper-26.3-253.jar'),
    ('paper-1.21.11-132.jar', 'sha256', '5ffef465eeeb5f2a3c23a24419d97c51afd7dbb4923ff42df9a3f58bba1ccfba',
     'https://fill-data.papermc.io/v1/objects/5ffef465eeeb5f2a3c23a24419d97c51afd7dbb4923ff42df9a3f58bba1ccfba/'
     'paper-1.21.11-132.jar'),
    ('grimac-bukkit-2.3.75-8ec9b7d.jar', 'sha256', '668eb22fa0c370c75fbcf8525027561812dce047219072e230f21491032b5d66',
     'https://cdn.modrinth.com/data/LJNGWSvH/versions/z1TwCHQr/grimac-bukkit-2.3.75-8ec9b7d.jar'),
    ('ViaVersion-5.12.3-SNAPSHOT.jar', 'sha256', '5d0ee19b454ba1484a6f877e4028211ac397bc45b45b1d599aabb7a0703395fc',
     'https://cdn.modrinth.com/data/P1OZGk5p/versions/agKgcGJR/ViaVersion-5.12.3-SNAPSHOT.jar'),
    # Intave dev-4f18a06dce40 (Modrinth, 2026-11-04; 1.8 - 26.3) and the ProtocolLib it fetches itself for 1.21.10+
    # and 26.x (its GitHub dev-build release, the file of 2026-11-04).
    ('Intave-dev-4f18a06dce40.jar', 'sha512',
     'a86bbe29995bb685464648ff087621c749e522c308b01e24ff26e33ebb734b5ecd3ce7b0c96af9dd2f0f471c0c415631b75d173e6f'
     '64e73b158ba4dcd51a2442', 'https://cdn.modrinth.com/data/4zi91sqi/versions/DGfwdJGc/Intave.jar'),
    ('ProtocolLib-dev-build-20261104.jar', 'sha256', '736a9d6054032871d593aa2c0a0bf65e117557aadc11830394e5e8b757ae05b2',
     'https://github.com/dmulloy2/ProtocolLib/releases/download/dev-build/ProtocolLib-Spigot.jar'),
    # Velocity 4.3.1-45 (fill.papermc.io, 2026-10-27, stable) for the proxy runs (item 6).
    ('velocity-4.3.1-45.jar', 'sha256', '0844bcabdcdb4f5b875906fd4c3365bc1bb76918e42bc595ed88ad195258dd75',
     'https://fill-data.papermc.io/v1/objects/0844bcabdcdb4f5b875906fd4c3365bc1bb76918e42bc595ed88ad195258dd75/'
     'velocity-4.3.1-45.jar'),
]


def digest(path, algorithm):
    h = hashlib.new(algorithm)
    with open(path, 'rb') as f:
        for block in iter(lambda: f.read(1 << 20), b''):
            h.update(block)
    return h.hexdigest()


def fetch(url, destination, checksum, algorithm='sha1'):
    destination = Path(destination)
    if destination.exists() and digest(destination, algorithm) == checksum:
        return destination
    destination.parent.mkdir(parents=True, exist_ok=True)
    partial = destination.with_name(destination.name + '.part')
    for attempt in range(4):
        try:
            request = urllib.request.Request(url, headers={'User-Agent': AGENT})
            with urllib.request.urlopen(request, timeout=60) as response, open(partial, 'wb') as out:
                for block in iter(lambda: response.read(1 << 20), b''):
                    out.write(block)
            found = digest(partial, algorithm)
            if found != checksum:
                raise ValueError(f'{url}: {algorithm} {found}, expected {checksum}')
            partial.replace(destination)
            return destination
        except Exception as error:  # noqa: BLE001 - retried, then raised
            if attempt == 3:
                raise
            print(f'retrying {url}: {error}', flush=True)
            time.sleep(2 + attempt * 3)


def read_json(url):
    request = urllib.request.Request(url, headers={'User-Agent': AGENT})
    with urllib.request.urlopen(request, timeout=60) as response:
        return json.load(response)


def minecraft(mc, servers):
    lock = json.loads((ROOT / 'research-lock.json').read_text(encoding='utf-8'))['minecraft']
    version = lock['version']
    accepted = {lock['manifest_sha1'], *lock.get('accepted_manifest_sha1', [])}
    entry = next(v for v in read_json(MANIFEST)['versions'] if v['id'] == version)
    sha1 = entry['sha1']
    if sha1 not in accepted:
        # Mojang republished the version JSON (so far only its asset index changed); its packages stay addressed
        # by sha1, so the one research-lock.json pins is still there.
        print(f'Mojang now serves a {version} version JSON the lock does not know ({sha1}); using the pinned one',
              flush=True)
        sha1 = lock['manifest_sha1']
    url = f'https://piston-meta.mojang.com/v1/packages/{sha1}/{version}.json'
    manifest_path = fetch(url, mc / 'versions' / version / f'{version}.json', sha1)
    manifest = json.loads(manifest_path.read_text(encoding='utf-8'))
    client, server = manifest['downloads']['client'], manifest['downloads']['server']
    if client['sha1'] != lock['client_sha1'] or server['sha1'] != lock['server_sha1']:
        sys.exit('the version JSON names other jars than research-lock.json pins')
    jobs = [(client['url'], mc / 'versions' / version / f'{version}.jar', client['sha1']),
            (server['url'], servers / 'server.jar', server['sha1'])]
    for library in manifest['libraries']:
        downloads = library.get('downloads', {})
        for artifact in [downloads.get('artifact'), *downloads.get('classifiers', {}).values()]:
            if artifact and artifact.get('url'):
                jobs.append((artifact['url'], mc / 'libraries' / artifact['path'], artifact['sha1']))
    index_info = manifest['assetIndex']
    index_path = fetch(index_info['url'], mc / 'assets' / 'indexes' / f'{index_info["id"]}.json', index_info['sha1'])
    objects = json.loads(index_path.read_text(encoding='utf-8'))['objects']
    skipped = 0
    for name, info in objects.items():
        if name.startswith(('minecraft/sounds/', 'minecraft/music/')) or name.endswith('.ogg'):
            skipped += 1
            continue
        h = info['hash']
        jobs.append((f'https://resources.download.minecraft.net/{h[:2]}/{h}', mc / 'assets' / 'objects' / h[:2] / h, h))
    with concurrent.futures.ThreadPoolExecutor(16) as pool:
        for future in concurrent.futures.as_completed([pool.submit(fetch, *job) for job in jobs]):
            future.result()
    print(f'Minecraft {version}: {len(jobs)} files from Mojang ({skipped} sound assets left out)', flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--mc', type=Path, required=True, help='the official-launcher-shaped game folder to fill')
    parser.add_argument('--servers', type=Path, required=True, help='Paper, Grim, ViaVersion and server.jar')
    args = parser.parse_args()
    args.servers.mkdir(parents=True, exist_ok=True)
    for name, algorithm, checksum, url in SERVERS:
        fetch(url, args.servers / name, checksum, algorithm)
    print(f'{len(SERVERS)} server jars checked', flush=True)
    minecraft(args.mc, args.servers)


if __name__ == '__main__':
    main()
