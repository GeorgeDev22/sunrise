use super::*;

#[derive(Default)]
struct StackProbe {
    stack: Arc<std::sync::OnceLock<std::result::Result<(), &'static str>>>,
}

#[mod_api::sunrise_mod]
impl Hooks for StackProbe {
    fn chunk_data_save(&self, _: &dyn ChunkDiskHost, _: &mut ChunkDataSaveEvent<'_>) {
        self.stack
            .set(sunrise_guest_stack::check_current())
            .unwrap();
    }
}

#[test]
#[ignore = "daily"]
fn asynchronous_chunk_save_hook_runs_with_a_guest_stack() {
    // Subscribed saves consult a process-wide lease. A parallel test must not block this save.
    const CHILD: &str = "SUNRISE_REGION_WRITER_STACK_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "save::chunk_disk_tests::writer_stack::asynchronous_chunk_save_hook_runs_with_a_guest_stack",
            ])
            .env(CHILD, "1")
            .status()
            .unwrap();
        assert!(status.success());
        return;
    }
    let probe = StackProbe::default();
    let stack = probe.stack.clone();
    let mods = vec![mod_api::ModEntry::new("stack_probe", Box::new(probe))].into();
    save_chunk(mods);
    assert_eq!(stack.get(), Some(&Ok(())));
}

#[test]
#[ignore = "daily"]
fn compiled_chunk_save_hook_updates_the_region_record() {
    // Loading the fixture enters the guest too, before the real writer thread takes over.
    sunrise_guest_stack::run_on_guest_stack(|| {
        use sunrise_mod_package::manifest::Package;
        let path = PathBuf::from(std::env::var_os("SUNRISE_WASM_FIXTURE").unwrap());
        let mut manifest = Package::read(&path).unwrap().manifest;
        // This test uses a hook, not the fixture's injection points.
        manifest.inject.clear();
        let info = mod_api::PackageInfo::new(
            manifest.id.clone(),
            manifest.version.to_string(),
            mod_api::PackageKind::Code,
            mod_api::Side::Both,
            vec![manifest.id.clone()],
            Vec::new(),
        );
        let std = fs::read(std::env::var_os("SUNRISE_WASM_STD").unwrap()).unwrap();
        let loaded = sunrise_mod_wasm::load(
            &std,
            vec![sunrise_mod_wasm::WasmPackage {
                manifest,
                module: Package::read_wasm(&path).unwrap(),
            }],
        );
        assert!(loaded.refused.is_empty(), "{:?}", loaded.refused);
        loaded.initialize(&[info]).unwrap();
        mod_platform::__private::initialize().unwrap();
        let _registration =
            mod_api::__private::PhaseOwner::scope(mod_api::__private::ModPhase::Registration);
        assert_eq!(save_chunk(loaded.mods.into()), 8);
    });
}

fn save_chunk(mods: mod_api::Mods) -> i32 {
    let (registry, biomes) = super::super::loot_tests::fixture();
    let registry = Box::leak(Box::new(registry));
    let biomes = Arc::new(biomes);
    let path = tempfile::tempdir().unwrap();
    mod_platform::testing::with_mods(mods, || {
        let save = SaveDir::new(
            path.path().to_path_buf(),
            LevelShape::OVERWORLD,
            Some(Default::default()),
        );
        context(&save);
        save.save_later(&[Arc::new(chunk(7))], registry, &biomes)
            .unwrap();
        save.flush().unwrap();
    });
    disk_value(path.path(), registry, &biomes).unwrap()
}
