#!/usr/bin/env python3
"""A dedicated server saves connected players on save-all/stop/SIGTERM and keeps safe data after a panic.

`MinecraftServer.runServer` catches what the tick throws and still stops the server. Sunrise's
`stop_after_crash` does the same, but a panic can interrupt a guest's turn (the game holds that
player's state swapped in) or a tick that has the hub out. `SUNRISE_CRASH_FIXTURE` makes the tick
panic at one of those points. Each run below joins a real Sunrise client, panics the server, and
checks, from the files the server leaves:

- Connected player: save-all writes five diamonds while the player stays joined;
  stop writes the newer inventory of twelve diamonds.
- `guest`: the panic comes in the joined player's turn. The log names the player; their file is byte for
  byte what the previous clean stop wrote, and a block placed before the panic is still there on the next start.
- `hub`: the panic comes while a console line has the hub out of the game, with a player in
  (the fixture takes it out first, as a panic in the network tick leaves it). Same checks.
- Bystander: a second player, given diamonds and riding a boat, is in when the panic interrupts the
  first one's turn. A crash saves the players it did not interrupt as a stop does, each leaving with
  the vehicle they alone ride: the bystander's file holds the diamonds and the boat, and the world
  saved after them no longer has the boat (it would be there twice). The interrupted
  player keeps the files the previous clean stop wrote, and a lost hub saves only the world.
"""
import hashlib
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import uuid
import time
from pathlib import Path

try:
    from . import lab
    from .integration_env import run_game
    from .test_dedicated_server_parity import Server, write_server_dir, free_port
except ImportError:
    import lab
    from integration_env import run_game
    from test_dedicated_server_parity import Server, write_server_dir, free_port

PLAYER = 'Tester'


def binary():
    metadata = subprocess.check_output(['cargo', 'metadata', '--no-deps', '--format-version', '1'], cwd=lab.ROOT, text=True)
    import json
    return Path(json.loads(metadata)['target_directory']) / 'fast' / 'sunrise-client'


def start(directory, fixture=None, guest=None):
    """The server, with `fixture` set to panic at that point (in `guest`'s turn only, when named)."""
    if fixture:
        os.environ['SUNRISE_CRASH_FIXTURE'] = fixture
    if guest:
        os.environ['SUNRISE_CRASH_FIXTURE_GUEST'] = guest
    try:
        server = Server([binary(), '--resources', lab.RESOURCES, '--dedicated', '--nogui'], directory,
                        f'server-{fixture or "clean"}{"-" + guest if guest else ""}')
    finally:
        os.environ.pop('SUNRISE_CRASH_FIXTURE', None)
        os.environ.pop('SUNRISE_CRASH_FIXTURE_GUEST', None)
    server.wait_for(r'Done \(', timeout=600)
    return server


def join(port, out):
    """A real client in a thread: it joins, chats and leaves, or loses its server."""
    report = out / 'report.json'

    def run():
        try:
            run_game([str(c) for c in [binary(), '--resources', lab.RESOURCES, '--render-distance', 2, '--server',
                                       f'127.0.0.1:{port}', '--name', PLAYER]],
                     cwd=out, env=lab.game_env(SUNRISE_NET_REPORT=report), stdout=subprocess.DEVNULL,
                     stderr=subprocess.STDOUT, timeout=120)
        except subprocess.TimeoutExpired:
            pass
    thread = threading.Thread(target=run, daemon=True)
    thread.start()
    return thread


def offline_uuid(name):
    """`UUIDUtil.createOfflinePlayerUUID`."""
    digest = bytearray(hashlib.md5(f'OfflinePlayer:{name}'.encode()).digest())
    digest[6] = digest[6] & 0x0F | 0x30
    digest[8] = digest[8] & 0x3F | 0x80
    return str(uuid.UUID(bytes=bytes(digest)))


def player_files(directory, name=PLAYER):
    """A joined player's files (data, advancements, stats), by content. The server's own profile is
    not theirs: its files are written by every save."""
    folder = directory / 'world' / 'players'
    me = offline_uuid(name)
    return {str(p.relative_to(folder)): hashlib.sha256(p.read_bytes()).hexdigest()
            for p in sorted(folder.rglob(f'*{me}*')) if p.is_file()}


def crash(directory, fixture, port, out, trigger):
    server = start(directory, fixture)
    try:
        server.console('forceload add 0 0')
        # The chunk loads a moment after the forceload.
        for _ in range(100):
            server.console(f'setblock 0 {200 + trigger["y"]} 0 minecraft:gold_block')
            time.sleep(0.5)
            if any('Changed the block' in l for l in server.lines):
                break
        else:
            raise AssertionError('the block was never placed:\n' + '\n'.join(server.lines[-20:]))
        client = join(port, out)
        server.wait_for(rf'{PLAYER} joined the game', timeout=300)
        if trigger.get('console'):
            server.console('say crash now')
        deadline = time.time() + 120
        while server.process.poll() is None and time.time() < deadline:
            time.sleep(0.2)
        assert server.process.poll() not in (None, 0), 'the server did not exit with an error after the panic'
        client.join(timeout=10)
    finally:
        server.stop()
    return '\n'.join(server.lines)


def connected_saves(build):
    """Save-all and stop persist a player who is still connected, before logout can save them."""
    try:
        from . import anvil_dump, lan_e2e
    except ImportError:
        import anvil_dump
        import lan_e2e
    directory = build / 'connected-server'
    port = free_port()
    write_server_dir(directory, port)
    server = start(directory)
    client = lan_e2e.Game('autosave', binary().with_name('sunrise-ctl'), lab.RESOURCES)
    file = directory / 'world' / 'players' / 'data' / f'{offline_uuid(PLAYER)}.dat'

    def diamonds():
        player = anvil_dump.plain(anvil_dump.read_dat(file), full=True)
        return sum(item['count'] for item in player['Inventory'] if item['id'] == 'minecraft:diamond')

    try:
        client.raw('launch', '--headless', '--game', binary())
        client.raw('connect', f'127.0.0.1:{port}', PLAYER)
        client.raw('wait-for', 'loaded', '--timeout', 300)
        server.wait_for(rf'{PLAYER} joined the game')
        assert not file.exists(), 'the player was already saved before save-all'
        server.console(f'give {PLAYER} minecraft:diamond 5')
        server.console('save-all flush')
        # Console commands execute in order: the marker follows the save barrier.
        server.console('say SAVED-CONNECTED-PLAYER')
        server.wait_for(r'\[Server\] SAVED-CONNECTED-PLAYER')
        assert not any(f'{PLAYER} left the game' in line for line in server.lines)
        assert diamonds() == 5, 'save-all did not persist the connected player inventory'
        server.console(f'give {PLAYER} minecraft:diamond 7')
        server.console('say STOP-WITH-CONNECTED-PLAYER')
        server.wait_for(r'\[Server\] STOP-WITH-CONNECTED-PLAYER')
        assert not any(f'{PLAYER} left the game' in line for line in server.lines)
        server.stop()
        assert diamonds() == 12, 'stop did not persist the newer connected player inventory'
    finally:
        client.close()
        server.stop()
    print('connected player save-all and stop: ok', flush=True)


def signal_saves(build):
    """SIGTERM stops the server as `stop` does (vanilla's shutdown hook): a connected player's
    inventory and ender chest changed since the last save are in their file afterwards."""
    try:
        from . import anvil_dump, lan_e2e
    except ImportError:
        import anvil_dump
        import lan_e2e
    directory = build / 'signal-server'
    port = free_port()
    write_server_dir(directory, port)
    server = start(directory)
    client = lan_e2e.Game('sigterm', binary().with_name('sunrise-ctl'), lab.RESOURCES)
    file = directory / 'world' / 'players' / 'data' / f'{offline_uuid(PLAYER)}.dat'
    try:
        client.raw('launch', '--headless', '--game', binary())
        client.raw('connect', f'127.0.0.1:{port}', PLAYER)
        client.raw('wait-for', 'loaded', '--timeout', 300)
        server.wait_for(rf'{PLAYER} joined the game')
        server.console(f'give {PLAYER} minecraft:diamond 9')
        server.console(f'item replace entity {PLAYER} enderchest.0 with minecraft:emerald 5')
        server.console('say GIVEN-BEFORE-SIGTERM')
        server.wait_for(r'\[Server\] GIVEN-BEFORE-SIGTERM')
        assert not file.exists(), 'the player was saved before the signal'
        server.process.send_signal(signal.SIGTERM)
        try:
            server.process.wait(timeout=120)
        except subprocess.TimeoutExpired:
            raise AssertionError('the server did not stop within 120 s of SIGTERM')
        server.reader.join(timeout=10)
        assert any('Saving players' in line for line in server.lines), \
            'SIGTERM ended the server without saving:\n' + '\n'.join(server.lines[-10:])
        assert file.exists(), 'SIGTERM ended the server without writing the player file'
        player = anvil_dump.plain(anvil_dump.read_dat(file), full=True)
        diamonds = sum(i['count'] for i in player['Inventory'] if i['id'] == 'minecraft:diamond')
        emeralds = sum(i['count'] for i in player.get('EnderItems', []) if i['id'] == 'minecraft:emerald')
        assert (diamonds, emeralds) == (9, 5), f'saved {diamonds} diamonds and {emeralds} ender chest emeralds, not 9 and 5'
    finally:
        client.close()
        server.stop()
    print('connected player saved on SIGTERM: ok', flush=True)


def crash_saves_bystander(build):
    """A crash in one player's turn still saves the other player (MinecraftServer.stopServer), and
    leaves the interrupted player's files as the last clean stop wrote them."""
    try:
        from . import anvil_dump, lan_e2e
    except ImportError:
        import anvil_dump
        import lan_e2e
    bystander = 'Bystander'
    directory = build / 'bystander-server'
    port = free_port()
    write_server_dir(directory, port)
    out = build / 'bystander-client'
    out.mkdir()
    server = start(directory)
    try:
        join(port, out).join(timeout=300)
        server.wait_for(rf'{PLAYER} left the game', timeout=60)
    finally:
        server.stop()
    saved = player_files(directory)
    assert saved, 'the clean run wrote no player file'
    file = directory / 'world' / 'players' / 'data' / f'{offline_uuid(bystander)}.dat'
    server = start(directory, 'guest', guest=PLAYER)
    other = lan_e2e.Game('bystander', binary().with_name('sunrise-ctl'), lab.RESOURCES)
    try:
        other.raw('launch', '--headless', '--game', binary())
        other.raw('connect', f'127.0.0.1:{port}', bystander)
        other.raw('wait-for', 'loaded', '--timeout', 300)
        server.wait_for(rf'{bystander} joined the game')
        server.console(f'give {bystander} minecraft:diamond 4')
        server.console(f'execute at {bystander} run summon minecraft:oak_boat ~ ~ ~ {{Tags:["bystander_boat"]}}')
        server.console(f'ride {bystander} mount @e[tag=bystander_boat,limit=1]')
        # Stays in the world: once the restart shows it, the chunk's entities have loaded.
        server.console(f'execute at {bystander} run summon minecraft:armor_stand ~ ~ ~ {{Tags:["bystander_anchor"],NoGravity:1b}}')
        server.console('say GIVEN-TO-BYSTANDER')
        server.wait_for(r'\[Server\] GIVEN-TO-BYSTANDER')
        assert not file.exists(), 'the bystander was saved before the crash'
        client = join(port, out)
        deadline = time.time() + 300
        while server.process.poll() is None and time.time() < deadline:
            time.sleep(0.2)
        assert server.process.poll() not in (None, 0), 'the server did not exit with an error after the panic'
        client.join(timeout=10)
    finally:
        other.close()
        server.stop()
    log = '\n'.join(server.lines)
    assert f"The panic interrupted a player's turn ({PLAYER})" in log, 'the panic did not interrupt the player'
    assert 'Saving players' in log, 'the crash stopped the server without saving players'
    assert player_files(directory) == saved, f'the interrupted player\'s files changed: {saved} -> {player_files(directory)}'
    assert file.exists(), 'the crash save did not write the bystander'
    player = anvil_dump.plain(anvil_dump.read_dat(file), full=True)
    diamonds = sum(i['count'] for i in player['Inventory'] if i['id'] == 'minecraft:diamond')
    assert diamonds == 4, f'the bystander was saved with {diamonds} diamonds, not 4'
    # Only the shutdown's leave save (PlayerList.remove) puts the boat they alone ride in their file:
    # the world save keeps a ridden vehicle in its chunk.
    vehicle = player.get('RootVehicle', {}).get('Entity', {}).get('id')
    assert vehicle == 'minecraft:oak_boat', f'the bystander left without their boat in the file ({vehicle!r})'
    # ...and the boat left the world before it was saved: the restart, with the bystander away, has the
    # anchor beside it but no boat (a world saved first keeps a second one in the chunk).
    server = start(directory)
    try:
        server.console('forceload add -32 -32 32 32')
        for _ in range(60):
            server.console('execute if entity @e[tag=bystander_anchor] run say ANCHOR-LOADED')
            time.sleep(1)
            if any('ANCHOR-LOADED' in l for l in server.lines):
                break
        else:
            raise AssertionError('the anchor beside the boat never loaded after the restart')
        server.console('execute if entity @e[tag=bystander_boat] run say BOAT-STILL-IN-WORLD')
        server.console('say BOAT-CHECKED')
        server.wait_for(r'\[Server\] BOAT-CHECKED', timeout=60)
        assert not any('BOAT-STILL-IN-WORLD' in l for l in server.lines), \
            'the boat the bystander left with is also still in the world: it was saved twice'
    finally:
        server.stop()
    print('crash saves the bystander, keeps the interrupted player: ok', flush=True)

def main():
    subprocess.run(['cargo', 'build', '--profile', 'fast', '-p', 'sunrise-game', '-p', 'sunrise-client-lib', '--bin', 'sunrise-client', '--bin', 'sunrise-ctl'], cwd=lab.ROOT, check=True)
    build = lab.LOCAL / 'dedicated-crash-save'
    shutil.rmtree(build, ignore_errors=True)
    build.mkdir(parents=True)
    port = free_port()
    directory = build / 'server'
    write_server_dir(directory, port)
    out = build / 'client'
    out.mkdir()
    problems = []
    connected_saves(build)
    signal_saves(build)
    crash_saves_bystander(build)

    # A clean stop writes the player's file.
    server = start(directory)
    try:
        join(port, out).join(timeout=300)
        server.wait_for(rf'{PLAYER} left the game', timeout=60)
    finally:
        server.stop()
    saved = player_files(directory)
    if not saved:
        problems.append('the clean run wrote no player file')

    for fixture, y, trigger, expect in (
        ('guest', 0, {'y': 0}, f"The panic interrupted a player's turn ({PLAYER})"),
        ('hub', 1, {'y': 1, 'console': True}, 'The panic interrupted the network tick'),
    ):
        log = crash(directory, fixture, port, out, trigger)
        if f'The server thread panicked: crash fixture at {fixture}' not in log:
            problems.append(f'{fixture}: no panic in the log')
        if expect not in log:
            problems.append(f'{fixture}: the log does not say {expect!r}')
        if 'Saving worlds' not in log:
            problems.append(f'{fixture}: the world was not saved')
        if player_files(directory) != saved:
            problems.append(f'{fixture}: a player file changed: {saved} -> {player_files(directory)}')
        # The next start finds the block placed before the panic.
        server = start(directory)
        try:
            server.console('forceload add 0 0')
            time.sleep(3)
            server.console(f'execute if block 0 {200 + y} 0 minecraft:gold_block run say FOUND-{fixture}')
            server.wait_for(rf'\[Server\] FOUND-{fixture}', timeout=60)
        except AssertionError:
            problems.append(f'{fixture}: the block placed before the panic is gone')
        finally:
            server.stop()
    for p in problems:
        print('FAIL', p)
    print('dedicated crash save:', 'ok' if not problems else 'FAILED')
    return 1 if problems else 0


if __name__ == '__main__':
    sys.exit(main())
