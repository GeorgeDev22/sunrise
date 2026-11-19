pub(super) struct ApplyEffectTickHooks;
impl mod_api::MobEffectPart for ApplyEffectTickHooks {
    fn effect_subscribed(&self) -> &'static [Option<mod_api::HookId>] {
        &[
            Some(mod_api::HookId::ShouldApplyEffectTickThisTick),
            Some(mod_api::HookId::ApplyEffectTick),
        ]
    }
    fn should_apply_effect_tick_this_tick(
        &self,
        _: &dyn mod_api::Host,
        event: &mut mod_api::ShouldApplyEffectTickThisTickEvent,
    ) {
        event.apply = event.tick_count % 2 == 0;
    }
    fn apply_effect_tick(
        &self,
        _: &dyn mod_api::Host,
        event: &mut mod_api::ApplyEffectTickEvent<'_>,
    ) {
        assert!(event.has_effect(event.effect));
        event.heal(2.5);
        event.hurt("minecraft:wither", 3.);
        event.eat(2, 0.25);
        event.add_exhaustion(0.5);
        event.keep = false;
    }
}
