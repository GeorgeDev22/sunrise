#!/bin/bash
# Fork only: the mod API surface report of this branch against the main commit it is based on. Its text (the mod
# API's names, no game data) also goes to the job summary.
set -uo pipefail
base_commit=dd672e6511b31b3170e9922ce1746b6a8d385299
base=$(mktemp -d)
out=$(mktemp)
{
  git fetch --quiet --depth 1 https://github.com/sunrise-org/sunrise.git "$base_commit" &&
  git worktree add --quiet --detach "$base" FETCH_HEAD &&
  cargo build --locked -q -p sunrise-mod-api-gen --bin sunrise-stage &&
  SUNRISE_STAGE_TOOL="$PWD/target/debug/sunrise-stage" GITHUB_STEP_SUMMARY= python3 tools/ci/surface_report.py --trees "$base" .
} > "$out" 2>&1
code=$?
if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
  { echo "### surface report (exit $code)"; echo '```'; tail -c 60000 "$out"; echo '```'; } >> "$GITHUB_STEP_SUMMARY"
fi
cat "$out"
exit $code
