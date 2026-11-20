#!/usr/bin/env python3
"""Compare Sunrise's light reads with the real 26.3 LevelLightEngine.

`tools/light_storage_scenario.txt` is replayed by the Java oracle on the real engine (the sections a client
stores, the padding past the build range, sections between stored ones, columns that unload or are never
loaded) and by `light_storage_tests` on a `World`; both must give the answers recorded in
`tools/light_storage_witness.txt`, and so each other.
"""
import os
from pathlib import Path

try:
    from . import lab, oracle_harness, test_rust_parity as base, test_model_baking_parity as baking
except ImportError:
    import lab, oracle_harness, test_rust_parity as base, test_model_baking_parity as baking


def main():
    build = Path(os.environ.get("SUNRISE_LIGHT_STORAGE_PARITY_DIR", lab.LOCAL / "light-storage-parity"))
    build.mkdir(parents=True, exist_ok=True)
    lock, jars, _ = base.verify_artifacts()
    classes, dependencies, _ = oracle_harness.compile_oracle(
        build, "VanillaLightStorageOracle", jars, baking.client_libraries(), lock["minecraft"]["java_major"])
    scenario = lab.ROOT / "tools" / "light_storage_scenario.txt"
    expected = build / "vanilla.txt"
    oracle_harness.java_run(build, classes, dependencies, "VanillaLightStorageOracle", [scenario, expected], "vanilla")
    if expected.read_text() != (lab.ROOT / "tools" / "light_storage_witness.txt").read_text():
        raise AssertionError("the real engine's answers differ from tools/light_storage_witness.txt")
    test = "light_storage_tests::light_reads_follow_vanilla_storage"
    oracle_harness.run_logged([
        "cargo", "test", "--locked", "-p", "sunrise-client-render", "--lib",
        test, "--", "--exact",
    ], build, "rust", env=lab.game_env())
    if f"test {test} ... ok" not in (build / "rust.log").read_text():
        raise AssertionError(f"Rust witness replay did not run: {test}")
    print(f"{sum(line.startswith('read ') for line in expected.read_text().splitlines())} light reads match the real engine")


if __name__ == "__main__":
    main()
