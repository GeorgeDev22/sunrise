//! Native and guest codecs share entry data, optional facts, tags and the selected RNG.
use super::*;
use mod_api::{LootEntryData, LootTypeContext, ModLootCondition, ModLootFunction, RandomSource};
struct Codec;
impl ModLootFunction for Codec {
    fn validate(&self, data: &LootEntryData) -> Result<(), String> {
        data.value()["count"]
            .as_i64()
            .map(|_| ())
            .ok_or_else(|| "count: expected integer".into())
    }
    fn apply(
        &self,
        data: &LootEntryData,
        mut stack: mod_api::LootStack,
        context: &mut dyn LootTypeContext,
    ) -> Result<mod_api::LootStack, String> {
        let tag = context
            .tags()
            .key(&mod_api::Identifier::parse("test:ingredients").unwrap())
            .unwrap();
        assert_eq!(context.tags().items(tag).unwrap(), vec![stack.item]);
        assert_eq!(context.tags().is_in_tag(stack.item, tag), Some(true));
        stack.count =
            data.value()["count"].as_i64().unwrap() as i32 + context.random().next_int_bound(10);
        Ok(stack)
    }
}
impl ModLootCondition for Codec {
    fn validate(&self, data: &LootEntryData) -> Result<(), String> {
        <Self as ModLootFunction>::validate(self, data)
    }
    fn test(&self, _: &LootEntryData, context: &mut dyn LootTypeContext) -> Result<bool, String> {
        let snapshot = context.snapshot();
        assert_eq!(snapshot.queried_table_id, "minecraft:blocks/short_grass");
        let stack = snapshot.queried_stack.unwrap();
        assert_eq!(stack.count(), 3);
        assert_eq!(stack.damage(), 2);
        assert!(snapshot.place.is_none());
        let tag = context
            .tags()
            .key(&mod_api::Identifier::parse("test:ingredients").unwrap())
            .unwrap();
        assert_eq!(context.tags().items(tag).unwrap(), vec![stack.item()]);
        Ok(context.random().next_int_bound(10) < 5)
    }
}
struct Tags;
impl mod_api::Tags for Tags {
    fn key(&self, id: &mod_api::Identifier) -> Option<mod_api::TagKey> {
        mod_api::__private::tag_key(id)
    }
    fn is_in_tag(&self, item: mod_api::ItemKey, tag: mod_api::TagKey) -> Option<bool> {
        assert_eq!(item, REGISTERED_ITEM.get().unwrap());
        mod_api::__private::with_tag_name(tag, |name| Some(name? == "test:ingredients"))
    }
    fn items(&self, tag: mod_api::TagKey) -> Option<Vec<mod_api::ItemKey>> {
        mod_api::__private::with_tag_name(tag, |name| {
            Some(if name? == "test:ingredients" {
                vec![REGISTERED_ITEM.get().unwrap()]
            } else {
                vec![]
            })
        })
    }
}
struct Context(mod_api::LegacyRandomSource);
impl LootTypeContext for Context {
    fn tags(&self) -> &dyn mod_api::Tags {
        &Tags
    }
    fn random(&mut self) -> &mut dyn RandomSource {
        &mut self.0
    }
    fn snapshot(&self) -> mod_api::LootContext {
        let mut snapshot = mod_api::LootContext::new(0);
        snapshot.queried_table_id = "minecraft:blocks/short_grass".into();
        let mut stack = mod_api::ItemStack::new(REGISTERED_ITEM.get().unwrap(), 3).unwrap();
        stack.set_damage(2);
        snapshot.queried_stack = Some(stack);
        snapshot
    }
}
#[test]
fn registered_loot_codecs_match_native_and_guest_validation_tags_parameters_and_random() {
    mod_api::__private::initialize().unwrap();
    let _phase = mod_api::__private::PhaseOwner::scope(mod_api::__private::ModPhase::World);
    install_counter();
    for wasm in [false, true] {
        let handler = if wasm {
            let index = guest::add_handler(vec![
                guest::view::<dyn ModLootFunction>(Arc::new(Codec)),
                guest::view::<dyn ModLootCondition>(Arc::new(Codec)),
            ]);
            Some(Arc::new(WasmHandler {
                index,
                module: WasmMod {
                    group: 0,
                    subscribed: &[],
                    listeners: &[],
                    corrupt: false,
                },
            }))
        } else {
            None
        };
        let function: Arc<dyn ModLootFunction> = match &handler {
            Some(handler) => handler.clone(),
            None => Arc::new(Codec),
        };
        let condition: Arc<dyn ModLootCondition> = match &handler {
            Some(handler) => handler.clone(),
            None => Arc::new(Codec),
        };
        let good = LootEntryData::new(serde_json::json!({"type":"test:codec", "count":29, "ignored":[null, true, -7, {"text":"value"}]})).unwrap();
        let bad =
            LootEntryData::new(serde_json::json!({"type":"test:codec", "count":false})).unwrap();
        assert!(function.validate(&bad).unwrap_err().contains("count"));
        assert!(condition.validate(&bad).unwrap_err().contains("count"));
        function.validate(&good).unwrap();
        condition.validate(&good).unwrap();
        let mut context = Context(mod_api::LegacyRandomSource::new(123));
        let mut expected = mod_api::LegacyRandomSource::new(123);
        let stack = function
            .apply(
                &good,
                mod_api::LootStack::new(REGISTERED_ITEM.get().unwrap(), -7),
                &mut context,
            )
            .unwrap();
        assert_eq!(stack.count, 29 + expected.next_int_bound(10));
        assert_eq!(
            condition.test(&good, &mut context).unwrap(),
            expected.next_int_bound(10) < 5
        );
        assert_eq!(context.0.next_long(), expected.next_long());
    }
}
