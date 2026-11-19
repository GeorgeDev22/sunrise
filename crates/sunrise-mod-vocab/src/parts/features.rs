use super::*;
/// The feature types registered at startup, for the executable to hand to the engine's feature
/// decoding.
pub(crate) fn installed_feature_type_list() -> &'static [crate::world::feature_type::FeatureTypeInfo]
{
    installed().map_or(&[], |installed| installed.feature_types.as_slice())
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
    /// The type's id is in a namespace owned by the calling mod's manifest; another mod's
    /// namespace (vanilla's included), an empty name, an id registered twice, an invalid schema
    /// and a wasm mod's type refuse the mods at startup, naming the mod.
    pub fn register_feature_type<T: crate::FeatureType>(
        &mut self,
        name: &str,
        config: Vec<crate::DatapackField>,
        ty: T,
    ) {
        self.add_feature_type(name, config, Arc::new(ty));
    }
}

/// Registrar operations for features.
pub trait FeaturesRegistrar {
    /// Register feature type `name` with its config schema and `ty`; mods reach it through
    /// `register_feature_type`. Native only: a wasm mod's registration is refused, because
    /// feature calls run on the worldgen workers, which a wasm group's store lock can't serve.
    fn add_feature_type(
        &mut self,
        name: &str,
        config: Vec<crate::DatapackField>,
        ty: Arc<dyn crate::FeatureType>,
    );
}
