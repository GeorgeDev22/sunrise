use super::*;
thread_local! { static ANSWER: std::cell::RefCell<Option<Vec<[f64; 6]>>> = const { std::cell::RefCell::new(None) }; }
struct Belt;
#[mod_api::sunrise_part]
impl mod_api::BlockPart for Belt {
    fn collision_shape(
        &self,
        context: &dyn LoopContext,
        event: &mut mod_api::CollisionShapeEvent<'_>,
    ) {
        assert_eq!(context.side(), mod_api::LoopSide::Server);
        assert_eq!(event.pos, mod_api::BlockPos::new(5, 64, 4));
        let query = event.query;
        assert_eq!(query.entity(), Some(7));
        assert_eq!(query.kind(), mod_api::CollisionEntityKind::Player);
        assert!(query.is_descending() && !query.is_placement());
        assert!(query.always_collide_with_fluid());
        assert!(query.is_holding_item("minecraft:stick"));
        assert!(query.is_above(&[[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]], [5, 64, 4], false));
        assert!(query.can_stand_on_fluid("minecraft:water", "minecraft:lava"));
        assert_eq!(*event.shape, None);
        *event.shape = ANSWER.take();
    }
}
#[test]
fn wire_loopback_collision_shape_carries_the_query_in_and_the_boxes_back() {
    mod_api::__private::initialize().unwrap();
    let _phase = mod_api::__private::PhaseOwner::scope(mod_api::__private::ModPhase::World);
    install_counter();
    let index = guest::add_handler(vec![guest::view::<dyn mod_api::BlockPart>(Arc::new(Belt))]);
    let proxy = WasmHandler {
        index,
        module: WasmMod {
            group: 0,
            subscribed: &[],
            listeners: &[],
            corrupt: false,
        },
    };
    let query =
        mod_api::CollisionQuery::new(Some(7), mod_api::CollisionEntityKind::Player, Some(65.0))
            .with_descending(true)
            .with_always_collide_with_fluid(true)
            .with_held_item("minecraft:stick".into())
            .with_standable_fluids(vec![Arc::<str>::from("minecraft:lava")].into());
    for answer in [
        None,
        Some(vec![]),
        Some(vec![[0.0, 0.0, 0.0, 1.0, 0.75, 1.0]; 64]),
    ] {
        ANSWER.set(answer.clone());
        let mut shape = None;
        let mut event = mod_api::CollisionShapeEvent {
            pos: mod_api::BlockPos::new(5, 64, 4),
            state: mod_api::BlockStateRecord::new(REGISTERED_BLOCK.get().unwrap(), 0).into(),
            query: &query,
            shape: &mut shape,
        };
        mod_api::BlockPart::collision_shape(&proxy, &Context, &mut event);
        assert_eq!(shape, answer);
    }
}
