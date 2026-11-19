//! Winning definitions for the synchronized registries that read world packs.
use super::{PACK_REGISTRIES, condition_filter, json_to_nbt, synchronized_entries};
use serde_json::Value;
use std::{collections::BTreeMap, path::Path};
use sunrise_core::{
    identifier::Identifier,
    nbt::{Compound, Tag},
};
use sunrise_items::{
    enchant::{Enchantments, HolderSetBinding},
    items::{Instrument, Items},
};
use sunrise_world::{item_defaults::ItemDefaults, loot::PackOverlay};

type Registry = BTreeMap<(String, String), Option<(Definition, bool)>>;

/// Validated once for the world session. A false condition suppresses the
/// winning entry, including any prepared or lower-priority definition.
#[derive(Default)]
pub struct PackRegistryDefinitions {
    entries: BTreeMap<&'static str, Registry>,
}

/// Keep the admitted item definitions alongside the exact wire encoding.
enum Definition {
    Encoded(Tag),
    Enchantment(Value),
    Instrument(Instrument, Tag),
}

impl PackRegistryDefinitions {
    /// RegistryDataLoader's collected errors: try every winning definition,
    /// then refuse the entire load and report each failure by registry and id.
    pub(super) fn load(
        resources: &Path,
        defaults: &ItemDefaults,
        overlay: &PackOverlay<'_>,
        read_errors: &BTreeMap<String, String>,
        text_registries: &crate::command::Registries,
        sound_exists: impl Fn(&str) -> bool,
    ) -> Result<Self, String> {
        let (definitions, admitted) = Self::stage(
            resources,
            defaults,
            overlay,
            read_errors,
            text_registries,
            sound_exists,
        );
        admitted.map(|()| definitions)
    }

    /// Keep replaced keys even on failure, so the caller can collect errors
    /// from other registry files without checking a losing definition.
    pub(super) fn stage(
        resources: &Path,
        defaults: &ItemDefaults,
        overlay: &PackOverlay<'_>,
        read_errors: &BTreeMap<String, String>,
        text_registries: &crate::command::Registries,
        sound_exists: impl Fn(&str) -> bool,
    ) -> (Self, Result<(), String>) {
        let mut definitions = Self::default();
        let mut errors = BTreeMap::<_, BTreeMap<_, _>>::new();
        for &registry in PACK_REGISTRIES {
            let kind = registry.trim_start_matches("minecraft:");
            let entries = definitions.entries.entry(registry).or_default();
            for (key, error) in read_errors {
                if let Some(id) = key.strip_prefix(kind).and_then(|id| id.strip_prefix('/')) {
                    if let Ok(id) = Identifier::parse(id) {
                        entries.insert((id.namespace().to_owned(), id.path().to_owned()), None);
                    }
                    errors
                        .entry(registry)
                        .or_default()
                        .insert(id.to_owned(), error.clone());
                }
            }
            for (id, files) in overlay.kind(kind) {
                if errors
                    .get(registry)
                    .is_some_and(|errors| errors.contains_key(id))
                {
                    continue;
                }
                let mut decode = || -> Result<(), String> {
                    let id = Identifier::parse(id).map_err(|error| error.to_string())?;
                    let key = (id.namespace().to_owned(), id.path().to_owned());
                    // Reserve the winner even if it fails admission or its condition is false.
                    let definition = entries.entry(key).or_insert(None);
                    let value: Value = serde_json::from_slice(PackOverlay::top(files))
                        .map_err(|error| error.to_string())?;
                    let file = format!("data/{}/{kind}/{}.json", id.namespace(), id.path());
                    if overlay.conditions.allows_file_isolated(&value, &file)? {
                        *definition = Some((
                            decode_definition(
                                registry,
                                &id.to_string(),
                                value,
                                text_registries,
                                &sound_exists,
                            )?,
                            true,
                        ));
                    }
                    Ok(())
                };
                if let Err(error) = decode() {
                    errors
                        .entry(registry)
                        .or_default()
                        .insert(id.to_owned(), error);
                }
            }
            if Self::includes_prepared_entries(registry) {
                let mut prepared = synchronized_entries(
                    &resources.join("data/minecraft"),
                    resources,
                    registry,
                    overlay,
                );
                // The pack winner (even if refused or skipped) hides resources below it.
                prepared.retain(|(key, _, _)| !entries.contains_key(key));
                condition_filter::filter(&mut prepared, false, overlay.conditions, kind);
                for (key, value, from_pack) in prepared {
                    let definition = if kind == "enchantment" {
                        Definition::Enchantment(value)
                    } else if from_pack {
                        let id = format!("{}:{}", key.0, key.1);
                        match decode_definition(
                            registry,
                            &id,
                            value,
                            text_registries,
                            &sound_exists,
                        ) {
                            Ok(definition) => definition,
                            Err(error) => {
                                errors.entry(registry).or_default().insert(id, error);
                                entries.insert(key, None);
                                continue;
                            }
                        }
                    } else {
                        let wire = json_to_nbt(&value);
                        match Instrument::decode(&value) {
                            Ok(instrument) => Definition::Instrument(instrument, wire),
                            Err(_) => Definition::Encoded(wire),
                        }
                    };
                    entries.insert(key, Some((definition, from_pack)));
                }
            }
        }
        if let Some(entries) = definitions.entries.get_mut("minecraft:enchantment") {
            let enchantments: Vec<_> = entries
                .iter_mut()
                .filter_map(|((namespace, path), entry)| {
                    let (Definition::Enchantment(value), from_pack) = entry.as_mut()? else {
                        return None;
                    };
                    Some((format!("{namespace}:{path}"), value, *from_pack))
                })
                .collect();
            let binding = enchantments
                .iter()
                .any(|(_, value, _)| HolderSetBinding::needed(value))
                .then(|| {
                    HolderSetBinding::load(
                        resources,
                        overlay,
                        defaults,
                        enchantments.iter().map(|(id, _, _)| id.clone()).collect(),
                    )
                });
            for (id, value, from_pack) in enchantments {
                let result = if let Some(binding) =
                    binding.as_ref().filter(|_| HolderSetBinding::needed(value))
                {
                    match binding {
                        Ok(binding) => binding.bind(&id, value).map_err(|error| error.to_string()),
                        Err(error) => Err(error.to_string()),
                    }
                } else {
                    Ok(())
                };
                // Holder binding may change the transmitted shape. Preflight
                // the exact JSON-to-NBT projection the registry packet uses.
                let result = result.and_then(|()| {
                    if from_pack {
                        check_network_nbt(&json_to_nbt(value))?;
                    }
                    Ok(())
                });
                if let Err(error) = result {
                    errors
                        .entry("minecraft:enchantment")
                        .or_default()
                        .insert(id, error);
                }
            }
        }
        if errors.is_empty() {
            return (definitions, Ok(()));
        }
        let mut report = String::from("failed to load world registry definitions:");
        for (registry, entries) in errors {
            report.push_str(&format!("\n{registry}:"));
            for (id, error) in entries {
                report.push_str(&format!("\n  {id}: {error}"));
            }
        }
        (definitions, Err(report))
    }

    pub(super) fn discard_replaced(
        &self,
        registry: &str,
        entries: &mut Vec<((String, String), Value, bool)>,
    ) {
        if let Some(pack) = self.entries.get(registry) {
            entries.retain(|(key, _, _)| !pack.contains_key(key));
        }
    }

    pub(super) fn includes_prepared_entries(registry: &str) -> bool {
        matches!(registry, "minecraft:enchantment" | "minecraft:instrument")
    }

    pub(super) fn append_encoded(
        &self,
        registry: &str,
        entries: &mut Vec<((String, String), Tag, bool)>,
    ) {
        if let Some(pack) = self.entries.get(registry) {
            entries.extend(pack.iter().filter_map(|(key, value)| {
                let (definition, from_pack) = value.as_ref()?;
                let tag = match definition {
                    Definition::Encoded(tag) | Definition::Instrument(_, tag) => tag.clone(),
                    Definition::Enchantment(json) => json_to_nbt(json),
                };
                Some((key.clone(), tag, *from_pack))
            }));
        }
    }

    /// Use the same selected definitions as the synchronized registry packets.
    pub fn install_items(
        &self,
        items: &mut Items,
        resources: &Path,
        overlay: &PackOverlay<'_>,
        defaults: &ItemDefaults,
    ) -> sunrise_items::Result<()> {
        let enchantments: Vec<_> = self
            .entries
            .get("minecraft:enchantment")
            .into_iter()
            .flatten()
            .filter_map(|((namespace, path), entry)| match entry {
                Some((Definition::Enchantment(value), _)) => {
                    Some((format!("{namespace}:{path}"), value))
                }
                _ => None,
            })
            .collect();
        let enchantments = Enchantments::load_selected(
            resources,
            overlay,
            defaults,
            enchantments.iter().map(|(id, value)| (id.as_str(), *value)),
        )?;
        let instruments = self
            .entries
            .get("minecraft:instrument")
            .into_iter()
            .flatten()
            .filter_map(|((namespace, path), entry)| match entry {
                Some((Definition::Instrument(instrument, _), _)) => {
                    Some((format!("{namespace}:{path}"), instrument.clone()))
                }
                _ => None,
            })
            .collect();
        items.install_registries(enchantments, instruments);
        Ok(())
    }
}

fn check_network_nbt(tag: &Tag) -> Result<(), String> {
    sunrise_protocol::buf::check_network_nbt(tag).map_err(|error| format!("network {error}"))
}

fn decode_definition(
    registry: &str,
    id: &str,
    value: Value,
    text_registries: &crate::command::Registries,
    sound_exists: &dyn Fn(&str) -> bool,
) -> Result<Definition, String> {
    let decode_text = |field: &str, value: &Value| {
        crate::text_grammar::build(value, text_registries, false)
            .ok_or_else(|| format!("{field}: invalid JSON text component"))
    };
    let string = |value: &str| Tag::String(value.into());
    let asset_id = |asset: &str| {
        // The render readers store the default namespace as a path only.
        if asset.contains(':') {
            string(asset)
        } else {
            string(&format!("minecraft:{asset}"))
        }
    };
    let encoded = match registry {
        "minecraft:dialog" => {
            use crate::dialog::{AfterAction, Dialog, Kind};
            let source = json_to_nbt(&value);
            let dialog = Dialog::decode_pack(&source, &value)
                .map_err(|field| format!("invalid dialog field {field}"))?;
            let mut fields = Compound::from([(
                "title".into(),
                json_to_nbt(&decode_text("title", &value["title"])?),
            )]);
            if dialog.external_title.is_some() && !value["external_title"].is_null() {
                fields.insert(
                    "external_title".into(),
                    json_to_nbt(&decode_text("external_title", &value["external_title"])?),
                );
            }
            for (name, enabled) in [
                ("pause", dialog.pause),
                ("can_close_with_escape", dialog.can_close_with_escape),
            ] {
                if !enabled {
                    fields.insert(name.into(), Tag::Byte(0));
                }
            }
            match dialog.after_action {
                AfterAction::Close => {}
                action => {
                    fields.insert(
                        "after_action".into(),
                        string(if matches!(action, AfterAction::None) {
                            "none"
                        } else {
                            "wait_for_response"
                        }),
                    );
                }
            }
            // Nested codecs remain #3067. Keep only the fields the dialog reader consumed.
            let (kind, children, exit, columns, width): (_, &[&str], _, _, _) = match dialog.kind {
                Kind::Notice(_) => ("notice", &["action"], false, None, None),
                Kind::Confirmation { .. } => ("confirmation", &["yes", "no"], false, None, None),
                Kind::MultiAction { exit, columns, .. } => (
                    "multi_action",
                    &["actions"],
                    exit.is_some(),
                    Some(columns),
                    None,
                ),
                Kind::ServerLinks {
                    exit,
                    columns,
                    button_width,
                } => (
                    "server_links",
                    &[],
                    exit.is_some(),
                    Some(columns),
                    Some(button_width),
                ),
                Kind::DialogList {
                    dialogs,
                    exit,
                    columns,
                    button_width,
                    ..
                } => {
                    fields.insert("dialogs".into(), dialogs);
                    (
                        "dialog_list",
                        &[],
                        exit.is_some(),
                        Some(columns),
                        Some(button_width),
                    )
                }
            };
            fields.insert("type".into(), string(&format!("minecraft:{kind}")));
            for (name, value) in [
                (
                    "columns",
                    columns.map(|n| n.get() as i32).filter(|n| *n != 2),
                ),
                ("button_width", width.filter(|n| *n != 150)),
            ] {
                if let Some(value) = value {
                    fields.insert(name.into(), Tag::Int(value));
                }
            }
            let keys = children
                .iter()
                .copied()
                .chain((!dialog.body.is_empty()).then_some("body"))
                .chain((!dialog.inputs.is_empty()).then_some("inputs"))
                .chain(exit.then_some("exit_action"));
            for name in keys {
                if let Some(value) = source.get(name) {
                    fields.insert(name.into(), value.clone());
                }
            }
            Tag::Compound(fields)
        }
        "minecraft:dimension_type" => {
            sunrise_world::dimension_type::DimensionFacts::decode_pack_definition(id, &value)?
        }
        "minecraft:enchantment" => {
            let mut value = sunrise_items::enchant::decode_pack_definition(value)?;
            value["description"] = decode_text("description", &value["description"])?;
            return Ok(Definition::Enchantment(value));
        }
        "minecraft:painting_variant" => {
            let painting = crate::entities::PaintingVariant::decode(id, &value)?;
            let mut fields = Compound::from([
                ("asset_id".into(), asset_id(&painting.asset)),
                ("width".into(), Tag::Int(painting.width)),
                ("height".into(), Tag::Int(painting.height)),
            ]);
            for (field, text) in [("title", painting.title), ("author", painting.author)] {
                if let Some(text) = text.filter(|value| !value.is_null()) {
                    fields.insert(field.into(), json_to_nbt(&decode_text(field, &text)?));
                }
            }
            Tag::Compound(fields)
        }
        "minecraft:instrument" => {
            let instrument = Instrument::decode(&value)?;
            let float = |v: f32| Tag::Float(v.to_bits());
            let sound = match instrument.inline_sound_range {
                None if !sound_exists(&instrument.sound) => {
                    return Err(format!("sound_event: unknown holder {}", instrument.sound));
                }
                None => string(&instrument.sound),
                Some(fixed) => {
                    let mut fields =
                        Compound::from([("sound_id".into(), string(&instrument.sound))]);
                    if let Some(range) = fixed {
                        fields.insert("range".into(), float(range));
                    }
                    Tag::Compound(fields)
                }
            };
            let mut fields = Compound::from([
                ("sound_event".into(), sound),
                ("use_duration".into(), float(instrument.use_duration)),
                ("range".into(), float(instrument.range)),
                (
                    "description".into(),
                    json_to_nbt(&decode_text("description", &value["description"])?),
                ),
            ]);
            if instrument.durability_damage != 0 {
                fields.insert(
                    "durability_damage".into(),
                    Tag::Int(instrument.durability_damage),
                );
            }
            let tag = Tag::Compound(fields);
            check_network_nbt(&tag)?;
            return Ok(Definition::Instrument(instrument, tag));
        }
        "minecraft:banner_pattern" => {
            let pattern = crate::banner::Pattern::decode(id, &value)?;
            Tag::compound([
                ("asset_id", asset_id(&pattern.asset)),
                ("translation_key", string(&pattern.translation_key)),
            ])
        }
        // WorldClock.DIRECT_CODEC is a unit map codec: extra keys are permitted.
        "minecraft:world_clock" => {
            if !value.is_object() {
                return Err("not a map".into());
            }
            json_to_nbt(&value)
        }
        _ => unreachable!("only selected registry families are decoded"),
    };
    check_network_nbt(&encoded)?;
    Ok(Definition::Encoded(encoded))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn enchantment(description: &str) -> Value {
        json!({
            "description": description, "weight": 1, "max_level": 1, "anvil_cost": 1,
            "min_cost": {"base": 1, "per_level_above_first": 0},
            "max_cost": {"base": 5, "per_level_above_first": 0},
            "supported_items": "minecraft:stick", "slots": ["hand"]
        })
    }

    #[test]
    fn a_set_object_reaches_clients_as_ids_and_plain_forms_are_untouched() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("items.json"),
            json!({"minecraft:stick": {}, "minecraft:iron_sword": {}, "test:custom": {}})
                .to_string(),
        )
        .unwrap();
        let mut sticky = enchantment("Sticky");
        sticky["supported_items"] = json!({"type": "neoforge:or",
            "values": ["#minecraft:swords", "minecraft:stick"]});
        sticky["primary_items"] = json!({"type": "neoforge:any"});
        sticky["exclusive_set"] = json!({"type": "neoforge:any"});
        let mut plain = enchantment("Plain");
        plain["supported_items"] = json!("#minecraft:swords");
        let files = [
            (
                "tags/minecraft:item/swords".into(),
                vec![
                    json!({"values": ["test:custom", "minecraft:iron_sword"]})
                        .to_string()
                        .into_bytes(),
                ],
            ),
            (
                "enchantment/test:sticky".into(),
                vec![sticky.to_string().into_bytes()],
            ),
            (
                "enchantment/test:plain".into(),
                vec![plain.to_string().into_bytes()],
            ),
        ];
        let defaults = sunrise_world::item_defaults::read(root.path()).unwrap();
        let packed = PackRegistryDefinitions::load(
            root.path(),
            &defaults,
            &PackOverlay::new(&files),
            &Default::default(),
            &Default::default(),
            |_| true,
        )
        .unwrap();
        let mut wire = Vec::new();
        packed.append_encoded("minecraft:enchantment", &mut wire);
        assert_eq!(wire[0].0.1, "plain");
        assert_eq!(wire[1].0.1, "sticky");
        let ids =
            |ids: &[&str]| Tag::List(ids.iter().map(|id| Tag::String((*id).into())).collect());
        for (field, expected) in [
            (
                "supported_items",
                ids(&["test:custom", "minecraft:iron_sword", "minecraft:stick"]),
            ),
            (
                "primary_items",
                ids(&["minecraft:iron_sword", "minecraft:stick", "test:custom"]),
            ),
            ("exclusive_set", ids(&["test:plain", "test:sticky"])),
        ] {
            assert_eq!(wire[1].1.get(field), Some(&expected));
        }
        assert_eq!(
            wire[0].1.get("supported_items"),
            Some(&Tag::String("#minecraft:swords".into()))
        );
    }

    #[test]
    fn bound_tag_conditions_select_the_same_item_definitions_for_gameplay_and_wire() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("items.json"),
            br#"{"minecraft:stick": {}}"#,
        )
        .unwrap();
        let defaults = sunrise_world::item_defaults::read(root.path()).unwrap();
        let mut enchantment = enchantment("Conditional");
        let lower_instrument = root.path().join("data/test/instrument");
        let lower_enchantment = root.path().join("data/test/enchantment");
        std::fs::create_dir_all(&lower_instrument).unwrap();
        std::fs::create_dir_all(&lower_enchantment).unwrap();
        std::fs::write(lower_instrument.join("conditional.json"),
            br#"{"sound_event":"test:lower","use_duration":3,"range":32,"description":"Lower horn"}"#).unwrap();
        std::fs::write(
            lower_enchantment.join("conditional.json"),
            enchantment.to_string(),
        )
        .unwrap();
        let guard = json!([{"type": "neoforge:tag_empty", "tag": "test:gate"}]);
        enchantment["neoforge:conditions"] = guard.clone();
        let files = [
            (
                "instrument/test:conditional".into(),
                vec![
                json!({"sound_event": {"sound_id": "test:horn"}, "use_duration": 5, "range": 64,
                    "description": "Conditional horn", "neoforge:conditions": guard})
                    .to_string().into_bytes()
            ],
            ),
            (
                "enchantment/test:conditional".into(),
                vec![enchantment.to_string().into_bytes()],
            ),
        ];
        for (tag, admitted) in [("test:unrelated", true), ("test:gate", false)] {
            let mut items = Items::for_tests(std::collections::HashMap::from([(
                tag.to_owned(),
                std::collections::HashSet::from(["minecraft:stick".to_owned()]),
            )]));
            let conditions =
                sunrise_world::conditions::LoadConditions::new([]).with_tags(items.bound_tags());
            let overlay = PackOverlay {
                conditions: &conditions,
                files: &files,
            };
            let packed = super::super::SyncData::stage_registry_definitions(
                root.path(),
                &defaults,
                &overlay,
                &Default::default(),
                &Default::default(),
                |_| true,
            )
            .unwrap();
            // Gameplay must not evaluate the definition conditions with this other context.
            packed
                .install_items(
                    &mut items,
                    root.path(),
                    &PackOverlay::new(&files),
                    &defaults,
                )
                .unwrap();
            assert_eq!(
                items
                    .instruments
                    .get("test:conditional")
                    .map(|i| i.sound.as_str()),
                admitted.then_some("test:horn")
            );
            assert_eq!(
                items.enchantments.get("test:conditional").is_some(),
                admitted
            );
            assert_eq!(
                items
                    .component_registry
                    .enchantments
                    .contains(&"test:conditional".to_owned()),
                admitted
            );
            for registry in ["minecraft:instrument", "minecraft:enchantment"] {
                let mut wire = Vec::new();
                packed.append_encoded(registry, &mut wire);
                assert_eq!(
                    wire.first().map(|((ns, id), _, from_pack)| (
                        ns.as_str(),
                        id.as_str(),
                        *from_pack
                    )),
                    admitted.then_some(("test", "conditional", true)),
                    "{registry}"
                );
            }
        }
    }

    #[test]
    fn every_bad_winning_definition_is_reported_in_one_error() {
        let mut oversized = enchantment("Oversized");
        // Each string fits modified UTF-8, but together they exceed the 2 MiB quota.
        oversized["description"] = json!(vec!["x".repeat(64_000); 33]);
        assert!(
            sunrise_core::nbt::write_network(&json_to_nbt(&oversized), Default::default()).is_ok()
        );
        let files = [
            (
                "dialog/test:broken".into(),
                vec![
                    br#"{"type":"notice","title":"Lower"}"#.to_vec(),
                    b"{".to_vec(),
                ],
            ),
            (
                "dialog/test:negative_columns".into(),
                vec![br#"{"type":"multi_action","title":"Buttons","actions":[{"label":"OK"}],"columns":-1}"#.to_vec()],
            ),
            (
                "dialog/test:wrong_columns".into(),
                vec![br#"{"type":"dialog_list","title":"Buttons","dialogs":[],"columns":true}"#.to_vec()],
            ),
            (
                "dialog/test:wide_button".into(),
                vec![br#"{"type":"server_links","title":"Links","button_width":1025}"#.to_vec()],
            ),
            (
                "dialog/test:wrong_pause".into(),
                vec![br#"{"type":"notice","title":"Pause","pause":1}"#.to_vec()],
            ),
            (
                "dialog/test:wrong_action".into(),
                vec![br#"{"type":"notice","title":"Action","after_action":"stay"}"#.to_vec()],
            ),
            (
                "dialog/test:condition_false".into(),
                vec![br#"{"neoforge:conditions":[{"type":"neoforge:never"}]}"#.to_vec()],
            ),
            (
                "dimension_type/test:wrong_scale".into(),
                vec![br#"{"min_y":0,"height":64,"logical_height":64,"has_skylight":true,"has_ceiling":false,"has_ender_dragon_fight":false,"infiniburn":["minecraft:netherrack"],"coordinate_scale":"one","ambient_light":0,"monster_spawn_light_level":0,"monster_spawn_block_light_limit":0}"#.to_vec()],
            ),
            (
                "dimension_type/test:unknown_attribute".into(),
                vec![br#"{"min_y":0,"height":64,"logical_height":64,"has_skylight":true,"has_ceiling":false,"has_ender_dragon_fight":false,"infiniburn":["minecraft:netherrack"],"coordinate_scale":1,"ambient_light":0,"monster_spawn_light_level":0,"monster_spawn_block_light_limit":0,"attributes":{"minecraft:visual/not_an_attribute":1}}"#.to_vec()],
            ),
            (
                "dimension_type/test:empty_bed_rule".into(),
                vec![br#"{"min_y":0,"height":64,"logical_height":64,"has_skylight":true,"has_ceiling":false,"has_ender_dragon_fight":false,"infiniburn":["minecraft:netherrack"],"coordinate_scale":1,"ambient_light":0,"monster_spawn_light_level":0,"monster_spawn_block_light_limit":0,"attributes":{"minecraft:gameplay/bed_rule":{}}}"#.to_vec()],
            ),
            ("enchantment/test:not_read".into(), vec![b"{}".to_vec()]),
            ("enchantment/test:wrong_level".into(), vec![br#"{"max_level":"seven"}"#.to_vec()]),
            (
                "enchantment/test:unencodable".into(),
                vec![enchantment(&"\0".repeat(32_768)).to_string().into_bytes()],
            ),
            (
                "enchantment/test:oversized".into(),
                vec![oversized.to_string().into_bytes()],
            ),
            ("painting_variant/test:numeric_style".into(), vec![br#"{"asset_id":"test:mural","width":1,"height":1,"title":{"text":"No","bold":1}}"#.to_vec()]),
            ("painting_variant/test:numeric_object".into(), vec![br#"{"asset_id":"test:mural","width":1,"height":1,"author":{"object":7,"sprite":"test:icon"}}"#.to_vec()]),
            ("painting_variant/test:wrong_atlas".into(), vec![br#"{"asset_id":"test:mural","width":1,"height":1,"author":{"object":"atlas","atlas":7,"sprite":"test:icon","player":"Steve"}}"#.to_vec()]),
            ("painting_variant/test:legacy_click".into(), vec![br#"{"asset_id":"test:mural","width":1,"height":1,"title":{"text":"Label","hover_event":{"action":"show_text","value":{"text":"Click","click_event":{"action":"run_command","value":"/help"}}}}}"#.to_vec()]),
            (
                "painting_variant/test:bad".into(),
                vec![br#"{"asset_id":"test:mural","width":17,"height":1}"#.to_vec()],
            ),
            (
                "instrument/test:bad".into(),
                vec![br#"{"sound_event":"minecraft:item.goat_horn.sound.0","use_duration":-1,"range":128,"description":"Broken"}"#.to_vec()],
            ),
            (
                "instrument/test:wrong_sound_range".into(),
                vec![br#"{"sound_event":{"sound_id":"test:sound","range":"near"},"use_duration":4,"range":128,"description":"Horn"}"#.to_vec()],
            ),
            (
                "banner_pattern/test:bad".into(),
                vec![br#"{"asset_id":"test:banner","translation_key":7}"#.to_vec()],
            ),
            (
                "banner_pattern/test:unencodable".into(),
                vec![serde_json::json!({"asset_id": "test:banner", "translation_key": "\0".repeat(32_768)})
                    .to_string()
                    .into_bytes()],
            ),
        ];
        let io = BTreeMap::from([("enchantment/test:not_read".into(), "read failed".into())]);
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("items.json"), b"{}").unwrap();
        let instruments = root.path().join("data/test/instrument");
        std::fs::create_dir_all(&instruments).unwrap();
        std::fs::write(
            instruments.join("resource_duration.json"),
            br#"{"sound_event":"test:horn","use_duration":"four","range":128,"description":"Broken resource horn"}"#,
        )
        .unwrap();
        let defaults = sunrise_world::item_defaults::read(root.path()).unwrap();
        let Err(error) = super::super::SyncData::stage_registry_definitions(
            root.path(),
            &defaults,
            &PackOverlay::new(&files),
            &io,
            &Default::default(),
            |_| true,
        ) else {
            panic!("world registry load accepted invalid entries");
        };
        for (registry, defs) in [
            (
                "dialog",
                &[
                    ("broken", ""),
                    ("negative_columns", "columns"),
                    ("wrong_columns", "columns"),
                    ("wide_button", "button_width"),
                    ("wrong_pause", "pause"),
                    ("wrong_action", "after_action"),
                ][..],
            ),
            (
                "dimension_type",
                &[
                    ("wrong_scale", "coordinate_scale"),
                    ("empty_bed_rule", "bed_rule"),
                    ("unknown_attribute", "minecraft:visual/not_an_attribute"),
                ],
            ),
            (
                "enchantment",
                &[
                    ("not_read", "read failed"),
                    ("oversized", "byte limit exceeded"),
                    ("wrong_level", "max_level"),
                    ("unencodable", "modified UTF-8"),
                ],
            ),
            (
                "painting_variant",
                &[
                    ("numeric_style", "title"),
                    ("numeric_object", "author"),
                    ("wrong_atlas", "author"),
                    ("legacy_click", "title"),
                    ("bad", "width"),
                ],
            ),
            (
                "instrument",
                &[
                    ("bad", "use_duration"),
                    ("resource_duration", "use_duration"),
                    ("wrong_sound_range", "sound_event.range"),
                ],
            ),
            (
                "banner_pattern",
                &[
                    ("bad", "translation_key"),
                    ("unencodable", "modified UTF-8"),
                ],
            ),
        ] {
            let header = format!("\nminecraft:{registry}:\n");
            let group = error
                .split_once(&header)
                .unwrap()
                .1
                .split("\nminecraft:")
                .next()
                .unwrap();
            for (id, field) in defs {
                let entry = format!("  test:{id}:");
                assert!(
                    group
                        .lines()
                        .any(|line| line.starts_with(&entry) && line.contains(field)),
                    "{error}"
                );
            }
        }
        assert!(!error.contains("condition_false"));
    }
}
