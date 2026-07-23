# OpenStrike

<p align="center">
  <img src="docs/hero.jpg" width="820" alt="OpenStrike rendering de_dust in Pocket3D — a wide sunlit desert courtyard with arches, crates and an open sky." />
</p>

<p align="center">
  <img src="docs/dust2.jpg" width="270" alt="OpenStrike on de_dust2" />
  <img src="docs/inferno.jpg" width="270" alt="OpenStrike on de_inferno" />
  <img src="docs/office.jpg" width="270" alt="OpenStrike on cs_office" />
</p>

<p align="center">
  <img src="docs/psp-dust2.png" width="480" alt="OpenStrike running on a Sony PSP — the sunlit de_dust2 courtyard at 480×272, with the JSX HUD and viewmodel" />
</p>

<p align="center"><em>A CS-like FPS on classic BSP maps — Pocket3D worlds, a PocketJS JSX HUD, gameplay in TypeScript.<br/>
The same game targets desktop (wgpu), PSP (sceGu), and PS Vita (vita2d/GXM); the bottom shot is the real PSP running at a locked 60 fps.</em></p>

A CS-like FPS with single-player bots and desktop LAN multiplayer, built on the **Pocket runtime family**: a Rust
core (Pocket3D) simulates and renders; the *product* — round rules, weapon
tables, difficulty, and the entire HUD — is JavaScript running in an embedded
QuickJS guest. OpenStrike is the first specialized game runtime of
[PocketJS](https://github.com/pocket-stack/pocketjs); the architecture it
instantiates is documented in the engine repo's
[RUNTIMES.md](https://github.com/pocket-stack/pocketjs/blob/main/RUNTIMES.md).

```
crates/openstrike-core   the simulation — portable Rust shared VERBATIM by all
                         targets (movement, bots, weapons, round state)
crates/openstrike        the desktop build
  ├─ pocket3d            wgpu renderer, BSP worlds, collision, skeletal anim
  ├─ pocket-mod          the QuickJS guest (one realm, one turn per tick)
  ├─ pocket-ui-wgpu      the `ui` surface — PocketJS, composited as the HUD
  └─ strike surface      this game's vocabulary (src/guest.rs)
crates/openstrike-psp    the PSP build: an EBOOT on pocket3d-gu (sceGu) and
                         the PocketJS PSP host — same surfaces, same bundle
crates/openstrike-vita   the PS Vita build: a native 960×544 VPK on
                         pocket3d-vita + PocketJS's vita2d/GXM host

game/                    the product bundle (JS/TSX) — runs on every target
  ├─ sdk.ts              `strike` SDK: state snapshots, events, commands
  ├─ rules.ts            the base game as the FIRST MOD: round flow, scoring,
  │                      weapon + bot tuning
  └─ hud.tsx             the HUD — a full PocketJS app (Solid + Tailwind)

vendor/pocketjs          the engine, pinned as a git submodule
```

Every tick: the core simulates, publishes a state snapshot + event batch to
the guest (`strike.__dispatch`), the HUD frame runs, and the guest's queued
commands (`setPhase`, `resetRound`, `configureWeapon`, …) apply back to the
core. Facts flow one way, intent flows the other, and the guest never blocks
the simulation. Change `game/rules.ts` and you have made a mod — the base
game grants itself no privileges a mod wouldn't have.

## Building

```sh
git clone --recursive https://github.com/Junio-flynn/openstrike.git
cd open-strike
bun run setup      # installs the vendored framework deps + solid-js link
bun run bootstrap  # install the pinned PSP toolchain into the shared cache
bun run check:platforms
bun run build:ui   # resolve PSP -> dist/pocket/psp/openstrike.{js,pak}
bun scripts/build-ui.ts --target vita
```

PSP builds resolve the normalized SDK in a fixed order: `PSP_SDK`, then
`PSPDEV`, then Pocket's versioned shared cache at
`$XDG_CACHE_HOME/pocket-stack/psp/sdk/sdk-noabicalls-normalized-2026-06-19/mipsel-sony-psp`
(or the same path under `~/.cache`). Both SDK environment variables are then
exported to the build, so Rust and QuickJS cannot silently select different
toolchains. The vendored PocketJS manifest pins the organization-owned
`pocket-stack/pspdev`, `pocket-stack/rust-psp`, and
`pocket-stack/quickjs-rs` revisions used by every PSP build; no DreamCart or
personal-fork checkout is required.

[`pocket.json`](pocket.json) is the portable Pocket application contract. It
requires the draw list, baked glyphs, buttons and the left analog API at a
480x272 logical `integer-fit` viewport; it does not claim touch, dynamic text,
or a stock Pocket3D capability. Pocket3D remains an extension implemented by
OpenStrike's custom native hosts. Every target build validates that manifest,
runs the ordinary reachable TypeScript check, writes
`.pocket/<target>/plan.json`, and delegates compilation to `pocket compile`.
The public `@pocketjs/framework/manifest` helpers verify the build-plan
checksum, project stable `HostBuildInputs`, and generate Cargo's target, host
ABI and viewport environment. Target artifacts are isolated under
`dist/pocket/<target>` so concurrent PSP/Vita builds cannot overwrite one
another. At runtime PocketJS compares target and host ABI; the plan checksum
is build-time consistency data, not a runtime trust mechanism.

### Map data

Maps and textures are **not** in this repo (they are Valve-copyrighted game
data). Point the game at a directory containing them:

```
<maps-root>/
├── maps/de_dust2.bsp  (and friends)
└── support/*.wad      (cs_dust.wad, halflife.wad, ...)
```

Any GoldSrc-era (BSP v30) map works; the eight classic CS maps are the
tested set. `OPENSTRIKE_MAPS` can replace `--maps-dir` below.

### Play

```sh
cargo run --release -p openstrike -- --maps-dir ~/path/to/cs-maps
```

| Input | Action |
| --- | --- |
| Mouse | look |
| WASD | move (Shift = walk) |
| Space | jump |
| Left mouse | fire |
| Right mouse (hold) | aim down sights |
| R | reload |
| F | inspect weapon |
| 1 / 2 / 3 / 4 | Scorpion / AK-74M / UZI / bolt-action sniper |
| Esc | release/capture mouse |
| F3 | debug overlay |
| V | noclip fly (debug) |

Each weapon keeps independent ammunition when switching. The Scorpion has a
30-round magazine and 2.4 s reload, the AK-74M has 30 rounds and a 2.65 s
reload with higher damage, and the UZI has 32 rounds and a 2.3 s reload with
a faster fire rate. The bolt-action sniper has a five-round magazine, carries
25 reserve rounds, reloads in 3.6 s, and must cycle its bolt for 1.7 s after
each shot; releasing and pressing fire again is required for every shot.
Switching weapons imposes a 0.5-second draw delay before the selected weapon
can fire.

The main menu's Callsign editor saves a name up to 24 characters. LAN players
show that name overhead while visible; labels are distance-scaled and hidden
behind map geometry.

All four weapons support hold-to-aim. Aiming smoothly narrows the field of
view, centers and steadies the viewmodel, reduces spread to 10% of its hip-fire
value, softens both camera and weapon recoil, and lowers movement speed to 68%.
The sniper slows further to 40%, has intentionally extreme hip-fire spread,
and becomes highly accurate through its stronger zoom and optical scope. The
saved Scope Multiplier setting scales scoped mouse input from 0.1 to 2.0;
`1.0` keeps the normal angular sensitivity unchanged.

Desktop play includes self-contained effects for each weapon, footsteps,
wall-running, aiming, reloading, landing, impacts, damage, weapon switching,
and empty magazines. If no audio device is available, the game logs a warning
and continues normally.

Options: `--map de_inferno`, `--bots 5`, `--spawn-t`, `--auto-quit 5`.

Round rules (v0.1, see `game/rules.ts`): eliminate every bot to win; die and
you lose. Either way the round resets automatically and the score carries
over.

### macOS LAN multiplayer and shareable app

The desktop build includes an in-game LAN lobby and a redistributable
`open_arena` map with two opposite player spawns. The arena and its procedural
texture WAD are OpenStrike-owned, so the receiving player does not need to
install Counter-Strike data before playing it.

The shareable desktop packages include that one redistributable map. Additional
GoldSrc BSP maps appear in the map picker when supplied through a source build's
`--maps-dir` option; copyrighted Counter-Strike maps are not bundled.

Build the shareable application on a Mac:

```sh
bun run package:mac
```

This produces a universal Apple Silicon + Intel build at
`dist/OpenStrike-macOS.zip`. Send that single ZIP to the other player. Both
players unzip it and open `OpenStrike.app`; if macOS blocks the
first launch, right-click the app and choose **Open**. Allow local-network and
firewall access when prompted.

Build the Linux desktop application on an x86_64 Linux machine:

```sh
bun run package:linux
```

This produces `dist/OpenStrike-linux-x86_64.tar.gz`. Extract it and run
`OpenStrike-linux/openstrike`, or run `OpenStrike-linux/install.sh` to add
OpenStrike to the current user's applications menu. The package is relocatable
and includes the native executable, UI bundle, models, and the redistributable
`open_arena` map.

Inside OpenStrike:

1. Both players select **Multiplayer**.
2. One selects **Create LAN Lobby**.
3. The other player's lobby browser finds it automatically; they select
   **Join**. No IP address is needed.
4. The host selects **Start Match**. Player 1 and Player 2 are placed at the
   two opposite arena spawns automatically.

To test multiplayer on one computer, open **LAN Lobby** and select
**Open Second Window**. OpenStrike hosts in the current window and launches a
second instance as `Local Player 2` over localhost. When both players show as
ready, select **Start Match** in the first window. Leaving the test lobby or
closing the first window also closes the test client it launched.

The host relays movement, stance, weapons, health, and hitscan shots over UDP.
OpenStrike uses ports `27015` and `27016` for the match and discovery. This
version is intended for trusted LANs: it has no internet matchmaking, NAT
traversal, dedicated server, encryption, or anti-cheat. The command-line
`--host`, `--join`, `--port`, and `--name` options remain available for
development and direct connections.

## Headless verification

Every acceptance criterion runs without a window — the renderer draws
offscreen, input is scripted, the RNG is seeded, and the round/lose scripts
boot the full QuickJS guest so the *shipped* rules and HUD are what gets
tested:

```sh
cargo run --release -p openstrike -- --maps-dir $MAPS --script walk   --screenshot out/walk
cargo run --release -p openstrike -- --maps-dir $MAPS --map open_arena --script wallrun
cargo run --release -p openstrike -- --maps-dir $MAPS --script model  --screenshot out/model
cargo run --release -p openstrike -- --maps-dir $MAPS --script combat --screenshot out/combat
cargo run --release -p openstrike -- --maps-dir $MAPS --script round  --screenshot out/round
cargo run --release -p openstrike -- --maps-dir $MAPS --script lose   --screenshot out/lose
```

- `walk` — spawn, run at 250 u/s, slide along walls without clipping, jump
  exactly ~45 units, and hold the viewmodel anti-jitter invariant (the gun
  must ride the interpolated camera, not the raw tick state).
- `wallrun` — use an `open_arena` parkour lane, verify the rising/falling
  wallrun arc and speed boost, then spend the refreshed double jump.
- `model` — the soldier renders and its clips actually animate.
- `combat` — aim, fire, tracers/flash, a bot takes three body shots and dies.
- `round` — observe bot AI, engage, eliminate all, **win** (scored by the JS
  rules), and verify the automatic next round. Screenshots include the HUD.
- `lose` — stand still until the bots win; verify the loss + restart.

## Real PSP hardware

OpenStrike runs on an actual Sony PSP — same simulation, same JS rules, same
JSX HUD, rendered by the sceGu backend (`pocket3d-gu`). Not a stripped-down
demo: the identical `dist/pocket/psp/openstrike.js` bundle that drives the
desktop build boots in QuickJS on the handheld. It ships as a proper EBOOT —
branded XMB icon and backdrop, a main menu that lists every cooked map, and
SELECT to return there mid-round.

<p align="center">
  <img src="docs/psp-menu.png" width="400" alt="OpenStrike PSP main menu — a two-column map list (all eight CS classics) under the wordmark" />
  <img src="docs/psp-fire.png" width="400" alt="Firing on PSP — muzzle flash and the compact corner HUD" />
</p>

<p align="center"><em>Captured from the shipping EBOOT in PPSSPP's software renderer at native 480×272 — the same
deterministic backend the byte-exact e2e goldens run on.</em></p>

```sh
git submodule update --init          # pocket-stack/pocketjs + rust-psp + quickjs-rs
bun scripts/psp.ts                   # resolve PSP plan → bundle → maps → EBOOT
bun scripts/psp.ts --package         # + assemble dist/PSP/GAME/OpenStrike (ms0 layout)
bun scripts/hw.ts --bench            # launch over PSPLINK; frame times stream back
bun scripts/e2e-psp.ts               # deterministic PPSSPP goldens (spawn/walk/fire)
```

Install: copy `dist/PSP/` to a Memory Stick root (or the emulator's memstick
dir) on a homebrew-enabled PSP; OpenStrike appears in the Game menu with its
icon. The build needs the PSP toolchain from the PocketJS ecosystem
(`pocket doctor`) plus the CS maps (`OPENSTRIKE_MAPS`).

Controls: analog stick moves, `△/✕/□/○` looks, `R` fires, `L` jumps, d-pad
down reloads, d-pad up walks, **SELECT** opens the return-to-menu dialog. In
the menu, d-pad selects a map and `○` deploys.

Measured on hardware (333 MHz, `--bench`, scripted dust2 tour): a locked
60 fps, GE under 30 µs — and combat is flat too (no per-shot hitch: the HUD
updates through the framework's imperative hot path, avoiding a reactive
flush per frame). Cooking bakes lightmaps into vertex colors, keeps WAD
textures as swizzled CLUT8 with full mip chains, and ships PVS so the renderer
draws only the visible leaves; each `.p3d` is consumed zero-copy, and maps
load on demand from `maps/` next to the EBOOT into one reused buffer.

## PS Vita

The Vita target runs the same simulation, JavaScript rules and Solid JSX HUD
as desktop and PSP. Pocket3D renders at Vita's native 960×544, while the
PocketJS UI keeps its 480×272 logical layout and rasterizes at Vita's native
2× density. Text, curves and rounded corners therefore use the full 960×544
pixel grid instead of duplicating PSP pixels. There is no letterboxing or
crop. Touch is not implemented yet; both sticks, shoulders, d-pad, face
buttons and SELECT cover gameplay and menus.

```sh
export VITASDK="$HOME/vitasdk"
export PATH="$VITASDK/bin:$HOME/.cargo/bin:$PATH"

OPENSTRIKE_MAPS=~/path/to/cs-maps bun scripts/vita.ts --release # resolved Vita plan → VPK
bun scripts/e2e-vita.ts
```

The VPK is written to `dist/vita/OpenStrike.vpk`. PocketJS's shared Vita
packager combines OpenStrike's branded 128×128 icon and cooked maps with the
framework's black 840×500 background, 280×158 startup image, and LiveArea
template. The Vita3K golden driver uses an isolated VitaFS, scripted dual-stick
input, a guest completion marker, byte-exact 960×544 HUD captures, and native
Pocket3D scene-stat assertions.
See [`crates/openstrike-vita/README.md`](crates/openstrike-vita/README.md) for
the pinned toolchain, controls and emulator-capture details.

## Modding, v0.1 shape

The `strike` surface currently speaks: state snapshots (`hp`, `ammo`,
`phase`, `aliveBots`, …), events (`hit`, `playerDamaged`, `playerDied`,
`roundReset`), and commands (`setPhase`, `resetRound`, `addWin`/`addLoss`,
`setBotCount`, `configureWeapon`, `configureBots`). The HUD has the whole
PocketJS `ui` surface. Both mount into one realm — a mod can restyle the
HUD and rewrite the rules in the same file.

Out of scope for v0.1: buy phases, waypointed bot navigation, internet
matchmaking/dedicated servers, and a stable mod-distribution format. The
surface grows by appending, never by renumbering.

## Assets

- Bot character: see [assets/models/CREDITS.md](assets/models/CREDITS.md).
- Map/texture data (`.bsp`/`.wad`) is © Valve and must be provided from your
  own copy of the game.
