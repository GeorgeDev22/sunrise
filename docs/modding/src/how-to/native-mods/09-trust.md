# Trust native mods

Native mods run unsandboxed with full access to your computer. Bundle validation checks packaging,
not whether code is safe, and native mixins compile function-replacement code with the same access
as any other native code.

## Registry releases and unreviewed sources

- **Registry releases** are signed with a GitHub artifact attestation tied to the mod's registered
  repository and Sunrise's shared release workflow, then reviewed for malware, dangerous patterns
  and hidden or obfuscated code before they are listed ([Signed, reviewed releases](../../players/installing-mods.md#signed-reviewed-releases)).
  The launcher verifies the file's SHA-256 hash and attestation against the signed registry index
  and remembers the mod's publisher; if a later release comes from a different publisher, it stops
  and asks before installing. A takedown in the signed index removes a release or a whole project.
- **Unreviewed sources** (**Add from source** and local `.smod` files) are not reviewed by Sunrise.
  The launcher shows a warning naming the exact repository and commit, release tag, folder or file,
  with **Cancel** as the default ([Add a mod from source](08-add-from-source.md)). On the command
  line, `--trust-native-code` records your consent to build and run unreviewed native code.

Registry review does not sandbox the code you run: native mods are compiled from source on your
computer and run with your permissions.

## Package checks

- A native `.smod` holds a flat Rust project (`sunrise-mod.toml`, `Cargo.toml`, `Cargo.lock`, `src/`)
  and can include `assets/`, `data/` and `icon.png`.
- The launcher validates the bundle without running its code, then builds it for your game build.
- Registry entries distinguish native code with the `native-mod` kind; switching a project between
  `mod` and `native-mod` changes the project's kind in the registry and removes its earlier releases
  ([Publish a mod](../publish.md)).

## Instance data

The launcher loads native mods for the instance where you install them, using that instance's
worlds, options and logs. Other instances select their own versions. This separation is not a
security boundary: native code can access files outside its instance.

The checks in `horizon-mixins` catch mistakes, not malicious code. See its
[security notes](https://github.com/horizon-rust/horizon-mixins/blob/main/docs/security.md).
