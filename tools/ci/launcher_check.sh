#!/usr/bin/env bash
# Fork-only: the launcher's CI checks (fmt, clippy, lib and bin tests) on one of
# GeorgeDev22/sunrise-launcher's branches, whose own fork has no Actions.
set -euo pipefail
branch=$1
dir="${RUNNER_TEMP:-/tmp}/launcher-${branch//\//-}"
rm -rf "$dir"
git clone -q --depth 1 -b "$branch" https://github.com/GeorgeDev22/sunrise-launcher "$dir"
cd "$dir"
bash tools/ci/install_launcher_deps.sh libxkbcommon-x11-dev libwayland-dev libx11-xcb-dev \
  libfontconfig-dev libvulkan-dev libasound2-dev libudev-dev
v=$(tr -d '[:space:]' < tools/ci/rust-version)
rustup toolchain install "$v" --profile minimal -c clippy -c rustfmt >/dev/null
export CARGO_TARGET_DIR="${RUNNER_TEMP:-/tmp}/launcher-target" SUNRISE_TELEMETRY_URL=off \
  SUNRISE_LAUNCHER_NO_SELF_UPDATE=1 SUNRISE_NO_AUDIO=1
cargo "+$v" fmt --all -- --check
cargo "+$v" clippy --all-targets --locked -- -D warnings
cargo "+$v" test --locked --workspace --lib --bins
