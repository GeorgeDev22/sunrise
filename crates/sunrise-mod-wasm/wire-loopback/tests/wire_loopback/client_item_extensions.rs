use super::*;
use mod_api::__private::game::RegistrationContextNew;
use mod_api::{
    ArmorModelRef, ArmorTextureRef, ClientHost, ClientItemExtensions,
    ClientItemExtensionsRegistrar, EquipmentLayer, EquipmentLayerType, ItemKey, ItemStack,
    ModelPart, PartPose,
};
use std::sync::atomic::{AtomicU32, Ordering};

struct TwoLayers {
    models: [ArmorModelRef; 2],
    textures: [ArmorTextureRef; 2],
    stage: AtomicU32,
}
impl ClientItemExtensions for TwoLayers {
    fn default_dye_color(&self, _: &dyn ClientHost, _: &ItemStack) -> u32 {
        self.stage.store(0, Ordering::Relaxed);
        0
    }
    fn armor_layer_tint_color(
        &self,
        _: &dyn ClientHost,
        _: &ItemStack,
        _: &EquipmentLayer,
        index: u32,
        dye: u32,
    ) -> u32 {
        assert_eq!(dye, 0);
        assert_eq!(self.stage.fetch_add(1, Ordering::Relaxed), index * 3);
        if index == 2 { 0 } else { 0xFFFFFFFF }
    }
    fn armor_texture(
        &self,
        _: &dyn ClientHost,
        _: &ItemStack,
        kind: EquipmentLayerType,
        _: &EquipmentLayer,
        _: ArmorTextureRef,
    ) -> ArmorTextureRef {
        assert_eq!(kind, EquipmentLayerType::Humanoid);
        let stage = self.stage.fetch_add(1, Ordering::Relaxed);
        assert_eq!(stage % 3, 1);
        self.textures[(stage / 3) as usize]
    }
    fn armor_layer_model(
        &self,
        _: &dyn ClientHost,
        _: &ItemStack,
        _: EquipmentLayerType,
        _: &EquipmentLayer,
        index: u32,
    ) -> ArmorModelRef {
        assert_eq!(self.stage.fetch_add(1, Ordering::Relaxed), index * 3 + 2);
        self.models[index as usize]
    }
}
fn register(registrar: &mut dyn ClientItemExtensionsRegistrar) {
    let model = ModelPart::new(Vec::new(), Vec::new(), PartPose::ZERO);
    let models = [
        registrar.register_model("wire_test:inner", &model).unwrap(),
        registrar.register_model("wire_test:outer", &model).unwrap(),
    ];
    let textures = [
        registrar.register_texture("wire_test:inner").unwrap(),
        registrar.register_texture("wire_test:outer").unwrap(),
    ];
    registrar
        .client_item_extensions(
            REGISTERED_ITEM.get().unwrap(),
            Arc::new(TwoLayers {
                models,
                textures,
                stage: AtomicU32::new(0),
            }),
        )
        .unwrap();
}
struct Native;
#[mod_api::__subscriptions]
impl Hooks for Native {
    fn register_client_item_extensions(
        &self,
        _: &mod_api::RegistrationContext,
        event: &mut mod_api::RegisterClientItemExtensionsEvent<'_>,
    ) {
        register(event.extensions);
    }
}
#[derive(Default)]
struct Registry {
    models: u32,
    textures: u32,
    provider: Option<Arc<dyn ClientItemExtensions>>,
}
impl ClientItemExtensionsRegistrar for Registry {
    fn register_model(&mut self, _: &str, _: &ModelPart) -> Result<ArmorModelRef, String> {
        let index = self.models;
        self.models += 1;
        Ok(ArmorModelRef::__new(7, index))
    }
    fn register_texture(&mut self, _: &str) -> Result<ArmorTextureRef, String> {
        let index = self.textures;
        self.textures += 1;
        Ok(ArmorTextureRef::__new(7, index))
    }
    fn client_item_extensions(
        &mut self,
        _: ItemKey,
        provider: Arc<dyn ClientItemExtensions>,
    ) -> Result<(), String> {
        self.provider = Some(provider);
        Ok(())
    }
}
struct Client;
impl mod_api::Host for Client {}
mod_api::impl_host_areas!(Client);
impl ClientHost for Client {}
mod_api::impl_client_host_areas!(Client);

#[test]
fn native_and_guest_item_extensions_produce_the_same_ordered_layer_submissions() {
    mod_api::__private::initialize().unwrap();
    let _phase = mod_api::__private::PhaseOwner::scope(mod_api::__private::ModPhase::World);
    install_counter();
    let stack = ItemStack::of(REGISTERED_ITEM.get().unwrap());
    for guest in [false, true] {
        let hooks: Box<dyn Hooks> = if guest {
            Box::new(WasmMod {
                group: 0,
                subscribed: &[Some(mod_api::HookId::RegisterClientItemExtensions)],
                listeners: &[],
                corrupt: false,
            })
        } else {
            Box::new(Native)
        };
        mod_api::testing::with_mods(
            vec![mod_api::ModEntry::new("wire_test", hooks)].into(),
            || {
                let mut registry = Registry::default();
                mod_api::dispatch_register_client_item_extensions(
                    &mod_api::RegistrationContext::new(),
                    &mut registry,
                );
                let provider = registry.provider.unwrap();
                let dye = provider.default_dye_color(&Client, &stack);
                let mut submissions = Vec::new();
                for index in 0..3 {
                    let layer =
                        EquipmentLayer::__new(ArmorTextureRef::__new(7, 0), false, None, false);
                    let tint = provider.armor_layer_tint_color(&Client, &stack, &layer, index, dye);
                    if tint == 0 {
                        continue;
                    }
                    let texture = provider.armor_texture(
                        &Client,
                        &stack,
                        EquipmentLayerType::Humanoid,
                        &layer,
                        layer.texture,
                    );
                    let model = provider.armor_layer_model(
                        &Client,
                        &stack,
                        EquipmentLayerType::Humanoid,
                        &layer,
                        index,
                    );
                    submissions.push((model, texture, tint));
                }
                assert_eq!(
                    submissions,
                    [
                        (
                            ArmorModelRef::__new(7, 0),
                            ArmorTextureRef::__new(7, 0),
                            0xFFFFFFFF
                        ),
                        (
                            ArmorModelRef::__new(7, 1),
                            ArmorTextureRef::__new(7, 1),
                            0xFFFFFFFF
                        )
                    ],
                    "guest {guest}"
                );
            },
        );
    }
}

pub(super) struct ClientItemExtensionsHooks;
impl ClientItemExtensionsHooks {
    pub(super) fn register_client_item_extensions(
        &self,
        _: &mod_api::RegistrationContext,
        event: &mut mod_api::RegisterClientItemExtensionsEvent<'_>,
    ) {
        register(event.extensions);
    }
}

#[test]
fn refused_scalar_and_reference_callbacks_unwind_the_equipment_boundary() {
    mod_api::__private::initialize().unwrap();
    let _phase = mod_api::__private::PhaseOwner::scope(mod_api::__private::ModPhase::World);
    install_counter();
    mod_api::testing::with_mods(
        vec![mod_api::ModEntry::new(
            "wire_test",
            Box::new(WasmMod {
                group: 0,
                subscribed: &[Some(mod_api::HookId::RegisterClientItemExtensions)],
                listeners: &[],
                corrupt: false,
            }),
        )]
        .into(),
        || {
            let mut registry = Registry::default();
            mod_api::dispatch_register_client_item_extensions(
                &mod_api::RegistrationContext::new(),
                &mut registry,
            );
            let provider = registry.provider.unwrap();
            let stack = ItemStack::of(REGISTERED_ITEM.get().unwrap());
            let layer = EquipmentLayer::__new(ArmorTextureRef::__new(7, 0), false, None, false);
            for callback in 0..4 {
                // One complete layer succeeds before the next call's reply is refused.
                let dye = provider.default_dye_color(&Client, &stack);
                assert_eq!(
                    provider.armor_layer_tint_color(&Client, &stack, &layer, 0, dye),
                    0xFFFFFFFF
                );
                assert_eq!(
                    provider.armor_texture(
                        &Client,
                        &stack,
                        EquipmentLayerType::Humanoid,
                        &layer,
                        layer.texture
                    ),
                    ArmorTextureRef::__new(7, 0)
                );
                assert_eq!(
                    provider.armor_layer_model(
                        &Client,
                        &stack,
                        EquipmentLayerType::Humanoid,
                        &layer,
                        0
                    ),
                    ArmorModelRef::__new(7, 0)
                );
                // Prepare the second layer's ordering before refusing a reference reply.
                if callback >= 2 {
                    provider.armor_layer_tint_color(&Client, &stack, &layer, 1, dye);
                }
                if callback == 3 {
                    provider.armor_texture(
                        &Client,
                        &stack,
                        EquipmentLayerType::Humanoid,
                        &layer,
                        layer.texture,
                    );
                }
                APPEND_ATTACK.set(3);
                let refused =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match callback {
                        0 => {
                            provider.default_dye_color(&Client, &stack);
                        }
                        1 => {
                            provider.armor_layer_tint_color(&Client, &stack, &layer, 1, dye);
                        }
                        2 => {
                            provider.armor_texture(
                                &Client,
                                &stack,
                                EquipmentLayerType::Humanoid,
                                &layer,
                                layer.texture,
                            );
                        }
                        _ => {
                            provider.armor_layer_model(
                                &Client,
                                &stack,
                                EquipmentLayerType::Humanoid,
                                &layer,
                                1,
                            );
                        }
                    }));
                APPEND_ATTACK.set(0);
                assert!(
                    refused.is_err(),
                    "callback {callback} must not supply a default answer"
                );
            }
        },
    );
}
