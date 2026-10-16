#!/usr/bin/env bash
# Regenerate conformance/expected/*.txt from the current engine.
#
# The expected files are checked in and reviewed by hand, so run this only when
# a behavior change is intended — and read the diff before committing it. A
# silent regeneration is how an unintended regression becomes the new baseline.
set -euo pipefail

cd "$(dirname "$0")/.."

cargo build -q -p matbox-cli
bin=target/debug/matbox

status=0
for case in conformance/cases/*.mbox; do
    stem="$(basename "$case" .mbox)"
    if ! "$bin" run "$case" > "conformance/expected/$stem.txt"; then
        echo "case failed to run: $stem" >&2
        status=1
    fi
done

if [ "$status" -eq 0 ]; then
    echo "regenerated $(ls conformance/expected/*.txt | wc -l) expected file(s)"
    echo "review the diff before committing"
fi
exit "$status"