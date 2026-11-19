//! #861 part 1 through the game's own hooks: a client mod reads the keys of a block state's baked
//! models and copies of their quads in `model_baking_completed`, and a reload supersedes them.
use super::model_baking_completed_tests::reload;
use super::native_entity_refs::tests::with_client_game;
use mod_api::{
    BakedModelKey, BakedModelView, BlockStateId, ClientHost, Hooks, ModEntry,
    ModelBakingCompletedEvent, ModelError,
};
use mod_platform::testing::with_mods;
use std::sync::{Arc, Mutex};

/// What the mod read in each `model_baking_completed`: stone's keys and their copies.
#[derive(Default)]
struct Read {
    state: Option<BlockStateId>,
    keys: Vec<Result<Vec<BakedModelKey>, ModelError>>,
    views: Vec<Vec<Result<BakedModelView, ModelError>>>,
    past_the_table: Vec<Result<Vec<BakedModelKey>, ModelError>>,
}
struct Reader(Arc<Mutex<Read>>);
#[mod_api::__subscriptions]
impl Hooks for Reader {
    fn model_baking_completed(&self, host: &mut dyn ClientHost, _: &mut ModelBakingCompletedEvent) {
        let mut read = self.0.lock().unwrap();
        let state = read.state.expect("the test names stone's state");
        let assets = host.render_assets();
        let keys = assets.block_state_models(state);
        let views = keys
            .iter()
            .flatten()
            .map(|key| assets.baked_model(*key))
            .collect();
        read.past_the_table
            .push(assets.block_state_models(BlockStateId(u32::MAX)));
        read.keys.push(keys);
        read.views.push(views);
    }
}

/// Stone's four weighted variants (26.3's blockstate lists four models, two of them turned half a
/// way round), each a full cube of six quads: read on the first load and again after a reload; the
/// first load's key answers `StaleModel` afterwards and the untouched model reads the same.
#[test]
#[ignore = "daily"]
fn a_client_mod_reads_a_block_states_baked_models_once_per_load() {
    let read = Arc::new(Mutex::new(Read::default()));
    let mods = vec![ModEntry::new("reader", Box::new(Reader(Arc::clone(&read))))].into();
    with_mods(mods, || {
        with_client_game(|game| {
            let stone = game
                .pack
                .assets
                .registry
                .default_state("minecraft:stone")
                .expect("stone");
            read.lock().unwrap().state = Some(BlockStateId(stone.0));
            game.publish_initial_client_models().unwrap();
            let first = {
                let read = read.lock().unwrap();
                let Ok(keys) = &read.keys[0] else {
                    panic!("stone's keys: {:?}", read.keys);
                };
                assert_eq!(keys.len(), 4, "stone has four weighted variants");
                let Ok(view) = &read.views[0][0] else {
                    panic!("stone's model: {:?}", read.views);
                };
                assert_eq!(view.quads.len(), 6, "a full cube");
                assert!(
                    view.quads
                        .iter()
                        .all(|q| q.sprite == "minecraft:block/stone")
                );
                assert_eq!(read.past_the_table, [Err(ModelError::UnknownModel)]);
                (keys[0], view.clone())
            };
            reload(game);
            let read = read.lock().unwrap();
            let Ok(keys) = &read.keys[1] else {
                panic!("stone's keys after reload: {:?}", read.keys);
            };
            assert_ne!(keys[0], first.0, "a new generation's key");
            assert_eq!(
                read.views[1][0].as_ref(),
                Ok(&first.1),
                "untouched: the same quads"
            );
            assert_eq!(
                game.pack.render.baked_model(first.0),
                Err(ModelError::StaleModel),
                "the first load's key is superseded"
            );
            assert_eq!(game.pack.render.baked_model(keys[0]).as_ref(), Ok(&first.1));
        });
    });
}
