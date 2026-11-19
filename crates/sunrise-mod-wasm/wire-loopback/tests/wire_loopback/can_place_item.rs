use super::*;
use mod_api::{BlockEntityPart, BlockPart, CanPlaceItemEvent, ItemStack, ModBlockEntity};

struct Filter;

#[mod_api::sunrise_part]
impl BlockPart for Filter {}

#[mod_api::sunrise_part]
impl BlockEntityPart for Filter {
    fn can_place_item(&self, event: &mut CanPlaceItemEvent<'_>) -> Option<bool> {
        assert_eq!(event.entity.id(), "wire_test:filter");
        assert_eq!(event.entity.data, [1]);
        assert_eq!(event.stack.item(), REGISTERED_ITEM.get().unwrap());
        match event.slot {
            0 | 1 => Some(event.slot == event.entity.data[0] as usize && event.stack.count() == 3),
            _ => None,
        }
    }
}

#[test]
fn generated_block_entity_admission_preserves_inputs_and_optional_answer() {
    struct Register;
    #[mod_api::__subscriptions]
    impl Hooks for Register {
        fn register(
            &self,
            registrar: &mut dyn mod_api::Registrar,
            _: &mod_api::RegistrationContext,
        ) {
            REGISTERED_ITEM.set(Some(registrar.item_key("minecraft:stone")));
            let kind = registrar.block_entity_type("filter").build();
            let handler = WasmHandler {
                index: guest::add_handler(vec![guest::view::<dyn BlockEntityPart>(Arc::new(
                    Filter,
                ))]),
                module: WasmMod {
                    subscribed: &[],
                    listeners: &[],
                    group: 0,
                    corrupt: false,
                },
            };
            registrar.block_entity_part(&kind, handler).unwrap();
        }
    }
    mod_api::__private::initialize().unwrap();
    let mods = [mod_api::ModEntry::new("wire_test", Box::new(Register))];
    mod_api::__private::install_parts(
        &mods,
        &mod_api::__private::part_tests::rule_schema(),
        |_| false,
    )
    .unwrap();
    mod_api::__private::resolve_items(["minecraft:stone"]).unwrap();
    mod_api::__private::resolve_block_entity_types(["wire_test:filter"], |_| Some(())).unwrap();
    mod_api::testing::with_mods(mods.into(), || {
        let _phase = mod_api::__private::PhaseOwner::scope(mod_api::__private::ModPhase::World);
        let mut entity = ModBlockEntity::new("wire_test:filter".into(), Default::default());
        entity.data = vec![1];
        let stack = ItemStack::new(REGISTERED_ITEM.get().unwrap(), 3).unwrap();
        for (slot, expected) in [(0, Some(false)), (1, Some(true)), (2, None)] {
            let mut allowed = None;
            mod_api::dispatch_can_place_item(&entity, slot, &stack, &mut allowed);
            assert_eq!(allowed, expected);
        }
    });
}
