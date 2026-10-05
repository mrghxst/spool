#!/usr/bin/env bash
# Generates E2E fixture jobs into e2e/.work/fixtures/<job>/ and the
# mock-nntp config. Needs python3, par2 and 7z; rar is optional.
set -euo pipefail
cd "$(dirname "$0")"
W=.work
rm -rf "$W/fixtures" "$W/expected" "$W/mock.json" && mkdir -p "$W/fixtures" "$W/expected" "$W/mock"

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

# Throughput benchmark job (only with SPOOL_PERF=1): one 256 MB file.
if [[ "${SPOOL_PERF:-}" == 1 ]]; then
  mkdir -p "$W/fixtures/perf"
  python3 -c "import random,sys; sys.stdout.buffer.write(random.Random(9).randbytes(256*1024*1024))" > "$W/fixtures/perf/dataset-256m.bin"
fi

# RAR multi-volume, only when a rar binary is available.
if command -v rar >/dev/null; then
  mkdir -p "$W/fixtures/rar"
  (cd "$W/expected/archive" && rar a -inul -m1 -v1000k "../../fixtures/rar/dataset.rar" dataset.bin readme.txt)
  echo "rar fixtures created"
fi

# mock-nntp config: one job per fixture folder, four providers.
python3 - "$W" <<'PY'
import json, os, sys
W = os.path.abspath(sys.argv[1])
jobs = [
    {"name": "clean", "dir": f"{W}/fixtures/clean"},
    {"name": "backup", "dir": f"{W}/fixtures/backup"},
    {"name": "repair", "dir": f"{W}/fixtures/repair", "options": {"obfuscate": True}},
    {"name": "archive", "dir": f"{W}/fixtures/archive", "options": {"password": "not-needed"}},
]
if os.path.isdir(f"{W}/fixtures/rar"):
    jobs.append({"name": "rar", "dir": f"{W}/fixtures/rar"})
if os.path.isdir(f"{W}/fixtures/perf"):
    # Real posts use articles of about 700 KB.
    jobs.append({"name": "perf", "dir": f"{W}/fixtures/perf", "options": {"article_size": 716800}})
def provider(name, port, missing=None):
    return {"name": name, "port": port, "user": "spool", "pass": "secret", "missing": missing or []}
config = {
    "out": f"{W}/mock",
    "jobs": jobs,
    "providers": [
        provider("full", 15631),
        # 10% missing: parts 3, 13, 23, ... of the backup job's data file.
        provider("sparse", 15632, [{"every": 10, "offset": 3, "file_contains": "debian"}]),
        # Both miss parts of the repair job; parts 1 and 36 are on neither.
        provider("partial-a", 15633, [{"every": 5, "offset": 1, "file_contains": "fedora"}]),
        provider("partial-b", 15634, [{"every": 7, "offset": 1, "file_contains": "fedora"}]),
    ],
}
json.dump(config, open(f"{W}/mock.json", "w"), indent=2)
PY

ls -l "$W/fixtures/"*
