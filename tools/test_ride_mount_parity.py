#!/usr/bin/env python3
"""A player put in a boat by `/ride` on a dedicated server, against vanilla's, seen by a vanilla client.

A vanilla dedicated server and Sunrise's (`sunrise-client --dedicated --nogui`) each serve a copy of one flat world
vanilla generated first, with Alice an operator. LocatorBarClient (a player made of vanilla's protocol code) summons
an oak boat where she stands and mounts it with `/ride @s mount`. ServerPlayer.startRiding seats her
(positionRider), teleports her there keeping her look (Relative.ROTATION) and sends the boat's passengers, all
before the command's reply; the tracker sends the passengers again on its next look. A `/rotate` and an effect
after it show what reaches her once she sits.

Alice's teleports, mounts and command replies in the mount and the steps after it are compared exactly; a reply
that names the boat by its key alone.
"""
import sys

try:
    from . import test_locator_bar_parity as locator
except ImportError:
    import test_locator_bar_parity as locator

NAME = 'ride-mount-parity'


def build(command):
    return f'cmd Alice {command}'


SCRIPT = [
    ('setup', [
        'chat on',
        # No natural spawns: a flat world's slimes attack Alice at their own random moments.
        build('gamerule spawn_monsters false'), build('gamerule spawn_mobs false'),
        build('forceload add -16 -16 16 16'),
        build('kill @e[type=minecraft:slime]'),
        build('tp @s 1.0 -60 0.5 0 0'),
    ]),
    ('mount', [
        build('summon minecraft:oak_boat 1.0 -60 0.5'),
        build('ride @s mount @e[type=minecraft:oak_boat,limit=1,sort=nearest]'),
    ]),
    ('seated', [
        build('rotate @s 0 0'),
        build('effect give @s minecraft:instant_health 1 9 true'),
    ]),
]

KEPT = ('teleport', 'passenger', 'chat', 'error')


# The replies that name the boat: its hover carries its random UUID, so only their key is compared.
NAMES_BOAT = ('"commands.summon.success"', '"commands.ride.mount.success"')


def shown(lines):
    """Alice's teleports, mounts and command replies from the mount on; the setup is left out."""
    steps = {f'step={step}' for step, _ in SCRIPT if step != 'setup'}
    kept = []
    for line in lines:
        parts = line.split(' ', 3)
        if len(parts) < 3 or parts[1] not in steps or parts[2] not in KEPT:
            continue
        key = next((key for key in NAMES_BOAT if key in line), None)
        kept.append(' '.join(parts[:3] + [key]) if key else line)
    return kept


def check(vanilla, vanilla_all):
    if not any(line.split(' ')[1:3] == ['step=mount', 'passenger'] for line in vanilla):
        return 'vanilla did not seat Alice in the mount step'
    return None


def main():
    lines = locator.one_player(NAME, SCRIPT, shown, check, 'A player put in a boat by /ride on a dedicated server: '
                               'the seat teleport and the passengers before the reply.')
    print(f'ride mount: {len(lines)} lines identical')


if __name__ == '__main__':
    try:
        main()
    except AssertionError as error:
        sys.exit(f'{NAME}: {error}')
