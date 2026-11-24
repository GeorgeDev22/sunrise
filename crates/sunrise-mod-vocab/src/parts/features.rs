use super::*;
/// The feature types registered at startup, for the executable to hand to the engine's feature
/// decoding.
pub(crate) fn installed_feature_type_list() -> &'static [crate::world::feature_type::FeatureTypeInfo]
{
    installed().map_or(&[], |installed| installed.feature_types.as_slice())
}

/// The placement modifier types registered at startup, for the executable to
/// hand to the engine's placement modifier decoding.
pub(crate) fn installed_placement_modifier_type_list()
-> &'static [crate::world::placement_modifier::PlacementModifierTypeInfo] {
    installed().map_or(&[], |installed| {
        installed.placement_modifier_types.as_slice()
    })
}

impl dyn Registrar + '_ {
    /// Register feature type `name` (`slime_tree` or `mymod:slime_tree`), the way NeoForge's
    /// registry `FEATURE_TYPE` registers once at startup: a configured feature with
    /// `"type": "modid:slime_tree"` then places by `ty`'s code, wherever the game places a
    /// feature (decoration, nested features, growth, bone meal, `/place feature`).
    ///
    /// `config` is the configured feature's settings as a flat record, the schema datapack
    /// registries use: the game decodes each configured feature against it when the definition
    /// loads and refuses one that doesn't fit, naming it. A `Reference` field is refused; name
    /// other entries with `Identifier`.
    ///
    /// The type's ID must be in a namespace owned by the calling mod's manifest. An unowned
    /// namespace (including vanilla's), an empty name, a duplicate ID or an invalid schema
    /// refuses registration at startup and names the mod.
    pub fn register_feature_type<T: crate::FeatureType>(
        &mut self,
        name: &str,
        config: Vec<crate::DatapackField>,
        ty: T,
    ) {
        self.add_feature_type(name, config, Arc::new(ty));
    }
    /// Register placement modifier type `name` (`below_sea_level` or
    /// `mymod:below_sea_level`), the way NeoForge's registry
    /// `PLACEMENT_MODIFIER_TYPE` registers once at startup: a placed feature's
    /// `placement` entry with `"type": "modid:below_sea_level"` then decodes
    /// and runs by `ty`'s code in the placement chain beside vanilla's
    /// modifiers. The id's namespace must be owned by the calling mod's
    /// manifest; another mod's namespace, an empty name and an id registered
    /// twice refuse the mods at startup, naming the mod.
    pub fn register_placement_modifier_type<T: crate::PlacementModifierType>(
        &mut self,
        name: &str,
        ty: T,
    ) {
        self.add_placement_modifier_type(name, Arc::new(ty));
    }
}

/// Registrar operations for features.
pub trait FeaturesRegistrar {
    /// Register feature type `name` with its config schema and `ty`; mods reach it through
    /// `register_feature_type`. Feature calls run on the worldgen workers.
    fn add_feature_type(
        &mut self,
        name: &str,
        config: Vec<crate::DatapackField>,
        ty: Arc<dyn crate::FeatureType>,
    );
    /// Register placement modifier type `name` with `ty`; mods reach it through
    /// `register_placement_modifier_type`.
    fn add_placement_modifier_type(
        &mut self,
        name: &str,
        ty: Arc<dyn crate::PlacementModifierType>,
    );
}
