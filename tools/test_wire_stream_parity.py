#!/usr/bin/env python3
"""The packets a Sunrise client sends, tick by tick and field by field, against a real vanilla 26.3 client's
for the same scripted play: an anticheat (Grim, Vulcan, Polar, Watchdog, Intave...) judges the serverbound
stream, also after ViaVersion translated it for an older server, so Sunrise's must be vanilla's.

A vanilla 26.3 dedicated server (flat world, offline) runs behind tools/net_capture.py. Each client joins it in
turn as Steve and plays every scenario below: the harness resets the place and the player from the server's
console, then hands the client the scenario, which it plays tick by tick from its own tick loop (Sunrise:
SUNRISE_TICK_SCRIPT, game/world/tick_script.rs; vanilla: VanillaTickScript.java, the unchanged client with its
keys pressed through KeyMapping). Both send /wire-mark on the scenario's first tick. tools/wire_stream.py
cuts each scenario's stream into client ticks (ClientTickEnd ends one) and decodes the packets; every tick
must hold the same packets, in the same order, with the same fields (doubles and floats exactly). Packets
the server's timing decides (keep-alive, pong, chunk batch, teleport acceptance) are compared as a sequence
over the scenario, and must never fall between two packets of one tick.

  python3 tools/test_wire_stream_parity.py                      # every scenario
  python3 tools/test_wire_stream_parity.py --scenarios walk,bow --keep
  python3 tools/test_wire_stream_parity.py --clients sunrise,sunrise-legacy   # Legacy (1.8.9) combat feel

The second form plays the same scenarios with two Sunrise clients, the second set to the Legacy combat feel with
every combat extra on (sunrise-extras.txt: `LEGACY_EXTRAS`), and compares their streams the same way: the feel
changes what the player sees and hears, never what the client sends, and the 1.8 rules ("in worlds you host")
never reach a server that isn't the player's own.

Sunrise runs `--headless` (no window or GPU: the ticks are what it sends from); the vanilla client runs on the
virtual screen (`vscreen`) on Linux, from the logged-in session on macOS. Every game and server here runs in fresh temporary folders (SUNRISE_HOME,
SUNRISE_LAUNCHER_HOME, XDG_*), with no audio and no self-update. Needs .local (the vanilla jars and prepared
resources) and a built sunrise-client and sunrise-ctl (--bin DIR, default the parity target's fast build).
"""
import argparse
import json
import os
import re
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import threading
import time

sys.path.insert(0, str(Path(__file__).resolve().parent))
import lab  # noqa: E402
import net_capture  # noqa: E402
import test_rust_parity as base  # noqa: E402
import wire_stream  # noqa: E402
from server_pack_e2e import Game  # noqa: E402
from test_dedicated_server_parity import Server, free_port  # noqa: E402

PLAYER = 'Steve'
PROPERTIES = {
    'online-mode': 'false', 'white-list': 'false', 'level-seed': '12345', 'level-type': 'minecraft\\:flat',
    'generate-structures': 'false', 'view-distance': '4', 'simulation-distance': '4', 'spawn-protection': '0',
    'pause-when-empty-seconds': '-1', 'gamemode': 'survival', 'difficulty': 'easy', 'spawn-monsters': 'false',
    'spawn-animals': 'false', 'enable-command-block': 'false',
}
# The flat world's grass is at y -61: the player stands at -60.
FLOOR = -61
RESET = [
    'gamerule spawn_mobs false', 'gamerule advance_time false', 'gamerule advance_weather false',
    f'fill -12 {FLOOR + 1} -12 12 {FLOOR + 12} 28 minecraft:air',
    f'fill -12 {FLOOR} -12 12 {FLOOR} 28 minecraft:grass_block',
    'kill @e[type=!minecraft:player]', f'clear {PLAYER}', f'effect clear {PLAYER}', f'gamemode survival {PLAYER}',
    f'effect give {PLAYER} minecraft:saturation 1 20 true', f'effect give {PLAYER} minecraft:instant_health 1 5 true',
    f'tp {PLAYER} 0.5 {FLOOR + 1} 0.5 0 0',
]
# A target that takes hits without moving: no AI, no knockback.
TARGET = ('summon minecraft:husk {x} {y} {z} {{NoAI:1b,Silent:1b,PersistenceRequired:1b,Health:1000f,'
          'attributes:[{{id:"minecraft:max_health",base:1000.0}},{{id:"minecraft:knockback_resistance",base:1.0}}]}}')
# Every scenario starts with a short step forward: it moves the player, which starts LocalPlayer.sendPosition's
# 20-tick reminder over on both clients alike, and selects the first slot.
PRELUDE = 10
# Client ticks between a reset's last teleport and the scenario: the player lands and stands still.
SETTLE = 40
# The Legacy client's sunrise-extras.txt (crates/sunrise-client-render/src/pvp.rs): the 1.8.9 feel, its
# indicator and sounds, and the 1.8 rules, which only a world the player hosts may use.
LEGACY_EXTRAS = 'combatFeel:legacy\ncombatIndicator:true\ncombatSounds:true\ncombatRules:true\n'
# The clients --clients may name: the first is the reference, the second must send what it sends.
CLIENTS = {'vanilla': 'vanilla', 'sunrise': 'Sunrise', 'sunrise-legacy': 'Sunrise Legacy'}


def taps(key, ticks):
    return {t: [f'key {key} tap'] for t in ticks}


def merge(*parts):
    out = {}
    for part in parts:
        for tick, lines in part.items():
            out.setdefault(tick, []).extend(lines)
    return out


def every(start, stop, step, *lines):
    return {t: list(lines) for t in range(start, stop, step)}


# A mob's hit on the player, from the player's own command: knockback away from the far husk (LivingEntity.knockback).
KNOCKBACK = 'command damage @s 1 minecraft:mob_attack by @e[tag=far,limit=1]'
# The attacker for KNOCKBACK, far ahead (+z): its hits push the player back (-z).
FAR_HUSK = ('summon minecraft:husk 0.5 {y} 9.0 {{NoAI:1b,Silent:1b,PersistenceRequired:1b,Tags:["far"]}}'
            .format(y=FLOOR + 1))
SWORD = f'item replace entity {PLAYER} weapon.mainhand with minecraft:diamond_sword'
# A husk with its AI, 6 blocks ahead: it walks to the player and attacks (a thousand health, takes knockback).
CHASER = ('summon minecraft:husk 0.5 {y} 6.5 {{PersistenceRequired:1b,Health:1000f,'
          'attributes:[{{id:"minecraft:max_health",base:1000.0}}]}}'.format(y=FLOOR + 1))
# Puts the player 0.3 into a husk at z 2.2, from behind.
OVERLAP = 'command tp @s 0.5 ~ 1.9'
# A smash's fall damage, and a 20-block fall's, must not kill the player mid-scenario (a death screen ends it).
MACE_RESISTANCE = f'effect give {PLAYER} minecraft:resistance 60 4 true'


def mace_fall(height, enchantment=None, effects=(), target_z=5.2):
    """The player stands on a 3x3 stone platform `height` blocks up, walks off its south edge looking down and
    ahead, and swings a mace every third tick on the way down at a target standing below: MaceItem's smash needs
    a fall of more than 1.5 blocks (heavy past 5), and the server answers a hit with the attacker's new motion
    (ClientboundSetEntityMotionPacket), which the client's next moves follow."""
    top = FLOOR + height
    mace = 'minecraft:mace' + (f'[minecraft:enchantments={{{enchantment}}}]' if enchantment else '')
    fall_ticks = 20 + 2 * height
    return dict(
        setup=[f'fill -1 {top} -1 1 {top} 1 minecraft:stone', TARGET.format(x=0.5, y=FLOOR + 1, z=target_z),
               MACE_RESISTANCE, *[f'effect give {PLAYER} {effect} 60 0 true' for effect in effects],
               f'item replace entity {PLAYER} weapon.mainhand with {mace}', f'tp {PLAYER} 0.5 {top + 1} 0.5 0 0'],
        ticks=20 + fall_ticks + 20,
        steps=merge({0: ['look 0 55', 'key w down'], 14: ['key w up']},
                    every(12, 12 + fall_ticks, 3, 'mouse left click')))


SCENARIOS = {
    'walk': dict(ticks=70, steps=merge(
        {0: ['key w down'], 20: ['look 30 0'], 30: ['look 30 15'], 40: ['key w up'], 50: ['look 0 0']})),
    'idle': dict(ticks=60, steps={}),
    'sprint_jump': dict(ticks=70, steps={
        0: ['key ctrl down', 'key w down'], 5: ['key space down'], 45: ['key space up'],
        50: ['key w up', 'key ctrl up']}),
    'strafe': dict(ticks=70, steps={
        0: ['key w down', 'key a down'], 12: ['key a up', 'key d down'], 24: ['key d up', 'key a down'],
        36: ['key a up', 'key ctrl down'], 48: ['key w up', 'key ctrl up', 'key s down'], 56: ['key s up']}),
    'w_tap': dict(ticks=70, steps=merge(
        {0: ['key ctrl down', 'key w down']},
        every(10, 50, 8, 'key w up'), every(12, 52, 8, 'key w down'),
        {55: ['key w up', 'key ctrl up']})),
    'sneak': dict(ticks=60, steps={
        0: ['key shift down'], 5: ['key w down'], 25: ['key shift up'], 30: ['key shift down'],
        40: ['key w up'], 45: ['key shift up']}),
    'hit_target': dict(
        setup=[TARGET.format(x=0.5, y=FLOOR + 1, z=3.0), f'item replace entity {PLAYER} weapon.mainhand with '
               'minecraft:diamond_sword'],
        ticks=70, steps=merge({0: ['look 0 20']}, every(5, 65, 12, 'mouse left click'))),
    'sprint_combo': dict(
        setup=[TARGET.format(x=0.5, y=FLOOR + 1, z=7.0), f'item replace entity {PLAYER} weapon.mainhand with '
               'minecraft:diamond_sword'],
        ticks=60, steps=merge(
            {0: ['look 0 10', 'key ctrl down', 'key w down']}, every(12, 52, 10, 'mouse left click'),
            every(14, 54, 10, 'key w up'), every(16, 56, 10, 'key w down'), {56: ['key w up', 'key ctrl up']})),
    'block_hit': dict(
        setup=[TARGET.format(x=0.5, y=FLOOR + 1, z=3.0),
               f'item replace entity {PLAYER} weapon.mainhand with minecraft:diamond_sword',
               f'item replace entity {PLAYER} weapon.offhand with minecraft:shield'],
        ticks=70, steps={0: ['look 0 20'], 5: ['mouse left click'], 8: ['mouse right down'], 20: ['mouse right up'],
                         22: ['mouse left click'], 25: ['mouse right down'], 30: ['mouse left click'],
                         40: ['mouse right up'], 50: ['mouse left click']}),
    'rod': dict(
        setup=[TARGET.format(x=0.5, y=FLOOR + 1, z=6.0),
               f'item replace entity {PLAYER} weapon.mainhand with minecraft:fishing_rod'],
        ticks=80, steps={0: ['look 0 -5'], 5: ['mouse right click'], 35: ['mouse right click'],
                         45: ['mouse right click'], 70: ['mouse right click']}),
    'bow': dict(
        setup=[f'item replace entity {PLAYER} weapon.mainhand with minecraft:bow',
               f'give {PLAYER} minecraft:arrow 16'],
        ticks=80, steps={0: ['look 0 -10'], 5: ['mouse right down'], 30: ['mouse right up'],
                         40: ['mouse right down'], 45: ['mouse right up'], 55: ['mouse right down'],
                         75: ['mouse right up']}),
    'eat': dict(
        setup=[f'item replace entity {PLAYER} weapon.mainhand with minecraft:golden_apple 4'],
        # Two bites of 10 ticks, a third of the 32 a golden apple takes: when a full one ends is the server's to
        # say (its entity event), and a loaded server that catches up runs its ticks in a burst.
        ticks=50, steps={5: ['mouse right down'], 15: ['mouse right up'], 25: ['mouse right down'],
                         35: ['mouse right up']}),
    'bridge': dict(
        setup=[f'fill -2 {FLOOR + 1} -2 2 {FLOOR + 2} 2 minecraft:stone', f'tp {PLAYER} 0.5 {FLOOR + 3} 2.2 180 78',
               f'item replace entity {PLAYER} weapon.mainhand with minecraft:stone 64'],
        ticks=90, steps=merge({0: ['look 0 78', 'key shift down', 'key s down']}, every(4, 80, 4, 'mouse right click'),
                              {82: ['key s up', 'key shift up']})),
    'inventory': dict(
        setup=[f'item replace entity {PLAYER} hotbar.0 with minecraft:diamond_sword',
               f'item replace entity {PLAYER} hotbar.1 with minecraft:cooked_beef 16',
               f'item replace entity {PLAYER} inventory.0 with minecraft:golden_apple 8'],
        ticks=60, steps={5: ['key e tap'], 10: ['click-slot 36'], 14: ['click-slot 9'], 18: ['click-slot 36'],
                         22: ['click-slot 37 shift'], 26: ['click-slot 10 right'], 30: ['key e tap'],
                         40: ['key e tap'], 45: ['key esc tap']}),
    'hotbar_swap': dict(
        setup=[TARGET.format(x=0.5, y=FLOOR + 1, z=3.0)] +
              [f'item replace entity {PLAYER} hotbar.{i} with minecraft:{item}' for i, item in
               enumerate(['diamond_sword', 'stone_axe', 'golden_apple', 'stone', 'bow'])],
        # 52-54: a swing at the sky with a new slot sends no slot before its Punch (no ensureHasSentCarriedItem
        # on a miss); the next tick's MultiPlayerGameMode.tick does.
        ticks=60, steps=merge({0: ['look 0 20']}, {t: [f'key {t % 5 + 1} tap'] for t in range(5, 15)},
                              {16: ['key 2 tap', 'mouse left click'], 20: ['key 1 tap'], 21: ['key 3 tap'],
                               30: ['key 5 tap', 'mouse left click'], 35: ['key q tap'], 40: ['key f tap'],
                               52: ['look 0 -80'], 54: ['key 3 tap', 'mouse left click']})),
    'dig_teleport': dict(
        setup=[f'setblock 0 {FLOOR + 1} 2 minecraft:stone'],
        ticks=60, anchor='accept_teleportation',
        steps={0: ['look 0 45'], 3: ['mouse left down'], 15: ['command tp @s ~ ~ ~'], 40: ['mouse left up']}),
    # A mace on the ground: no fall, so no smash; one hit at full charge and one early.
    'mace_ground': dict(
        setup=[TARGET.format(x=0.5, y=FLOOR + 1, z=3.0), f'item replace entity {PLAYER} weapon.mainhand with '
               'minecraft:mace'],
        ticks=80, steps={0: ['look 0 20'], 5: ['mouse left click'], 20: ['mouse left click'],
                         60: ['mouse left click']}),
    **{f'mace_fall_{height}': mace_fall(height) for height in (3, 5, 10, 20)},
    # Each of the mace's own enchantments on a 5-block smash; Wind Burst launches the attacker back up.
    **{f'mace_{name}': mace_fall(5, enchantment=f'"minecraft:{name}":{level}')
       for name, level in (('density', 5), ('breach', 4), ('wind_burst', 3))},
    # A smash from 10 blocks under Slow Falling, whose fall the server still counts.
    'mace_slow_falling': mace_fall(10, effects=['minecraft:slow_falling']),
    # The first demo's geometry: the player lands past the target, still sliding, with no key held.
    'mace_fall_5_overshoot': mace_fall(5, target_z=3.4),
    # Falling past the target and swinging at the air: the misses' 10-tick delay, no hit.
    'mace_fall_miss': mace_fall(10, target_z=8.0),
    # Knockback while overlapping a mob. The player is teleported 0.3 into a husk in front of it (z 1.9 against
    # 2.2), whose pushEntities then pushes it back each tick, after the player's own tick; two ticks later a second
    # husk far ahead hits it, so the knockback and the push both point back and nothing blocks them. The server's
    # motion replaces the player's velocity, push included (ClientPacketListener.handleSetEntityMotion).
    'push_knockback': dict(
        setup=[TARGET.format(x=0.5, y=FLOOR + 1, z=2.2), FAR_HUSK],
        ticks=60, steps=merge({t: [OVERLAP] for t in (10, 25, 40)}, {t: [KNOCKBACK] for t in (12, 27, 42)})),
    # The same teleports and hits with no husk to overlap: the control for push_knockback.
    'push_knockback_control': dict(
        setup=[FAR_HUSK],
        ticks=60, steps=merge({t: [OVERLAP] for t in (10, 25, 40)}, {t: [KNOCKBACK] for t in (12, 27, 42)})),
    # A teleport onto itself two ticks after overlapping the husk: the teleport's velocity reset and the push.
    'push_teleport': dict(
        setup=[TARGET.format(x=0.5, y=FLOOR + 1, z=2.2)],
        ticks=60, steps={10: [OVERLAP], 12: ['command tp @s ~ ~ ~'], 30: [OVERLAP], 32: ['command tp @s ~ ~ ~']}),
    # Knockback from the far husk in different states, no mob in reach: what the server's motion meets.
    'knockback_sprint': dict(
        setup=[FAR_HUSK],
        ticks=60, steps=merge({0: ['key ctrl down', 'key w down'], 40: ['key w up', 'key ctrl up']},
                              {t: [KNOCKBACK] for t in (10, 25)})),
    'knockback_air': dict(
        setup=[FAR_HUSK],
        ticks=60, steps={5: ['key space down'], 7: ['key space up', KNOCKBACK], 30: ['key space down'],
                         33: ['key space up', KNOCKBACK]}),
    'knockback_sneak': dict(
        setup=[FAR_HUSK],
        ticks=60, steps={0: ['key shift down', 'key s down'], 15: [KNOCKBACK], 30: [KNOCKBACK],
                         45: ['key s up', 'key shift up']}),
    # No sprint at 6 food or less (LocalPlayer.hasEnoughFoodToSprint): Hunger 255 empties the bar at once.
    'hunger_sprint': dict(
        setup=[f'effect give {PLAYER} minecraft:hunger 3 255 true'],
        ticks=60, steps={5: ['key ctrl down', 'key w down'], 40: ['key w up', 'key ctrl up']}),
    # Opening the inventory while walking: Minecraft.setScreen releases every key (KeyMapping.releaseAll), so the
    # player stops though W was never let go; it walks again only after W is pressed anew.
    'inventory_walk': dict(
        ticks=60, steps={0: ['key w down'], 10: ['key e tap'], 30: ['key e tap'], 40: ['key w down'],
                         50: ['key w up']}),
    # Hitting a mob that moves (item 112): which entity the crosshair picks, and when, follows how the client
    # interpolates the mob's moves (the server sends them; a replay gives both clients the same). A husk with its
    # AI walks up and hits back (its knockback too); the player strafes and swings, or sprint-hits with w-taps.
    'moving_husk_fight': dict(
        setup=[MACE_RESISTANCE, CHASER, f'item replace entity {PLAYER} weapon.mainhand with minecraft:diamond_sword'],
        ticks=100, steps=merge({0: ['look 0 15', 'key a down'], 25: ['key a up', 'key d down'], 50: ['key d up',
                                'key a down'], 75: ['key a up']}, every(8, 96, 12, 'mouse left click'))),
    'moving_husk_sprint': dict(
        setup=[MACE_RESISTANCE, CHASER, f'item replace entity {PLAYER} weapon.mainhand with minecraft:diamond_sword'],
        ticks=100, steps=merge({0: ['look 0 10', 'key ctrl down', 'key w down']}, every(10, 90, 12, 'mouse left click'),
                               every(12, 92, 12, 'key w up'), every(15, 95, 12, 'key w down'),
                               {95: ['key w up', 'key ctrl up']})),
    # The same with a mace, from the ground: hits on a mob walking in and out of reach.
    'moving_husk_mace': dict(
        setup=[MACE_RESISTANCE, CHASER, f'item replace entity {PLAYER} weapon.mainhand with minecraft:mace'],
        ticks=100, steps=merge({0: ['look 0 15', 'key s down'], 30: ['key s up']}, every(10, 96, 17, 'mouse left click'))),
    # Movement on special blocks and shapes, all client physics a movement-predicting anticheat simulates. Each
    # builds its course in front of the player (+z) inside the area the reset refills.
    'packed_ice_sprint': dict(
        setup=[f'fill -1 {FLOOR} 1 1 {FLOOR} 14 minecraft:packed_ice'],
        ticks=70, steps=merge({0: ['key ctrl down', 'key w down'], 40: ['key w up', 'key ctrl up']},
                              {10: ['key space down'], 12: ['key space up'], 25: ['key space down'], 27: ['key space up']})),
    'blue_ice_sprint': dict(
        setup=[f'fill -1 {FLOOR} 1 1 {FLOOR} 14 minecraft:blue_ice'],
        ticks=70, steps={0: ['key ctrl down', 'key w down'], 20: ['key w up', 'key ctrl up'], 35: ['key s down'],
                         45: ['key s up']}),
    'soul_sand_walk': dict(
        setup=[f'fill -1 {FLOOR} 1 1 {FLOOR} 8 minecraft:soul_sand'],
        ticks=60, steps={0: ['key ctrl down', 'key w down'], 15: ['key space down'], 17: ['key space up'],
                         40: ['key w up', 'key ctrl up']}),
    'honey_walk': dict(
        setup=[f'fill -1 {FLOOR} 1 1 {FLOOR} 8 minecraft:honey_block'],
        ticks=60, steps={0: ['key w down'], 15: ['key space down'], 17: ['key space up'], 40: ['key w up']}),
    # Falling 5 blocks onto slime: the bounces; then onto it again sneaking (no bounce). The player starts on the
    # ground and is put up there from the scenario, so both clients start the fall on the same tick.
    'slime_bounce': dict(
        setup=[f'fill -1 {FLOOR} -1 1 {FLOOR} 1 minecraft:slime_block'],
        ticks=90, steps={0: ['command tp @s 0.5 ~5 0.5'], 45: ['key shift down'], 46: ['command tp @s 0.5 ~5 0.5'],
                         85: ['key shift up']}),
    'cobweb_walk': dict(
        setup=[f'fill 0 {FLOOR + 1} 3 0 {FLOOR + 2} 4 minecraft:cobweb'],
        ticks=70, steps={0: ['key w down'], 20: ['key space down'], 22: ['key space up'], 55: ['key w up']}),
    'powder_snow_walk': dict(
        setup=[f'fill 0 {FLOOR} 3 0 {FLOOR + 1} 4 minecraft:powder_snow'],
        ticks=70, steps={0: ['key w down'], 25: ['key space down'], 35: ['key space up'], 55: ['key w up']}),
    # Up a ladder, held still sneaking halfway, on up, then off backwards.
    'ladder_climb': dict(
        setup=[f'fill 0 {FLOOR + 1} 2 0 {FLOOR + 8} 2 minecraft:stone',
               f'fill 0 {FLOOR + 1} 1 0 {FLOOR + 8} 1 minecraft:ladder[facing=north]'],
        ticks=90, steps={0: ['key w down'], 25: ['key shift down'], 40: ['key shift up'], 55: ['key w up'],
                         56: ['key s down', 'key space down'], 58: ['key space up'], 65: ['key s up']}),
    'scaffolding_climb': dict(
        setup=[f'fill 0 {FLOOR + 1} 1 0 {FLOOR + 6} 1 minecraft:scaffolding'],
        ticks=90, steps={0: ['key w down'], 4: ['key w up', 'key space down'], 35: ['key space up'],
                         45: ['key shift down'], 75: ['key shift up']}),
    # Into a pool, sprint-swimming underwater, up and out over the edge.
    'water_swim': dict(
        setup=[f'fill -3 {FLOOR - 3} 2 3 {FLOOR} 10 minecraft:water'],
        ticks=100, steps={0: ['key w down'], 15: ['look 0 40', 'key ctrl down'], 45: ['look 0 -30'],
                          60: ['key ctrl up', 'key space down'], 85: ['key space up', 'key w up']}),
    # Sprint-jumping under a ceiling two blocks up: every jump hits it.
    'head_hitter': dict(
        setup=[f'fill -1 {FLOOR + 3} 1 1 {FLOOR + 3} 14 minecraft:stone'],
        ticks=60, steps={0: ['key ctrl down', 'key w down', 'key space down'], 40: ['key space up', 'key w up',
                                                                                    'key ctrl up']}),
    # Sprinting up a stair, a slab and a step (0.5 and 1 high), jumping where it must.
    'stairs_slabs_sprint': dict(
        setup=[f'setblock 0 {FLOOR + 1} 3 minecraft:stone_slab', f'setblock 0 {FLOOR + 1} 4 minecraft:stone',
               f'setblock 0 {FLOOR + 2} 5 minecraft:stone_stairs[facing=south]',
               f'fill 0 {FLOOR + 1} 5 0 {FLOOR + 1} 9 minecraft:stone', f'fill 0 {FLOOR + 2} 6 0 {FLOOR + 2} 9 minecraft:stone',
               f'setblock 0 {FLOOR + 3} 7 minecraft:oak_slab'],
        ticks=60, steps={0: ['key ctrl down', 'key w down'], 8: ['key space down'], 9: ['key space up'],
                         40: ['key w up', 'key ctrl up']}),
    # Sneaking to the edge of a raised platform, straight and diagonally: it holds the player on it.
    'sneak_edge': dict(
        setup=[f'fill -1 {FLOOR + 1} -1 1 {FLOOR + 1} 2 minecraft:stone', f'tp {PLAYER} 0.5 {FLOOR + 2} 0.5 0 0'],
        ticks=70, steps={0: ['key shift down', 'key w down'], 30: ['key w up', 'key d down', 'key s down'],
                         55: ['key d up', 'key s up', 'key shift up']}),
    # Diagonally along a wall: horizontal collision each tick.
    'wall_slide': dict(
        setup=[f'fill 2 {FLOOR + 1} -2 2 {FLOOR + 3} 14 minecraft:stone'],
        ticks=60, steps={0: ['look -20 0', 'key ctrl down', 'key w down', 'key d down'], 40: ['key w up', 'key d up',
                                                                                             'key ctrl up']}),
    'speed_jump_effects': dict(
        setup=[f'effect give {PLAYER} minecraft:speed 30 2 true', f'effect give {PLAYER} minecraft:jump_boost 30 2 true'],
        ticks=60, steps={0: ['key ctrl down', 'key w down', 'key space down'], 30: ['key space up'],
                         40: ['key w up', 'key ctrl up']}),
    # The effects come from the scenario's own commands, so both clients start rising on the same tick.
    'levitation_slow_falling': dict(
        ticks=90, steps={0: ['command effect give @s minecraft:levitation 2 1 true', 'key w down'],
                         45: ['command effect give @s minecraft:slow_falling 10 0 true'], 60: ['key w up']}),
    # Combat details (plan items 40-54, 110) and interactions (108-111, 128, 134-136).
    # Critical hits: jumping and swinging on the way down.
    'crit_hits': dict(
        setup=[TARGET.format(x=0.5, y=FLOOR + 1, z=2.6), SWORD],
        ticks=80, steps=merge({0: ['look 0 15']}, {t: ['key space down'] for t in (5, 30, 55)},
                              {t: ['key space up'] for t in (7, 32, 57)}, {t: ['mouse left click'] for t in (12, 37, 62)})),
    # A full-charge hit on the ground beside two mobs: the sweep.
    'sweep_hits': dict(
        setup=[TARGET.format(x=0.5, y=FLOOR + 1, z=2.6), TARGET.format(x=1.6, y=FLOOR + 1, z=2.8), SWORD],
        ticks=70, steps=merge({0: ['look 0 15']}, every(5, 65, 15, 'mouse left click'))),
    # A click every tick (far faster than the cooldown) at a mob in reach.
    'high_cps': dict(
        setup=[TARGET.format(x=0.5, y=FLOOR + 1, z=2.6), SWORD],
        ticks=50, steps=merge({0: ['look 0 15']}, every(5, 45, 1, 'mouse left click'))),
    # A mob at the edge of reach, the player stepping in and out of it while swinging.
    'reach_limit': dict(
        setup=[TARGET.format(x=0.5, y=FLOOR + 1, z=3.85), SWORD],
        ticks=70, steps=merge({0: ['look 0 5'], 10: ['key w down'], 12: ['key w up'], 30: ['key s down'],
                               32: ['key s up']}, every(5, 65, 6, 'mouse left click'))),
    # An end crystal set on obsidian and hit: its explosion's knockback, under Resistance.
    'end_crystal': dict(
        setup=[MACE_RESISTANCE, f'setblock 0 {FLOOR} 3 minecraft:obsidian',
               f'item replace entity {PLAYER} weapon.mainhand with minecraft:end_crystal 4'],
        ticks=60, steps={0: ['look 0 40'], 5: ['mouse right click'], 15: ['mouse left click']}),
    'ender_pearl': dict(
        setup=[f'item replace entity {PLAYER} weapon.mainhand with minecraft:ender_pearl 4'],
        ticks=80, steps={0: ['look 0 -20'], 5: ['mouse right click'], 50: ['mouse right click']}),
    'throw_projectiles': dict(
        setup=[f'item replace entity {PLAYER} hotbar.0 with minecraft:snowball 16',
               f'item replace entity {PLAYER} hotbar.1 with minecraft:egg 16',
               f'item replace entity {PLAYER} hotbar.2 with minecraft:wind_charge 16'],
        ticks=60, steps=merge({0: ['look 0 -10']}, every(5, 20, 4, 'mouse right click'), {22: ['key 2 tap']},
                              every(25, 40, 4, 'mouse right click'), {42: ['key 3 tap']}, {45: ['mouse right click']})),
    'trident_throw': dict(
        setup=[f'item replace entity {PLAYER} weapon.mainhand with minecraft:trident'],
        ticks=60, steps={0: ['look 0 -5'], 5: ['mouse right down'], 20: ['mouse right up']}),
    'crossbow_shoot': dict(
        setup=[f'item replace entity {PLAYER} weapon.mainhand with minecraft:crossbow',
               f'give {PLAYER} minecraft:arrow 16'],
        ticks=80, steps={0: ['look 0 -5'], 5: ['mouse right down'], 35: ['mouse right up'], 45: ['mouse right click'],
                         55: ['mouse right down'], 60: ['mouse right up']}),
    # Holding a shield up while a husk with its AI attacks.
    'shield_block_mob': dict(
        setup=[MACE_RESISTANCE, CHASER, f'item replace entity {PLAYER} weapon.offhand with minecraft:shield'],
        ticks=90, steps={0: ['look 0 10'], 10: ['mouse right down'], 60: ['mouse right up'], 70: ['mouse right down'],
                         85: ['mouse right up']}),
    # Totems swapped to the offhand and back in quick succession.
    'totem_swap': dict(
        setup=[f'item replace entity {PLAYER} hotbar.0 with minecraft:totem_of_undying',
               f'item replace entity {PLAYER} hotbar.1 with minecraft:totem_of_undying'],
        ticks=50, steps={5: ['key f tap'], 7: ['key f tap'], 8: ['key 2 tap'], 9: ['key f tap'], 20: ['key f tap'],
                         21: ['key 1 tap', 'key f tap']}),
    # Two clicks and two hotbar keys in one tick (all handled in that tick).
    'inputs_one_tick': dict(
        setup=[TARGET.format(x=0.5, y=FLOOR + 1, z=2.6), SWORD,
               f'item replace entity {PLAYER} hotbar.1 with minecraft:stone 16'],
        ticks=50, steps={0: ['look 0 15'], 10: ['mouse left click', 'mouse left click'], 20: ['key 2 tap', 'key 1 tap'],
                         30: ['key 2 tap', 'mouse right click', 'mouse right click']}),
    # Holding right click on the ground ahead: vanilla places every 4 ticks (rightClickDelay).
    'held_place': dict(
        setup=[f'item replace entity {PLAYER} weapon.mainhand with minecraft:stone 64'],
        ticks=50, steps={0: ['look 0 60'], 5: ['mouse right down'], 35: ['mouse right up']}),
    # Towering: jumping and placing a block under the player at the top of each jump.
    'tower_up': dict(
        setup=[f'item replace entity {PLAYER} weapon.mainhand with minecraft:stone 64'],
        ticks=80, steps=merge({0: ['look 0 90'], 5: ['key space down'], 70: ['key space up']},
                              every(9, 70, 6, 'mouse right click'))),
    # A water bucket placed under the player just before it lands from 10 blocks, then scooped up.
    'water_clutch': dict(
        setup=[f'item replace entity {PLAYER} weapon.mainhand with minecraft:water_bucket',
               f'tp {PLAYER} 0.5 {FLOOR + 11} 0.5 0 90'],
        ticks=70, steps={0: ['look 0 90'], 24: ['mouse right click'], 26: ['mouse right click'], 40: ['mouse right click']}),
    # Instant mining: Haste II and an Efficiency V pickaxe on a row of stone, walking along it.
    'instant_mine': dict(
        setup=[f'fill 0 {FLOOR + 1} 2 0 {FLOOR + 1} 8 minecraft:stone', f'effect give {PLAYER} minecraft:haste 30 1 true',
               f'item replace entity {PLAYER} weapon.mainhand with minecraft:diamond_pickaxe[minecraft:enchantments={{"minecraft:efficiency":5}}]'],
        ticks=70, steps={0: ['look 0 40', 'mouse left down'], 5: ['key w down'], 55: ['key w up', 'mouse left up']}),
    # An armor stand given a helmet with a right click on its head.
    'armor_stand_interact': dict(
        setup=[f'summon minecraft:armor_stand 0.5 {FLOOR + 1} 2.5 {{Rotation:[180f,0f]}}',
               f'item replace entity {PLAYER} weapon.mainhand with minecraft:iron_helmet'],
        ticks=40, steps={0: ['look 0 -20'], 5: ['mouse right click'], 20: ['look 0 -20'], 25: ['mouse right click']}),
    # A wind charge at the feet launches the player; the mace comes out and smashes on the way down.
    'mace_wind_charge': dict(
        setup=[TARGET.format(x=0.5, y=FLOOR + 1, z=2.4), MACE_RESISTANCE,
               f'item replace entity {PLAYER} hotbar.0 with minecraft:mace',
               f'item replace entity {PLAYER} hotbar.1 with minecraft:wind_charge 8'],
        ticks=90, steps=merge({0: ['key 2 tap', 'look 0 90'], 4: ['mouse right click'], 8: ['key 1 tap'],
                               12: ['look 0 55']}, every(20, 80, 3, 'mouse left click'))),
    # A TNT's explosion beside the player (the explode packet's knockback), under Resistance; its fuse outlasts the
    # reset's settling. Last: the crater
    # stays below the floor the reset refills.
    'tnt_knockback': dict(
        setup=[MACE_RESISTANCE, f'summon minecraft:tnt 0.5 {FLOOR + 1} 3.0 {{fuse:80}}'],
        ticks=60, steps={}),
}


def sunrise_lines(lines):
    """A scenario's lines for sunrise-ctl: menu slot ids become its inventory indices (hotbar 0-8, main 9-35)."""
    out = []
    for line in lines:
        w = line.split()
        if w[0] == 'click-slot':
            slot = int(w[1])
            w[1] = str(slot - 36 if 36 <= slot <= 44 else slot)
        out.append(' '.join(w))
    return out


def script(name, spec, sunrise):
    steps = {0: ['key 1 tap', 'key w down'], 2: ['key w up']}
    for tick, lines in spec['steps'].items():
        steps.setdefault(tick + PRELUDE, []).extend(sunrise_lines(lines) if sunrise else lines)
    return {'name': name, 'ticks': spec['ticks'] + PRELUDE, 'steps': {str(k): v for k, v in sorted(steps.items())}}


# WIRE_STREAM_GRIM=DIR: the server is Paper with the Grim anticheat (DIR holds paper-<version>-*.jar and
# grimac-bukkit-*.jar, which the user downloaded); each client's Grim violations are counted after the run.


def grim_server(directory, jars):
    """The command for Paper and Grim in `directory`. Paperclip patches the Mojang server jar it finds in its cache,
    so it is given the prepared copy (the same jar its download-context names) and downloads nothing."""
    servers = Path(os.environ['WIRE_STREAM_GRIM'])
    paper = next(servers.glob(f'paper-{lab.VERSION}-*.jar'))
    (directory / 'cache').mkdir()
    # The bundled server.jar the user imported (jars['server'] is the unbundled game jar inside it).
    shutil.copy(jars['server'].with_name('server.jar'), directory / 'cache' / f'mojang_{lab.VERSION}.jar')
    (directory / 'plugins').mkdir()
    shutil.copy(next(servers.glob('grimac-bukkit-*.jar')), directory / 'plugins')
    shutil.copy(paper, directory / 'paper.jar')
    return ['java', '-Xmx1536m', '-jar', 'paper.jar', 'nogui']


def grim_violations(db, clients):
    """Each client's Grim violations, from Grim's own history database: it logs every one (punishments.yml
    `1:1 [log]`) but prints an alert only from a violation level of 100 (`100:40 [alert]`), so the console shows
    small deviations never. The clients joined one after the other: their sessions, in order."""
    import sqlite3
    con = sqlite3.connect(db)
    checks = dict(con.execute('select check_id, display from grim_checks'))
    sessions = [row[0] for row in con.execute('select session_id from grim_sessions order by started_at')]
    out = {}
    for who, session in zip(clients, sessions):
        rows = con.execute('select check_id, vl from grim_violations where session_id = ? order by occurred_at',
                           (session,)).fetchall()
        out[who] = [(checks.get(check, str(check)), vl) for check, vl in rows]
    con.close()
    return out


class Failed(Exception):
    pass


MESA_EGL = Path('/run/opengl-driver/share/glvnd/egl_vendor.d/50_mesa.json')


def screen_env(env):
    """Where the vanilla client's window goes. On Linux, the virtual screen (`vscreen`), never a real monitor,
    and Mesa's EGL: NVIDIA's crashed the client there (libEGL_nvidia). On macOS, the logged-in session."""
    if sys.platform == 'darwin' or not shutil.which('vscreen'):
        return env
    subprocess.run(['vscreen', 'up'], check=False, capture_output=True)
    text = subprocess.run(['vscreen', 'env'], capture_output=True, text=True, check=True).stdout
    env = dict(env)
    env.pop('DISPLAY', None)
    for match in re.finditer(r'(\w+)=(\S+)', text.split('unset')[0]):
        env[match.group(1)] = match.group(2).strip('"\'')
    if MESA_EGL.exists():
        env['__EGL_VENDOR_LIBRARY_FILENAMES'] = str(MESA_EGL)
    return env


class VanillaClient:
    """The vanilla 26.3 client, VanillaTickScript as its main class, joined to `address`."""

    def __init__(self, address, work, log):
        import vanilla_client
        classes = work / 'classes'
        classes.mkdir(parents=True)
        jars = os.pathsep.join(map(str, vanilla_client.classpath()))
        release = str(lab.config()[0]['minecraft']['java_major'])
        source = lab.ROOT / 'mods/sunrise/src/test/java/dev/sunriselab/parity/VanillaTickScript.java'
        subprocess.run(['javac', '--release', release, '-proc:none', '-nowarn', '-cp', jars, '-d', classes, source],
                       check=True, capture_output=True)
        args = argparse.Namespace(name=PLAYER, join=address, world=None, game_dir=work / 'client', assets=None,
                                  heap='2G', width=854, height=480, lan_port=None, backend=None)
        args.game_dir.mkdir()
        # 20 frames a second at least: a tick's lines then act in their own frame, as Sunrise's do. On a virtual
        # screen the frames are drawn on the CPU: the lightest graphics keep each well under a tick, or the client
        # runs its missed ticks in bursts and takes the server's packets on other ticks than a smooth one would.
        vanilla_client.write_options(args.game_dir, {
            'maxFps': '60', 'renderDistance': '2', 'graphicsMode': '0', 'renderClouds': '"false"', 'ao': 'false',
            'entityShadows': 'false', 'particles': '2', 'biomeBlendRadius': '0', 'mipmapLevels': '0'})
        command = vanilla_client.command(args)
        cp = command.index('-cp') + 1
        command[cp] = os.pathsep.join([str(classes), command[cp]])
        command[command.index('net.minecraft.client.main.Main')] = 'dev.sunriselab.parity.VanillaTickScript'
        self.log = open(log, 'w', encoding='utf-8')
        self.ready, self.done = threading.Event(), {}
        # Its window opens without the focus (SDL's hint), so it takes neither the keyboard nor the pointer of
        # whoever is at the computer; VanillaTickScript keeps their input out once it runs.
        env = dict(screen_env(vanilla_client.environment()), SDL_WINDOW_ACTIVATE_WHEN_SHOWN='0')
        self.process = vanilla_client.spawn(command, args.game_dir, env)
        threading.Thread(target=self.pump, daemon=True).start()

    def pump(self):
        for line in self.process.stdout:
            self.log.write(line)
            self.log.flush()
            if '[VanillaTickScript] ready' in line:
                self.ready.set()
            elif '[VanillaTickScript] done ' in line:
                self.done[line.split('[VanillaTickScript] done ')[1].strip()] = time.time()

    def play(self, path, name, timeout):
        self.process.stdin.write(f'run {path}\n')
        self.process.stdin.flush()
        deadline = time.time() + timeout
        while name not in self.done:
            if time.time() > deadline or self.process.poll() is not None:
                raise Failed(f'vanilla: scenario {name} never finished (see vanilla-client.log)')
            time.sleep(0.2)

    def close(self):
        self.process.terminate()
        try:
            self.process.wait(20)
        except subprocess.TimeoutExpired:
            self.process.kill()
        self.log.close()


class HeadlessGame(Game):
    """server_pack_e2e's game, `--headless`: no display, window or GPU; the frame and the ticks run as a
    window's do (game/runtime/headless.rs), and only the ticks' packets matter here."""

    def __init__(self, bins, resources, work, scripts, extras=None):
        self.home = work / 'home'
        for d in ('home', 'launcher', 'data', 'config'):
            (work / d).mkdir(parents=True)
        if extras:
            # The game folder is the home here (Game runs sunrise-ctl there): extras sit next to options.txt.
            (self.home / 'sunrise-extras.txt').write_text(extras)
        self.env = lab.game_env(SUNRISE_HOME=self.home, SUNRISE_LAUNCHER_HOME=work / 'launcher',
                                XDG_DATA_HOME=work / 'data', XDG_CONFIG_HOME=work / 'config',
                                SUNRISE_TELEMETRY_URL='off', SUNRISE_TICK_SCRIPT=scripts / 'scenario.json')
        self.base = [str(lab.binary(bins, 'sunrise-ctl')), '--socket', str(work / 'c.sock')]
        self.log = self.home / 'logs' / 'latest.log'
        headless = [] if os.environ.get('WIRE_STREAM_WINDOWED') == '1' else ['--headless']
        reply = self('launch', '--game', lab.binary(bins, 'sunrise-client'), *headless, '--resources', resources)
        self.pid = reply.get('pid')


class SunriseClient:
    def __init__(self, bins, resources, work, address, scripts, extras=None):
        self.scripts = scripts
        self.game = HeadlessGame(bins, resources, work, scripts, extras)
        self.game('connect', address, PLAYER)
        self.game('wait-for', 'loaded', '--timeout', 180)

    def play(self, path, name, timeout):
        target = self.scripts / 'scenario.json'
        done = target.with_suffix('.done')
        done.unlink(missing_ok=True)
        shutil.copy(path, target)
        target.with_suffix('.go').touch()
        deadline = time.time() + timeout
        while not done.exists():
            if time.time() > deadline:
                raise Failed(f'Sunrise: scenario {name} never finished (see sunrise-client.log)')
            time.sleep(0.2)

    def close(self):
        self.game('disconnect', ok=False)
        self.game.quit()


class ClientCounts:
    """How many teleports the client has accepted, and how many ticks it has ended, read from its capture as it
    grows (complete lines only)."""

    def __init__(self, capture):
        self.capture, self.offset, self.accepted, self.ticks = capture, 0, 0, 0

    def update(self):
        with open(self.capture, 'rb') as f:
            f.seek(self.offset)
            data = f.read()
        data = data[:data.rfind(b'\n') + 1]
        self.offset += len(data)
        for line in data.splitlines():
            if b'"c2s"' in line:
                e = json.loads(line)
                self.accepted += e.get('name') == 'accept_teleportation'
                self.ticks += e.get('name') == 'client_tick_end'
        return self


def settle(counts, accepted, timeout=120):
    """Wait until the client has accepted `accepted` teleports in all, then for SETTLE more of its ticks. The
    last command of a reset is a teleport, so the server has run them all by then; a busy server can take
    seconds to, and a teleport landing in a scenario's first ticks would change all of its moves."""
    deadline = time.time() + timeout
    while counts.update().accepted < accepted:
        if time.time() > deadline:
            raise Failed(f'{counts.capture.stem}: the client never accepted the reset\'s teleports')
        time.sleep(0.2)
    ticks = counts.ticks
    while counts.update().ticks < ticks + SETTLE:
        if time.time() > deadline:
            raise Failed(f'{counts.capture.stem}: the client stopped ticking')
        time.sleep(0.2)


def play_all(client, server, scenarios, scripts, sunrise, capture):
    counts = ClientCounts(capture)
    for name in scenarios:
        spec = SCENARIOS[name]
        # The place settles: blocks and items reach the client, the player stands still on the ground.
        # From the console `~` is the world spawn: the player teleports onto itself.
        # WIRE_STREAM_SERVER_TPS: the server answers within one of its (shorter) ticks, so an answer reaches both
        # clients on the same client tick instead of whichever one two independent 50 ms clocks happen to give.
        # The clients stay at 20 ticks a second: a server's client only ever slows down (getTickTargetMillis).
        # The server's floating kick counts its own ticks (80): at a faster tick rate a smash's or Wind Burst's
        # airtime would end in it, so these runs set allow-flight (server-side only; the clients send the same).
        tps = os.environ.get('WIRE_STREAM_SERVER_TPS')
        commands = ([f'tick rate {tps}'] if tps else []) + RESET + spec.get('setup', []) + [f'tp {PLAYER} {PLAYER}']
        accepted = counts.update().accepted + sum(c.startswith('tp ') for c in commands)
        for command in commands:
            server.console(command)
        settle(counts, accepted)
        path = scripts / f'{name}-{"sunrise" if sunrise else "vanilla"}.json'
        path.write_text(json.dumps(script(name, spec, sunrise)))
        client.play(path, name, timeout=60)
        time.sleep(0.5)


def run(args, out):
    lock, jars, libraries = base.verify_artifacts()
    deps = os.pathsep.join(map(str, [jars['server'], *libraries]))
    vanilla = ['java', '-Xmx1g', '-cp', deps, 'net.minecraft.server.Main', 'nogui']
    resources = lab.LOCAL / 'sunrise-client' / lab.VERSION
    work = Path(tempfile.mkdtemp(prefix='hz-wire-stream-'))
    print(f'scratch: {work}')
    for var, sub in (('SUNRISE_HOME', 'home'), ('SUNRISE_LAUNCHER_HOME', 'launcher'), ('XDG_DATA_HOME', 'data'),
                     ('XDG_CONFIG_HOME', 'config')):
        (work / 'env' / sub).mkdir(parents=True)
        os.environ[var] = str(work / 'env' / sub)
    os.environ.update(lab.game_env(SUNRISE_TELEMETRY_URL='off'))
    directory = work / 'server'
    directory.mkdir()
    port = free_port()
    props = dict(PROPERTIES, **{'server-port': str(port), 'server-ip': '127.0.0.1'})
    if os.environ.get('WIRE_STREAM_SERVER_TPS'):
        props['allow-flight'] = 'true'
    (directory / 'server.properties').write_text(''.join(f'{k}={v}\n' for k, v in props.items()),
                                                 encoding='iso-8859-1')
    (directory / 'eula.txt').write_text('eula=true\n')
    (directory / 'ops.json').write_text(json.dumps([{'uuid': str(lab.offline_uuid(PLAYER)), 'name': PLAYER,
                                                     'level': 4, 'bypassesPlayerLimit': False}]))
    scripts = work / 'scripts'
    scripts.mkdir()
    if os.environ.get('WIRE_STREAM_GRIM'):
        vanilla = grim_server(directory, jars)
    server = Server(vanilla, directory, 'server')
    captures = {}
    try:
        server.wait_for(r'Done \(', timeout=900)
        for who in args.clients:
            capture = out / f'{who}.jsonl'
            capture.touch()
            listen = free_port()
            # WIRE_STREAM_PHASE (seconds): the server's packets reach the client that long after each of its
            # tick-ends (net_capture's phase), so wire_replay.py --phase can give Sunrise them on the same ticks.
            phase = float(os.environ['WIRE_STREAM_PHASE']) if os.environ.get('WIRE_STREAM_PHASE') else None
            stop = net_capture.start_background(listen, f'127.0.0.1:{port}', capture, full=True, phase=phase)
            client = None
            try:
                if who != 'vanilla':
                    client = SunriseClient(Path(args.bin).resolve(), resources, work / who, f'127.0.0.1:{listen}',
                                           scripts, LEGACY_EXTRAS if who == 'sunrise-legacy' else None)
                else:
                    client = VanillaClient(f'127.0.0.1:{listen}', work / 'vanilla', out / 'vanilla-client.log')
                    if not client.ready.wait(900):
                        raise Failed('the vanilla client never got into the world')
                time.sleep(3)
                play_all(client, server, args.scenarios, scripts, who != 'vanilla', capture)
            finally:
                if client:
                    if who != 'vanilla' and client.game.log.exists():
                        shutil.copy(client.game.log, out / f'{who}-client.log')
                    client.close()
                time.sleep(2)
                stop.set()
            captures[who] = capture
    finally:
        server.stop()
        shutil.copy(server.log_path, out / server.log_path.name)
        grim_db = directory / 'plugins' / 'GrimAC' / 'data' / 'history.v1.db'
        if grim_db.exists():
            shutil.copy(grim_db, out / 'grim.db')
        if not args.keep:
            shutil.rmtree(work, ignore_errors=True)
    return captures


def anchored_diffs(v, h, ticks, anchor, problems, names=('vanilla', 'Sunrise')):
    """wire_stream.compare over the scenario's ticks. With an anchor (a frame packet the server's answer brings,
    as AcceptTeleportation), the two ticks that answer arrived in compare with each other
    (wire_stream.compare_answered). Numbers are vanilla's ticks."""
    if not anchor:
        return wire_stream.compare(v, h, ticks)
    av, ah = wire_stream.first_frame(v, anchor), wire_stream.first_frame(h, anchor)
    if av is None or ah is None:
        problems.append(f'  no {anchor}: {names[0]} at tick {av}, {names[1]} at tick {ah}')
        return wire_stream.compare(v, h, ticks)
    return wire_stream.compare_answered(v, h, ticks, av, ah)


def compare(captures, scenarios):
    """Each scenario's problems, the second client's stream against the first's (`captures` in that order)."""
    (ref, ref_name), (new, new_name) = [(who, CLIENTS[who]) for who in captures]
    runs = {}
    for who, capture in captures.items():
        entries = [e for e in net_capture.load(capture) if e.get('conn') == 1 and 'name' in e]
        found = wire_stream.scenarios(entries)
        if len(found) != len(scenarios):
            raise Failed(f'{who}: {len(found)} marked scenarios in the capture, expected {len(scenarios)}')
        runs[who] = dict(zip(scenarios, found))
    results = []
    for name in scenarios:
        v, h = runs[ref][name], runs[new][name]
        ticks = SCENARIOS[name]['ticks'] + PRELUDE
        problems = []
        for i, vt, ht in anchored_diffs(v, h, ticks, SCENARIOS[name].get('anchor'), problems, (ref_name, new_name)):
            problems.append(f'  tick {i}:\n  {ref_name}:\n{wire_stream.describe(vt)}\n  {new_name}:\n'
                            f'{wire_stream.describe(ht)}')
            problems += wire_stream.field_diff(vt, ht, (ref_name, new_name))
        # The server decides when keep-alives and chunk batches come; the others answer what the scenario did.
        frame = lambda run: [n for t in run[:ticks + 1] for n, _ in t['frame']  # noqa: E731
                             if n not in ('keep_alive', 'chunk_batch_received')]
        if frame(v) != frame(h):
            problems.append(f'  frame packets: {ref_name} {frame(v)}, {new_name} {frame(h)}')
        misplaced = [(i, t['misplaced']) for i, t in enumerate(h[:ticks + 1]) if t['misplaced']]
        if misplaced:
            problems.append(f'  {new_name} sent the server\'s answers inside a tick: {misplaced}')
        results.append((name, problems))
    return results


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--bin', default=str(lab.cargo_target() / 'fast'), help='sunrise-client and sunrise-ctl')
    parser.add_argument('--out', default=str(lab.LOCAL / 'wire-stream-parity'))
    parser.add_argument('--scenarios', default=','.join(SCENARIOS), type=lambda s: s.split(','))
    parser.add_argument('--clients', default='vanilla,sunrise', type=lambda s: s.split(','),
                        help='the reference client and the one compared with it, of vanilla, sunrise and '
                             'sunrise-legacy; one alone is only captured (a loaded machine whose vanilla client fails '
                             'to log in: rerun sunrise alone, copy an earlier vanilla.jsonl into --out, then '
                             '--compare-only)')
    parser.add_argument('--compare-only', action='store_true', help='compare the captures already in --out')
    parser.add_argument('--keep', action='store_true', help='keep the scratch folders')
    args = parser.parse_args()
    unknown = [s for s in args.scenarios if s not in SCENARIOS]
    if unknown:
        parser.error(f'unknown scenarios {unknown}; known: {", ".join(SCENARIOS)}')
    if len(args.clients) not in (1, 2) or len(set(args.clients)) != len(args.clients) \
            or not set(args.clients) <= set(CLIENTS):
        parser.error(f'--clients takes one or two of {", ".join(CLIENTS)}')
    if args.compare_only and len(args.clients) != 2:
        parser.error('--compare-only compares two clients: name both in --clients')
    out = Path(args.out)
    if args.compare_only:
        captures = {who: out / f'{who}.jsonl' for who in args.clients}
    else:
        shutil.rmtree(out, ignore_errors=True)
        out.mkdir(parents=True)
        try:
            captures = run(args, out)
        except Failed as e:
            sys.exit(f'FAIL: {e}')
        if len(captures) < 2:
            print(f'captured {", ".join(captures)} in {out}; nothing to compare it with')
            return
    try:
        results = compare(captures, args.scenarios)
    except Failed as e:
        sys.exit(f'FAIL: {e}')
    if (out / 'grim.db').exists():
        for who, violations in grim_violations(out / 'grim.db', args.clients).items():
            counts = {}
            for check, _ in violations:
                counts[check] = counts.get(check, 0) + 1
            print(f'Grim violations, {CLIENTS[who]}: {len(violations)} {counts}')
    print(f'\nserverbound play streams, {CLIENTS[args.clients[0]]} vs {CLIENTS[args.clients[1]]}; captures in {out}')
    for name, problems in results:
        print(f'  {"pass" if not problems else "FAIL"}  {name}')
        for p in problems:
            print(p)
    if any(p for _, p in results):
        sys.exit(1)


if __name__ == '__main__':
    main()
