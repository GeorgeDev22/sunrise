struct TestTags;
impl engine::LootTags for TestTags {
    fn members(&self, _: &str, _: &str) -> Vec<String> {
        Vec::new()
    }
}
struct TagReader;
#[mod_api::__subscriptions]
impl Hooks for TagReader {
    fn register(&self, r: &mut dyn mod_api::Registrar, _: &mod_api::RegistrationContext) {
        r.register_loot_function_type(
            &mod_api::Identifier::parse("tag_reader:count").unwrap(),
            TagReader,
        );
        r.register_loot_condition_type(
            &mod_api::Identifier::parse("tag_reader:member").unwrap(),
            TagReader,
        );
    }
}
impl ModLootFunction for TagReader {
    fn validate(&self, _: &LootEntryData) -> Result<(), String> {
        Ok(())
    }
    fn apply(
        &self,
        _: &LootEntryData,
        mut stack: LootStack,
        context: &mut dyn LootTypeContext,
    ) -> Result<LootStack, String> {
        let tags = context.tags();
        let tag = tags
            .key(&mod_api::Identifier::parse("test:ingredients").unwrap())
            .unwrap();
        let members = tags.items(tag).unwrap();
        stack.count = if tags.is_in_tag(stack.item, tag) == Some(true) {
            assert_eq!(
                members
                    .iter()
                    .map(|key| mod_api::__private::item_key_id(*key).unwrap())
                    .collect::<Vec<_>>(),
                ["minecraft:potion", "minecraft:stick"]
            );
            2
        } else {
            assert!(members.is_empty());
            0
        };
        Ok(stack)
    }
}
impl mod_api::ModLootCondition for TagReader {
    fn validate(&self, _: &LootEntryData) -> Result<(), String> {
        Ok(())
    }
    fn test(&self, _: &LootEntryData, context: &mut dyn LootTypeContext) -> Result<bool, String> {
        let tags = context.tags();
        let tag = tags
            .key(&mod_api::Identifier::parse("test:ingredients").unwrap())
            .unwrap();
        let stick = mod_api::__private::item_key("minecraft:stick").unwrap();
        let holds = tags.is_in_tag(stick, tag).unwrap();
        let missing = tags
            .key(&mod_api::Identifier::parse("test:missing").unwrap())
            .unwrap();
        assert_eq!(tags.is_in_tag(stick, missing), Some(false));
        assert!(tags.items(missing).unwrap().is_empty());
        Ok(holds)
    }
}
#[test]
fn loot_types_read_current_membership_and_order_after_tag_reload() {
    use std::collections::HashMap;
    use sunrise_items::items::{Items, TagRegistry};
    super::super::prepare_stack_boundary_tests();
    let mut items = Items::for_tests(HashMap::new());
    with_mods(
        vec![ModEntry::new("tag_reader", Box::new(TagReader))].into(),
        || {
            super::super::prepare_stack_boundary_tests();
            mod_platform::__private::part_tests::install_parts_fixture(mod_api::__private::mods())
                .unwrap();
            super::super::resolve_stack_boundary_test_items();
            for (members, expected_count, expected_holds) in [
                (
                    vec!["minecraft:potion".to_owned(), "minecraft:stick".to_owned()],
                    2,
                    true,
                ),
                (Vec::new(), 0, false),
            ] {
                items.set_tags(
                    TagRegistry::Item,
                    HashMap::from([("test:ingredients".into(), members)]),
                );
                let mut value = stack();
                let mut holds = false;
                apply_type(
                    "tag_reader:count",
                    &serde_json::json!({}),
                    &context(),
                    &mut value,
                    &items,
                );
                test_type(
                    "tag_reader:member",
                    &serde_json::json!({}),
                    &context(),
                    &mut holds,
                    &items,
                );
                assert_eq!(value.count, expected_count);
                assert_eq!(holds, expected_holds);
            }
        },
    );
}
