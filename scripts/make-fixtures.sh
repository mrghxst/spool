#!/usr/bin/env bash
# Regenerates the small PAR2 fixtures used by the engine's Rust tests.
# Requires par2cmdline (GPL). It's only used to generate test data; none of
# its code is part of Spool. The output is committed.
set -euo pipefail
cd "$(dirname "$0")/.."
out=crates/engine/tests/fixtures/par2
rm -rf "$out" && mkdir -p "$out"
python3 - "$out" <<'PY'
import random, sys
out = sys.argv[1]
r = random.Random(20240424)
for name, size in [("a.bin", 40000), ("b.bin", 12345), ("c.bin", 8192)]:
    with open(f"{out}/{name}", "wb") as f:
        f.write(bytes(r.getrandbits(8) for _ in range(size)))
PY
(cd "$out" && par2 create -q -q -s4096 -c8 -n3 set.par2 a.bin b.bin c.bin)
ls -l "$out"
