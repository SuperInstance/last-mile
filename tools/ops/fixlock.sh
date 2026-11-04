#!/usr/bin/env bash
# Resolve a Cargo.lock rebase conflict safely: take the BASE (upstream)
# lockfile, let cargo add only the new entries, and refuse if any base pin moved.
set -euo pipefail
base="${1:?base branch}"
git checkout --ours Cargo.lock
cargo metadata --format-version 1 >/dev/null 2>&1
python3 - "$base" <<'PY'
import subprocess,re,sys
def pk(t): return set(re.findall(r'name = "([^"]+)"\nversion = "([^"]+)"',t))
a=pk(subprocess.run(['git','show',f'{sys.argv[1]}:Cargo.lock'],capture_output=True,text=True).stdout)
b=pk(open('Cargo.lock').read())
gone=a-b
if gone: print("REFUSE: base pins moved:",sorted(gone)[:10]); sys.exit(1)
print(f"lockfile OK: {len(b-a)} new packages, 0 base pins moved")
PY
git add Cargo.lock
