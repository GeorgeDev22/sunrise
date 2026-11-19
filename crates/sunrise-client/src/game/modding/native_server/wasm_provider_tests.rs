//! The suggestion provider a compiled guest declares on a command argument type, next to the same
//! policy compiled natively: the engine asks both for one argument and keeps the matching
//! candidates, and the two answers must be equal.
//!
//! Nothing else covers the nested handle: a registered type whose `suggestion_provider` returns an
//! object that lives in the guest, asked for a prefix, the arguments before it and the sender.
use super::*;
use sunrise_client::command::{Registries, Suggestion};

#[test]
#[ignore = "daily"]
fn a_compiled_guests_suggestion_provider_answers_like_the_native_one() {
    // Enters the guest: libtest's thread is smaller than the guest stack policy requires. The load
    // budget: this checks behaviour, not speed, and a loaded runner can pass 100 ms.
    sunrise_guest_stack::run_on_guest_stack(|| {
        sunrise_mod_wasm::copy_probe::with_load_budget(
            a_compiled_guests_suggestion_provider_answers_like_the_native_one_on_guest_stack,
        )
    });
}

/// The same policy as the fixture's (`tests/fixture/src/suggestion_provider.rs`), compiled
/// natively: the tooltip carries the request back, so a lost prefix, argument or sender differs.
struct NativeWarps;

impl mod_api::SuggestionProvider for NativeWarps {
    fn suggest(&self, request: &mod_api::SuggestionRequest) -> mod_api::SuggestionReply {
        let earlier: Vec<String> = request
            .arguments
            .iter()
            .map(|argument| format!("{}={}", argument.name, argument.text))
            .collect();
        let sender = request.sender.as_ref().map_or_else(
            || "none".to_owned(),
            |sender| {
                format!(
                    "{}={}:{}",
                    sender.name(),
                    sender.uuid(),
                    sender.permission().level()
                )
            },
        );
        mod_api::SuggestionReply::new([
            mod_api::CommandSuggestion::with_tooltip(
                "press",
                format!("{}|{}|{}", request.prefix, earlier.join(","), sender),
            ),
            mod_api::CommandSuggestion::new("furnace"),
        ])
    }
}

struct NativeWarpsArgument;

impl mod_api::CommandArgumentType for NativeWarpsArgument {
    fn kind(&self) -> mod_api::CommandArgumentKind {
        mod_api::CommandArgumentKind::Word
    }

    fn parse(&self, input: &str) -> Result<String, mod_api::CommandArgumentError> {
        Ok(input.to_owned())
    }

    fn suggest(
        &self,
        _: Option<&mod_api::CommandSender>,
        _: &str,
    ) -> Vec<mod_api::CommandSuggestion> {
        panic!("`suggest` is not called for an argument that declares a provider")
    }

    fn suggestion_provider(&self) -> Option<Arc<dyn mod_api::SuggestionProvider>> {
        Some(Arc::new(NativeWarps))
    }
}

/// What `/warps tp <world> <name>` completes for one guest at `pr`: the prefix, the world parsed
/// before it and the sender all reach the provider, and only its matching candidate comes back.
fn completed(
    argument: Arc<dyn mod_api::CommandArgumentType>,
    kind: &str,
) -> Vec<(String, Option<String>)> {
    let arguments: CommandArguments = HashMap::from([(kind.to_owned(), argument)]);
    let commands = [mod_api::Command::new(&format!(
        "warps tp <world:word> <name:{kind}>"
    ))];
    let (tree, _, problems) = command_tree(&commands, |_| false, &arguments);
    assert!(problems.is_empty(), "{problems:?}");
    let mut dispatcher = Dispatcher::new();
    register_dispatcher(&mut dispatcher, &tree, "native_mod_command", &arguments);
    let registries = Registries {
        permission: 3,
        sender: Some(sunrise_client_core::registries::CommandSender {
            uuid: 0x5eed,
            name: "Simon".into(),
        }),
        ..Registries::default()
    };
    let typed = "warps tp survival pr";
    let (_, suggestions) = dispatcher.suggestions(
        &dispatcher.parse(typed, &registries),
        typed.len(),
        &registries,
    );
    let completions: Vec<(String, Option<String>)> = suggestions
        .into_iter()
        .map(|suggestion: Suggestion| (suggestion.text, suggestion.tooltip))
        .collect();
    completions
}

fn a_compiled_guests_suggestion_provider_answers_like_the_native_one_on_guest_stack() {
    use sunrise_mod_package::manifest::Package;
    use sunrise_mod_wasm::{WasmPackage, load};
    let path = std::env::var("SUNRISE_WASM_FIXTURE").unwrap();
    let manifest = Package::read(std::path::Path::new(&path)).unwrap().manifest;
    let info = mod_api::PackageInfo::new(
        manifest.id.clone(),
        manifest.version.to_string(),
        mod_api::PackageKind::Code,
        mod_api::Side::Both,
        vec![manifest.id.clone()],
        vec![],
    );
    let loaded = load(
        &std::fs::read(std::env::var("SUNRISE_WASM_STD").unwrap()).unwrap(),
        vec![WasmPackage {
            manifest,
            module: Package::read_wasm(std::path::Path::new(&path)).unwrap(),
        }],
    );
    assert!(loaded.refused.is_empty(), "{:?}", loaded.refused);
    loaded.initialize(&[info]).unwrap();
    let mods: mod_api::Mods = loaded.mods.into();
    mod_platform::testing::with_mods(mods.clone(), || {
        // `with_mods` registers for the test's controllers only; the argument types a game asks are
        // the installed Parts', as a loaded game has them.
        crate::game::api_values::install_parts(&mods).unwrap();
        mod_api::__private::mark_game_thread();
        let _phase = mod_api::__private::PhaseOwner::scope(mod_api::__private::ModPhase::World);
        let arguments = command_arguments();
        let wasm = arguments
            .get("wasm_fixture:warps")
            .expect("the guest registered its command argument type")
            .clone();
        let native: Arc<dyn mod_api::CommandArgumentType> = Arc::new(NativeWarpsArgument);
        let from_guest = completed(wasm, "wasm_fixture:warps");
        let from_native = completed(native, "wasm_fixture:warps");
        // Same prefix, same earlier argument, same sender, same tooltip, and the candidate that
        // does not match the prefix is kept out by the engine on both targets.
        assert_eq!(
            from_guest,
            [(
                "press".to_owned(),
                // The sender's uuid (`0x5eed` above) prints as a decimal u128 in both providers.
                Some("pr|world=survival|Simon=24301:3".to_owned())
            )],
            "the guest's provider answers what the engine asks"
        );
        assert_eq!(from_guest, from_native, "native and wasm answer alike");
    });
}
