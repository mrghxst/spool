#!/usr/bin/env bash
# Generates E2E fixture jobs into e2e/.work/fixtures/<job>/ and the
# mock-nntp config. Needs python3, par2 and 7z; rar is optional.
set -euo pipefail
cd "$(dirname "$0")"
W=.work
rm -rf "$W/fixtures" "$W/expected" && mkdir -p "$W/fixtures" "$W/expected" "$W/mock"

python3 - "$W" <<'PY'
import os, random, sys
W = sys.argv[1]
def blob(path, size, seed):
    r = random.Random(seed)
    with open(path, "wb") as f:
        f.write(r.randbytes(size))
jobs = {
    "clean": [("ubuntu-24.04.iso", 3_000_000, 1)],
    "backup": [("debian-12.iso", 3_200_000, 2)],
    "repair": [("fedora-40.iso", 3_100_000, 3)],
}
for job, files in jobs.items():
    os.makedirs(f"{W}/fixtures/{job}", exist_ok=True)
    os.makedirs(f"{W}/expected/{job}", exist_ok=True)
    for name, size, seed in files:
        blob(f"{W}/expected/{job}/{name}", size, seed)
# Archive job: a small folder of files.
os.makedirs(f"{W}/expected/archive", exist_ok=True)
blob(f"{W}/expected/archive/dataset.bin", 2_500_000, 4)
with open(f"{W}/expected/archive/readme.txt", "w") as f:
    f.write("Sample dataset for the Spool end-to-end tests.\n" * 200)
PY

for job in clean backup repair; do
  cp "$W/expected/$job/"* "$W/fixtures/$job/"
  (cd "$W/fixtures/$job" && par2 create -q -q -s65536 -r12 -n4 "$job.par2" ./*.iso >/dev/null)
done

# 7z multi-volume archive (1 MB volumes), plus PAR2.
mkdir -p "$W/fixtures/archive"
(cd "$W/expected/archive" && 7z a -bd -mx=1 -v1m "../../fixtures/archive/dataset.7z" dataset.bin readme.txt >/dev/null)
(cd "$W/fixtures/archive" && par2 create -q -q -s65536 -r10 -n2 archive.par2 dataset.7z.* >/dev/null)

# RAR multi-volume, only when a rar binary is available.
if command -v rar >/dev/null; then
  mkdir -p "$W/fixtures/rar"
  (cd "$W/expected/archive" && rar a -inul -m1 -v1000k "../../fixtures/rar/dataset.rar" dataset.bin readme.txt)
  echo "rar fixtures created"
fi

ls -l "$W/fixtures/"*
