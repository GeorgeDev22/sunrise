# Items, recipes and loot

Pages for mods that work with items and the data around them: recipes and ingredients, enchantment and consume
effects, loot, data maps and conditional data.

## When a player's item is used up

Implement `Hooks::player_destroy_item` to observe the last item consumed by use or by an entity,
a completed food/drink use, or a durability break.
Its `PlayerDestroyItemEvent` contains `player` (UUID), `player_entity_id`, `original` (an immutable full
stack snapshot taken before consumption or wear), and `hand` (`Some(Hand::Main)`, `Some(Hand::Off)`,
or `None` for worn equipment and inventory ammunition). Count, pre-wear damage and component patches
are preserved; a returned bottle or bucket does not replace `original`.

The callback receives `HostSide`: the server sees its applied action, including its local host player,
and a client sees its own prediction or completed use. An equipment-break status packet still plays
its sound and particles, but does not send this notice: it has no original stack. Inventory updates
do not send notices either.

This is a notification after the change, so it cannot cancel the action. A mining break notifies
after the block's drops. A completed use notifies after consumption and before a remainder such as
an empty bottle enters the hand. Partial consumption, refused interactions, and creative use that
preserves the item do not notify. Equipping an item into an empty armour slot moves it rather than
consuming it.

See the [API reference](../reference/api.md) for the generated event and `Hooks` method.

## Custom enchantment effect kinds

Implement `Hooks::enchant_effect` to handle custom `minecraft:post_attack` effects
when a melee hit lands: a player hitting a mob or another player, or a mob
hitting a mob or a player with an enchanted weapon in its main hand. The hook
runs on the server for each matching entry, including each mob or player hit by
a sweep.

An entry dispatches when:

- The enchantment's `slots` include the main hand.
- `enchanted` is `attacker` and `affected` is `victim`.
- `effect.type` has a namespace other than `minecraft:`. A bare type name uses
  the `minecraft` namespace.
- The entry's `requirements` pass, judged with the hit's damage type.

The event provides the enchantment ID, effect kind, borrowed `Tag` payload,
enchantment level, the `attacker` and `victim` entities and the enchanted
`weapon` stack the attacker hit with, taken before the hit (it may have broken
since). The victim may have died from the hit. Damage the weapon through the attacker's main hand to charge durability.

Other components and attacker/victim pairs and entries nested inside
`minecraft:all_of` do not dispatch this hook. An unclaimed kind does
nothing. Payloads are converted to retained NBT when definitions load; invalid
payloads refuse the definition rather than failing during a hit.

## Registered potions

A client mod lists the registered potions with `ClientHost::potions()` (`Registries.POTION`), beside
`enchantments()`. Each `PotionInfo` carries the potion's registry id (`minecraft:long_swiftness`), in
registry order: vanilla's 46, from `minecraft:water` to `minecraft:infested`. Content mods cannot
declare potions yet, so none of theirs appear.

The read is `ClientSetup`: call it once a client mod is set up, not every frame. The game builds the
list once; each call returns a copy.

There is no potion stack API. To show a potion, splash potion, lingering potion or tipped arrow per
potion, build the stack and give it a `minecraft:potion_contents` component whose `potion` is the
id, through `ItemStack::set_components`, as a recipe viewer does:

```rust,ignore
for potion in host.potions() {
    let mut stack = ItemStack::new(splash_potion, 1)?;
    stack.set_components(Compound::from([(
        "minecraft:potion_contents".into(),
        Tag::compound([("potion", Tag::String(potion.id.as_str().into()))]),
    )]))?;
    entries.push(stack);
}
```

<!-- section-pages -->
