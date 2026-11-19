use super::*;

#[test]
fn wire_loopback_attach_mob_effect_part_answers_invalid_key_for_unresolved_effect_key() {
    mod_api::__private::initialize().unwrap();
    let _phase = mod_api::__private::PhaseOwner::scope(mod_api::__private::ModPhase::Registration);
    thread_local! {
        static ATTACHED: std::cell::RefCell<Option<(mod_api::EffectKey, Vec<u8>)>> =
            const { std::cell::RefCell::new(None) };
    }
    // The wasm guest's registration, served at the boundary: the key request and the
    // attachment both cross the generated host imports into the real registrar.
    struct BadKeyOwner;
    #[mod_api::__subscriptions]
    impl Hooks for BadKeyOwner {
        fn register(
            &self,
            registrar: &mut dyn mod_api::Registrar,
            _: &mod_api::RegistrationContext,
        ) {
            let module = WasmMod {
                group: 0,
                subscribed: &[],
                listeners: &[],
                corrupt: false,
            };
            let mut cx = Box::new(boundary::Cx::default());
            cx.module = &module;
            let mut frame = boundary::Frame::default();
            frame.set_registrar_mut(registrar);
            cx.frames.push(frame);
            let cx = &mut *cx as *mut boundary::Cx;
            let _resources = unsafe { boundary::resource_scope(cx, false) };
            let mut request = Writer::new();
            request.put(&"Bad Id");
            let mut key = Writer::new();
            let id = boundary::import_id("Registrar", "effect_key").unwrap();
            unsafe { boundary::serve(id, cx, request.bytes(), &mut key) }.unwrap();
            // The guest carries the host's opaque token; read it as its raw slot.
            let slot: u32 = Reader::decode(key.bytes()).unwrap();
            let effect = mod_api::__private::parts_installation::effect_key(slot);
            let mut input = Writer::new();
            input.put(&slot);
            input.put(&"wire_test::Part");
            input.put(&guest::add_handler(vec![guest::view::<
                dyn mod_api::MobEffectPart,
            >(Arc::new(
                apply_effect_tick_hook::ApplyEffectTickHooks,
            ))]));
            let mut out = Writer::new();
            let id = boundary::import_id("Registrar", "attach_mob_effect_part").unwrap();
            unsafe { boundary::serve(id, cx, input.bytes(), &mut out) }.unwrap();
            ATTACHED.set(Some((effect, out.into_bytes())));
        }
    }
    let mods = [mod_api::ModEntry::new("wire_test", Box::new(BadKeyOwner))];
    let refused = mod_api::__private::install_parts(
        &mods,
        &mod_api::__private::part_tests::rule_schema(),
        |_| false,
    )
    .unwrap_err();
    let (effect, answer) = ATTACHED
        .take()
        .expect("the guest's attachment reached the host");
    assert_eq!(
        effect,
        mod_api::__private::parts_installation::effect_key(u32::MAX)
    );
    let mut expected = Writer::new();
    expected.put(&Result::<(), mod_api::MobEffectPartError>::Err(
        mod_api::MobEffectPartError::InvalidKey { effect },
    ));
    assert_eq!(
        answer,
        expected.bytes(),
        "the attachment answers InvalidKey"
    );
    assert!(
        refused
            .iter()
            .any(|error| error.contains("attached wire_test::Part to an invalid effect key")),
        "{refused:?}"
    );
}
