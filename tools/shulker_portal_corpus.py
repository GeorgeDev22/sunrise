"""A shulker's box grows toward its open lid (`Shulker.makeBoundingBox`), so `Entity.checkInsideBlocks` finds a nether
portal block the lid opens into, beside the shell or above it. Three NoAI shells, each with a portal block where its
lid opens (placed `strict`, so no frame is needed and none breaks): one on a wall and one on the floor whose lids open
on a frozen tick, and a wall one that stays shut. A NoAI pig put into a portal block on the same tick is the control.
Each tick after the lids open records which of them still stand in the overworld: vanilla's shells cross the tick
after the pig, since `Shulker.tick` grows the box only after `super.tick()` has checked the blocks inside it."""
try:
    from .command_exec_corpus import cmd, setup
except ImportError:
    from command_exec_corpus import cmd, setup

# (name, UUID suffix, shell position, attach face, support block, portal block and axis, peek it is set to)
SHELLS = [
    ('wall', 1, (4, 201, 4), 4, (3, 201, 4), (5, 201, 4, 'z'), 100),
    ('floor', 2, (10, 201, 4), 0, (10, 200, 4), (10, 202, 4, 'x'), 100),
    ('closed', 3, (4, 201, 10), 4, (3, 201, 10), (5, 201, 10, 'z'), 0),
]
PIG = 4
PIG_AT = (12, 201, 10)
TICKS = 8


def uuid(n):
    return f'00000e74-0000-0000-0000-{n:012x}'


def shulker_portal():
    out = setup('/tick freeze', '/gamerule minecraft:spawn_mobs false', '/gamemode spectator',
                '/tp @s 8.5 215 8.5 0 0')
    out += [cmd('/forceload add 0 0 15 15', expect_result=1), {'op': 'settle'}]
    out += setup('/fill 0 199 0 15 199 15 stone', '/fill 0 200 0 15 210 15 air')
    for name, n, (x, y, z), face, support, (px, py, pz, axis), _ in SHELLS:
        out += [cmd(f'/setblock {support[0]} {support[1]} {support[2]} stone', expect_result=1),
                cmd(f'/setblock {px} {py} {pz} minecraft:nether_portal[axis={axis}] strict', expect_result=1),
                cmd(f'/summon minecraft:shulker {x}.5 {y} {z}.5 '
                    f'{{UUID:[I;3700,0,0,{n}],NoAI:1b,AttachFace:{face}b,Peek:0b}}', expect_result=1)]

    def where():
        # Whether each one still stands in its own cell of the overworld: what crosses goes on to wait for its exit in
        # Sunrise, where vanilla lands it at once, so the probe asks only where it left.
        return [cmd(f'/execute in minecraft:overworld if entity '
                    f'@e[type=minecraft:{kind},x={x},y={y},z={z},dx=0,dy=0,dz=0]')
                for kind, (x, y, z) in [('shulker', s[2]) for s in SHELLS] + [('pig', PIG_AT)]]

    # Shut, none is in a portal: two ticks change nothing.
    out += [cmd('/tick step 2', expect_result=1), {'op': 'tick', 'n': 2}, *where()]
    for name, n, *rest in SHELLS:
        if rest[-1]:
            out.append(cmd(f'/data merge entity {uuid(n)} {{Peek:{rest[-1]}b}}', expect_result=1))
    # The control: a pig put into a portal block on the same tick, whose box never changes.
    out += [cmd('/setblock {} {} {} minecraft:nether_portal[axis=x] strict'.format(*PIG_AT), expect_result=1),
            cmd(f'/summon minecraft:pig {PIG_AT[0]}.5 {PIG_AT[1]} {PIG_AT[2]}.5 '
                f'{{UUID:[I;3700,0,0,{PIG}],NoAI:1b,NoGravity:1b}}', expect_result=1)]
    for _ in range(TICKS):
        out += [cmd('/tick step 1', expect_result=1), {'op': 'tick', 'n': 1}, *where()]
    return out
