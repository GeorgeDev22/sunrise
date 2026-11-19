use super::*;
use std::sync::Mutex;

/// One package's files, as the game serves them.
struct Files;
impl mod_api::RuntimeFileSource for Files {
    fn names(&self) -> Vec<String> {
        vec!["nav.bin".into()]
    }
    fn file(&self, name: &str) -> Option<Vec<u8>> {
        (name == "nav.bin").then(|| b"baked tables".to_vec())
    }
    fn path(&self, _: &str) -> Option<mod_api::InstanceDirectory> {
        None
    }
}

/// What the guest read, from its registration.
static SEEN: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn read(cx: &mod_api::RegistrationContext) {
    let seen = match cx.runtime_files() {
        Ok(files) => format!(
            "{:?} {:?} {:?} {:?}",
            files.names(),
            files.file("nav.bin"),
            files.file("other.bin"),
            files.file_path("nav.bin"),
        ),
        Err(error) => error.to_string(),
    };
    SEEN.lock().unwrap().push(seen);
}

#[test]
fn wire_loopback_a_guest_reads_its_own_runtime_files_by_id_and_has_no_path() {
    // The game's snapshot, once: the files of the mod being dispatched.
    let files: std::collections::BTreeMap<String, Arc<dyn mod_api::RuntimeFileSource>> = [(
        "runtime_fixture".to_owned(),
        Arc::new(Files) as Arc<dyn mod_api::RuntimeFileSource>,
    )]
    .into();
    let data = mod_api::__private::RegistrationData::new(
        Default::default(),
        Default::default(),
        mod_api::InstanceDirectory::Unix(b"/".to_vec()),
        mod_api::RuntimeEnvironment::new(
            mod_api::Dist::Client,
            "26.3".into(),
            "nightly-test".into(),
            false,
        ),
    )
    .with_runtime_files(files);
    // The data is this test's alone, for the dispatch and its assertions.
    mod_api::__private::with_test_registration_data(data, || {
        mod_api::__private::initialize().unwrap();
        let _phase = mod_api::__private::PhaseOwner::scope(mod_api::__private::ModPhase::World);
        struct Fixture;
        impl Hooks for Fixture {
            fn subscribed(&self) -> &'static [Option<mod_api::HookId>] {
                &[]
            }
            fn register(
                &self,
                _: &mut dyn mod_api::Registrar,
                context: &mod_api::RegistrationContext,
            ) {
                let module = WasmMod {
                    group: 0,
                    subscribed: &[],
                    listeners: &[],
                    corrupt: false,
                };
                let mut cx = Box::new(boundary::Cx::default());
                cx.module = &module;
                cx.frames.push(boundary::Frame::default());
                let pointer = &mut *cx as *mut boundary::Cx;
                // SAFETY: the module and context outlive the synchronous calls and scopes.
                let _resources = unsafe { boundary::resource_scope(pointer, true) };
                let mut out = Writer::new();
                out.put(context);
                // SAFETY: the fixture owns cx until the transport scope drops.
                let transport = unsafe { host_dispatch::transport(pointer) };
                let _transport = TransportScope(None);
                guest::set_transport(Some(transport));
                let _guest = guest::resource_scope();
                let context: mod_api::RegistrationContext =
                    Reader::new(out.bytes()).take().unwrap();
                read(&context);
            }
        }
        mod_api::__private::install_parts(
            &[mod_api::ModEntry::new("runtime_fixture", Box::new(Fixture))],
            &mod_api::__private::part_tests::rule_schema(),
            |_| false,
        )
        .unwrap();
        let seen = SEEN.lock().unwrap().clone();
        assert!(!seen.is_empty(), "the guest registered");
        for line in &seen {
            assert_eq!(
                line,
                "[\"nav.bin\"] Some([98, 97, 107, 101, 100, 32, 116, 97, 98, 108, 101, 115]) None Err(NotAFolder)"
            );
        }
    });
}
