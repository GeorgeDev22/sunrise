use super::{block_entity_id, string, uuid_tag};
use crate::{
    beehive::BeehiveExt,
    mobs::{spawner::SpawnerExt, trial_spawner::TrialSpawnerExt},
};
use sunrise_core::nbt::{Compound, Tag};

/// Save block-entity stacks against the world's loaded item prototypes.
pub fn encode(
    p: (i32, i32, i32),
    e: &crate::containers::BlockEntity,
    block: &str,
    items: &crate::items::Items,
) -> Tag {
    encode_with(p, e, block, items, |_, _| None)
}

/// Merge modeled stacks with saved rows the engine did not take ownership of.
pub fn mod_items(
    m: &crate::containers::ModBlockEntity,
    items: &crate::items::Items,
) -> Option<Tag> {
    let mut entries = sunrise_items::item_nbt::encode_items_using(&m.items, |stack| {
        sunrise_items::item_nbt::item_to_nbt(stack, items)
    });
    match m.nbt.get(&"Items".into()) {
        Some(Tag::List(raw)) => {
            entries.extend(raw.iter().cloned());
            Some(Tag::List(entries))
        }
        // A non-list is opaque; unoccupied modeled slots introduce no field.
        raw if entries.is_empty() => raw.cloned(),
        _ => Some(Tag::List(entries)),
    }
}

/// Encode a live block entity, allowing its owner to supply the mod payload
/// before the engine adds identity, position, item slots and published shapes.
pub fn encode_with(
    p: (i32, i32, i32),
    e: &crate::containers::BlockEntity,
    block: &str,
    items: &crate::items::Items,
    save_additional: impl FnOnce(&str, &Compound) -> Option<Compound>,
) -> Tag {
    let item_to_nbt =
        |stack: &crate::items::ItemStack| sunrise_items::item_nbt::item_to_nbt(stack, items);
    use crate::containers::BlockEntity;
    let encode_items = |slots: &[Option<crate::items::ItemStack>]| {
        Tag::List(sunrise_items::item_nbt::encode_items_using(
            slots,
            item_to_nbt,
        ))
    };
    if let BlockEntity::Mod(m) = e {
        let mut map = save_additional(m.id(), &m.nbt).unwrap_or_else(|| m.nbt.clone());
        if let Some((key, shapes)) = m.shapes_tag() {
            map.insert(key.into(), shapes);
        }
        map.insert("id".into(), string(m.id()));
        if !m.items.is_empty() {
            let key = "Items".into();
            if let Some(items) = mod_items(m, items) {
                map.insert(key, items);
            } else {
                map.remove(&key);
            }
        }
        for (key, v) in [("x", p.0), ("y", p.1), ("z", p.2)] {
            map.insert(key.into(), Tag::Int(v));
        }
        return Tag::Compound(map);
    }
    let components = match e {
        BlockEntity::Banner(b) => b.components.clone(),
        BlockEntity::CopperGolemStatue(components) => components.clone(),
        _ => Compound::new(),
    };
    let mut entries = vec![
        ("id", string(block_entity_id(e, block))),
        ("x", Tag::Int(p.0)),
        ("y", Tag::Int(p.1)),
        ("z", Tag::Int(p.2)),
        ("keepPacked", Tag::Byte(0)),
        ("components", Tag::Compound(components)),
    ];
    match e {
        // EnderChestBlockEntity saves nothing of its own: the items are the player's.
        BlockEntity::Chest(_) if block == "minecraft:ender_chest" => {}
        BlockEntity::Chest(items) => {
            let list = encode_items(items);
            // ShulkerBoxBlockEntity.saveAdditional: `saveAllItems(.., false)`
            // writes no list for an empty box; other containers always do.
            let empty = matches!(&list, Tag::List(l) if l.is_empty());
            if !(empty && block.ends_with("shulker_box")) {
                entries.push(("Items", list));
            }
        }
        BlockEntity::Furnace(f) => {
            entries.push(("Items", encode_items(&f.items)));
            entries.push(("lit_time_remaining", Tag::Int(f.lit_remaining)));
            entries.push(("lit_total_time", Tag::Int(f.lit_total)));
            entries.push(("cooking_time_spent", Tag::Int(f.cooking_timer)));
            entries.push(("cooking_total_time", Tag::Int(f.cooking_total)));
            entries.push(("speed_multiplier", Tag::Float(f.speed.to_bits())));
            let used: Compound = f
                .recipes_used
                .iter()
                .map(|(id, count)| (id.as_str().into(), Tag::Int(*count as i32)))
                .collect();
            entries.push(("RecipesUsed", Tag::Compound(used)));
        }
        BlockEntity::Brewing(b) => {
            entries.push(("Items", encode_items(&b.items)));
            entries.push(("BrewTime", Tag::Int(b.brew_time)));
            entries.push(("total_brew_time", Tag::Int(b.total_brew_time)));
            entries.push(("Fuel", Tag::Int(b.fuel)));
            entries.push(("total_fuel", Tag::Int(b.total_fuel)));
            entries.push(("speed_multiplier", Tag::Float(b.speed.to_bits())));
        }
        BlockEntity::Sign(sign) => entries.extend(sign.encode()),
        BlockEntity::Jukebox(j) => {
            if let Some(record) = j.record() {
                entries.push(("RecordItem", item_to_nbt(record)));
            }
            if j.song_clock_present {
                entries.push(("ticks_since_song_started", Tag::Long(j.ticks)));
            }
        }
        BlockEntity::Beacon(b) => {
            if let Some(p) = &b.primary {
                entries.push(("primary_effect", string(p)));
            }
            if let Some(s) = &b.secondary {
                entries.push(("secondary_effect", string(s)));
            }
            entries.push(("Levels", Tag::Int(b.levels)));
        }
        BlockEntity::Lectern(l) => {
            if let Some(book) = &l.book {
                entries.push(("Book", item_to_nbt(book)));
                entries.push(("Page", Tag::Int(l.page)));
            }
        }
        BlockEntity::Sculk(s) => entries.extend(s.to_nbt()),
        BlockEntity::Shelf(shelf) => {
            entries.push(("Items", encode_items(&shelf.items)));
            entries.push((
                "align_items_to_bottom",
                Tag::Byte(i8::from(shelf.align_to_bottom)),
            ));
        }
        // ChiseledBookShelfBlockEntity.saveAdditional.
        BlockEntity::ChiseledBookshelf(b) => {
            entries.push(("Items", encode_items(&b.items)));
            entries.push(("last_interacted_slot", Tag::Int(b.last_slot)));
        }
        // CampfireBlockEntity.saveAdditional.
        BlockEntity::Campfire(c) => {
            entries.push(("Items", encode_items(&c.items)));
            entries.push(("CookingTimes", Tag::IntArray(c.progress.to_vec())));
            entries.push(("CookingTotalTimes", Tag::IntArray(c.total.to_vec())));
        }
        BlockEntity::Crafter(c) => {
            entries.push(("Items", encode_items(&c.items)));
            entries.push((
                "disabled_slots",
                Tag::IntArray(
                    (0..9)
                        .filter(|&i| c.disabled[i])
                        .map(|i| i as i32)
                        .collect(),
                ),
            ));
            entries.push(("triggered", Tag::Int(c.triggered)));
            entries.push(("crafting_ticks_remaining", Tag::Int(c.crafting_ticks)));
        }
        BlockEntity::Banner(b) => {
            if !b.layers.is_empty() {
                entries.push(("patterns", crate::banner::layers_to_nbt(&b.layers)));
            }
            if let Some(name) = &b.name {
                entries.push(("CustomName", string(name)));
            }
        }
        BlockEntity::CreakingHeart(h) => {
            if let Some((_, uuid)) = h.creaking {
                entries.push(("creaking", uuid_tag(uuid)));
            }
        }
        BlockEntity::Skull(s) => {
            if let Some(profile) = &s.profile {
                entries.push(("profile", profile.clone()));
            }
            if let Some(sound) = &s.note_block_sound {
                entries.push(("note_block_sound", string(sound)));
            }
            if let Some(name) = &s.custom_name {
                entries.push(("custom_name", name.clone()));
            }
        }
        // TheEndGatewayBlockEntity.saveAdditional.
        BlockEntity::EndGateway(g) => {
            entries.push(("Age", Tag::Long(g.age)));
            if let Some(exit) = g.exit_portal {
                entries.push(("exit_portal", Tag::IntArray(exit.to_vec())));
            }
            if g.exact_teleport {
                entries.push(("ExactTeleport", Tag::Byte(1)));
            }
        }
        BlockEntity::Spawner(s) => entries.extend(s.save()),
        BlockEntity::TrialSpawner(s) => entries.extend(s.save()),
        BlockEntity::Vault(v) => entries.extend(v.save(items)),
        BlockEntity::Beehive(h) => entries.extend(h.to_nbt()),
        BlockEntity::CommandBlock(c) => entries.extend(c.save()),
        BlockEntity::StructureBlock(b) => entries.extend(b.save()),
        BlockEntity::JigsawBlock(b) => entries.extend(b.save()),
        BlockEntity::TestBlock(b) => entries.extend(b.save()),
        BlockEntity::TestInstanceBlock(b) => entries.extend(b.save()),
        BlockEntity::Brushable(b) => {
            if let Some(table) = &b.loot_table {
                entries.push(("LootTable", table.clone()));
                if let Some(seed) = &b.table_seed_tag {
                    entries.push(("LootTableSeed", seed.clone()));
                }
            } else if let Some(item) = &b.item {
                entries.push(("item", item_to_nbt(item)));
            }
        }
        // ConduitBlockEntity.saveAdditional: EntityReference.store.
        BlockEntity::Conduit(target) => {
            if let Some(uuid) = target {
                entries.push(("Target", Tag::IntArray(uuid.to_vec())));
            }
        }
        // Encoded above; an enchanting table's CustomName is the block name.
        BlockEntity::DaylightDetector
        | BlockEntity::Bell
        | BlockEntity::EndPortal
        | BlockEntity::EnchantingTable
        | BlockEntity::CopperGolemStatue(_)
        | BlockEntity::Mod(_) => {}
        // PotentSulfurBlockEntity.saveAdditional.
        BlockEntity::PotentSulfur(sulfur) => {
            entries.push(("countdown", Tag::Int(sulfur.waiting_countdown)))
        }
    }
    Tag::compound(entries)
}
