# The Orin archive and the golden check

> **A handoff for an agent running on the Orin or on the Thor.**
> Everything below is a command you can paste. Nothing here needs the ProArt,
> and nothing here needs a network beyond the box's own.
>
> Branch: `phase6-3d`. Repository: `https://github.com/SuperInstance/last-mile.git`.

## What this is for

Two things, in one sitting:

1. **The golden check on ARM.** The three pinned rounds of `tests/golden.toml`
   were played on the ProArt (x86) in Phase 1. Play them again, here, on
   aarch64, and they have to reach the **same three hashes**. That is the
   x86↔ARM determinism proof (spec §9, D10/D26): if it fails, the two
   machines did not play the same game, and every round that crosses between
   them is a round that cannot be compared.
2. **The archive, running.** The Orin is the third node of the M1 loop: it
   takes the rounds the ProArt pushes, replays every one of them headless,
   checks **every** state hash, and reports the verdict. It does that at a
   lower CPU priority and an idle I/O class, because the box belongs to
   somebody else.

The answer the phase wants is `OK 3/3` on the Orin, and a service that is
listening and verifying.

## 0. What the box needs

| tool | why | if it is missing |
|---|---|---|
| `git` | the tree | stop and report it |
| `cargo` + `rustc` 1.99 | the build and the check | see §1 |
| `nice` | the archive's CPU priority | the service runs in the foreground (see §4) |
| `ionice` | the archive's I/O class | the service runs in the foreground (see §4) |

`nice` and `ionice` come with `util-linux`, which is on every Ubuntu and
JetPack image. Check them anyway — the archive warns and carries on when
they are missing, but it would be running at the foreground priority.

## 1. Install Rust 1.99

The repo pins its toolchain in `rust-toolchain.toml`, so `cargo` inside the
tree uses 1.99 by itself. The toolchain still has to be *installed* once:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
  | sh -s -- -y --default-toolchain 1.99
. "$HOME/.cargo/env"
rustc --version    # must say 1.99.x
```

If `rustup` is already installed, `rustup toolchain install 1.99` is enough.

## 2. Get the tree

```bash
mkdir -p ~/src
git clone --branch phase6-3d https://github.com/SuperInstance/last-mile.git ~/src/last-mile
cd ~/src/last-mile
```

Already cloned? Then pull instead:

```bash
cd ~/src/last-mile
git fetch origin phase6-3d
git checkout phase6-3d
git pull --ff-only origin phase6-3d
```

Or let `scripts/orin-build.sh` do all of it — see §9.

## 3. The golden check — the headline

```bash
cd ~/src/last-mile
cargo run --release --bin lm -- golden
```

The answer is one line and an exit code:

```text
OK 3/3
```

Exit code `0` means the ARM machine reached the same three hashes the x86
machine reached in Phase 1.

**Anything else is the finding this phase exists to make.** A failure looks
like this:

```text
seed 2: golden says 0xbdcb9285c869c6d6, the round says 0x1234567890abcdef
2/3 seeds match
```

with a **non-zero exit code**. When you see that:

- **do not** regenerate `tests/golden.toml` — the pinned hashes are the
  contract, and rewriting them hides the finding instead of fixing it;
- **do not** "fix" a divergence in the archive — the archive reports a
  mismatch, it never rewrites a hash;
- report the exact output and stop.

The archive's own version of the same check writes the verdict where the
ProArt can read it:

```bash
cd ~/src/last-mile
cargo build --release -p lm-archive
mkdir -p ~/.local/share/last-mile/archive
./target/release/lm-archive golden --store ~/.local/share/last-mile/archive
cat ~/.local/share/last-mile/archive/reports/golden.json
```

The report names the machine it ran on (`"arch": "aarch64"`), the tick count
and the verdict. The ProArt's HUD reads that file.

## 4. The archive, as a service

### 4.1 The configuration

Every knob has a default; this file names them all. The options may come
before or after the command.

```bash
mkdir -p ~/.config/last-mile ~/.local/share/last-mile/archive
cat > ~/.config/last-mile/archive.toml <<EOF
listen = "0.0.0.0:8790"
store_dir = "$HOME/.local/share/last-mile/archive"
max_log_bytes = 268435456
verify_on_arrival = true
retain_rounds = 0
nice = 10
ionice_class = 3
workers = 1
read_timeout_s = 30
queue_capacity = 8
EOF
```

| knob | default | what it does |
|---|---|---|
| `listen` | `0.0.0.0:8790` | where the service takes pushes. Use `127.0.0.1:<port>` to keep it off the LAN. |
| `store_dir` | `/var/lib/last-mile/archive` | the rounds, the reports and the state. |
| `max_log_bytes` | 256 MiB | the biggest push accepted. A frame that claims to be bigger is refused on the length alone, before its body is buffered. |
| `verify_on_arrival` | `true` | replay every round as it arrives. |
| `retain_rounds` | 0 | how many rounds to keep. 0 keeps them all; the oldest go first. |
| `nice` | 10 | the CPU priority. `nice -n 10` **adds** 10 to the niceness the process already has. |
| `ionice_class` | 3 | the I/O class. 3 is idle: I/O only when nothing else is pending. |
| `workers` | 1 | how many verification workers. More than one replays rounds in parallel. |
| `read_timeout_s` | 30 | how long a connection may stay silent before it is dropped. A peer that opens a socket and says nothing cannot pin a thread. |
| `queue_capacity` | 8 | how many rounds may wait for the worker. A full queue is backpressure on the push, never on the accept loop. |

### 4.2 Build it

```bash
cd ~/src/last-mile
cargo build --release -p lm-archive -p lm-tools
```

### 4.3 Run it by hand first

```bash
~/src/last-mile/target/release/lm-archive serve --config ~/.config/last-mile/archive.toml
```

It prints where it is listening, and it **re-executes itself** under
`nice -n 10` and `ionice -c 3` — the same pid, the same sockets, at the
priority it asked for. The re-exec happens exactly once: an environment
variable marks it.

Check that it took:

```bash
pid=$(pgrep -n lm-archive)
awk '{print "nice:", $19}' /proc/$pid/stat     # 10 or more
ionice -p $pid                                # idle
```

### 4.4 The systemd --user unit

```bash
mkdir -p ~/.config/systemd/user
cat > ~/.config/systemd/user/lm-archive.service <<EOF
[Unit]
Description=last-mile archive (the Orin)
After=network.target

[Service]
Type=simple
ExecStart=%h/src/last-mile/target/release/lm-archive serve --config %h/.config/last-mile/archive.toml
Restart=on-failure
RestartSec=5
# The service lowers its own priority (nice/ionice). Do NOT add Nice= or
# IOSchedulingClass= here: the two would be counted twice, and the niceness
# would end up at 20 instead of 10.

[Install]
WantedBy=default.target
EOF

systemctl --user daemon-reload
systemctl --user enable --now lm-archive.service
systemctl --user status lm-archive.service --no-pager
journalctl --user -u lm-archive.service -n 50 --no-pager
```

So that the unit runs without a login session:

```bash
loginctl enable-linger "$USER"
```

`SIGTERM` (what `systemctl --user stop` sends) ends the service. Every file
the archive writes is written atomically — a temporary file, then a rename —
so a kill never leaves half a round behind.

### 4.5 Does this box even have a desktop?

The spec only says the Orin "archives and verifies". Nothing in the repo
says the Orin runs a desktop, so **check before assuming**:

```bash
ps -eo comm | grep -iE 'xorg|wayland|gnome|kde'
```

- **Nothing** → the box has no desktop. The yielding is harmless insurance.
- **Something** → a desktop is running, and the yielding matters. Say so in
  the report.

## 5. The ProArt push

The ProArt plays a round and pushes the log over the LAN. The archive
replays it, checks every hash, and answers with the verdict.

**On the ProArt:**

```bash
lm run --seed 1 --ticks 3000 --out ~/round.lml
lm-archive push ~/round.lml <the-orin's-ip>:8790
```

The answer is one line:

```text
verified: the archive replayed the round to tick 2999 (hash 0x...)
```

A refusal names itself:

```text
lm-archive: the archive refused the push: FAILED: diverged at tick 41: ...
```

**On the Orin**, the round and its report are in the store:

```bash
ls -l ~/.local/share/last-mile/archive/rounds/
cat ~/.local/share/last-mile/archive/state.json
cat ~/.local/share/last-mile/archive/reports/<the-round-name>.json
```

`state.json` counts what came in, what verified and what did not. The
report says the verdict and, when it is a failure, **the first divergent
tick**.

**If the LAN push is not up yet**, copy the round instead — it is the same
ingest:

```bash
# on the ProArt
scp ~/round.lml <the-orin's-ip>:~/incoming/
# on the Orin
mkdir -p ~/incoming
lm-archive ingest ~/incoming/round.lml --store ~/.local/share/last-mile/archive
```

## 6. The Thor

The Thor runs the same golden check — it is the same proof on a third
machine, and the Thor is where the prerender-ahead will run later (D14).

```bash
mkdir -p ~/src
git clone --branch phase6-3d https://github.com/SuperInstance/last-mile.git ~/src/last-mile
cd ~/src/last-mile
cargo run --release --bin lm -- golden
```

Expect `OK 3/3` and exit code 0.

The Thor does **not** run the archive: it is a mirror, not a store. The
`golden` check is all this phase asks of it.

## 7. If the box cannot build Rust

Build it on the ProArt and copy the binary over. The cross-linker is the
only thing missing from a plain Rust install:

```bash
sudo apt-get install gcc-aarch64-linux-gnu
rustup target add aarch64-unknown-linux-gnu
mkdir -p .cargo
cat > .cargo/config.toml <<'EOF'
[target.aarch64-unknown-linux-gnu]
linker = "aarch64-linux-gnu-gcc"
EOF
cargo build --release --target aarch64-unknown-linux-gnu -p lm-archive -p lm-tools
```

A static musl binary, with `zig` doing the linking, avoids the glibc
version skew entirely:

```bash
cargo install cargo-zigbuild
cargo zigbuild --release --target aarch64-unknown-linux-gnu -p lm-archive -p lm-tools
```

**The skew is the reason the native build is the answer.** A binary
cross-built on an Ubuntu 26.04 ProArt links against that glibc, and a
JetPack Orin may not have it; the binary then fails to start with a version
error. A binary built on the Orin runs on the Orin. Build natively
whenever you can.

> The machine this handoff was written on has **no** cross toolchain:
> `rustup target list --installed` lists `x86_64-unknown-linux-gnu` only,
> and there is no `aarch64-linux-gnu-gcc`, no `cross` and no `zig`. The
> commands above are therefore documented, not run — which is exactly why
> the native build on the Orin is the plan.

## 8. `scripts/orin-build.sh`

The steps of §1 to §3 in one script, driven over SSH from the ProArt:

```bash
ORIN_HOST=<the-orin's-ip> bash scripts/orin-build.sh
```

It clones or pulls `phase6-3d`, builds, runs the golden check and records
the archive's verdict. It is **idempotent**: running it twice does the same
work and reports the same result. `--dry-run` prints the steps instead of
running them, and `--local` runs them on the machine you are on.

## 9. What to report back

Report these, in this order, with the exact output:

1. `uname -m` and `rustc --version`.
2. The output of `cargo run --release --bin lm -- golden` — the `OK 3/3`
   line and the exit code.
3. The contents of `~/.local/share/last-mile/archive/reports/golden.json`.
4. `ps -eo comm | grep -iE 'xorg|wayland|gnome|kde'` — the desktop answer.
5. The service's own priority: `awk '{print $19}' /proc/<pid>/stat` and
   `ionice -p <pid>` for the running service.
6. `systemctl --user status lm-archive.service --no-pager` — one line is
   enough.
7. The push round trip: the `lm-archive push` output from the ProArt, and
   the Orin's `state.json`.
8. Anything that failed, with the exact output and the exit code.

**If the golden check failed, report the named mismatch and stop.** Do not
regenerate the golden file, and do not touch a hash in the archive: a
mismatch is reported, never fixed.
