# Castle Fight Native

[![Linux and Windows builds](https://github.com/ripdog/castle-fight-native/actions/workflows/build.yml/badge.svg)](https://github.com/ripdog/castle-fight-native/actions/workflows/build.yml)

An in-progress, native Rust reimplementation of **Castle Fight DE**, the Warcraft III custom map. Castle Fight Native uses a deterministic, authoritative simulation and a 3D client built with [Bevy](https://bevyengine.org/). The goal is to reproduce the map's mechanics and Classic Warcraft III presentation without requiring the Warcraft III engine to run the game.

**Status:** active development, **not a complete or stable release**. The currently supported gameplay data targets **Castle Fight DE Beta 9.27 (revision r1)**. The repository also archives a 9.32 map for future compatibility work; 9.32 is not yet a supported native ruleset. Expect incomplete races, spells, and features.

## Download and play

Builds for **Linux x86-64** and **Windows x86-64** are produced by [GitHub Actions](.github/workflows/build.yml). Open the repository's **Actions → Build** run and download the matching artifact. Each artifact contains:

- `castle-fight-client` (or `.exe`) — graphical game and lobby.
- `castle-fight-server` — optional standalone authoritative server.
- `cf-wc3-assets` — converter for Warcraft III Classic/SD visual and audio assets.
- `assets/shaders`, `assets/ui`, and `maps/castle-fight-9.27.w3x` — assets and map input needed to get started.

**You need your own Warcraft III installation to render the original game art.** Blizzard's models, textures and audio are not included in the release. Convert them locally *before* running the client:

On Linux (adjust the Warcraft III installation path, including for Wine installations):

```sh
./cf-wc3-assets \
  --wc3 "/path/to/Warcraft III" \
  --map "./maps/castle-fight-9.27.w3x" \
  --castle-fight \
  --output "./assets/wc3"
./castle-fight-client
```

On Windows (PowerShell):

```powershell
.\cf-wc3-assets.exe --wc3 "C:\path\to\Warcraft III" --map ".\maps\castle-fight-9.27.w3x" --castle-fight --output ".\assets\wc3"
.\castle-fight-client.exe
```

Extraction produces an `assets/wc3` directory containing the converted models, textures, audio, and manifests. Keep the `assets` folder beside the executables. Missing or outdated conversions may result in placeholders or missing visuals; rerun the converter when switching to an updated build. The extractor is restricted to **Classic/SD** assets, not Reforged/HD art. See [asset extraction details](crates/wc3-assets/README.md).

From the client's main menu, choose **Single Player**, **Host Game**, or **Join Game**. Hosting uses TCP port **6112** by default. To join over a network, enter the host's IP address or domain (and port if different); the host may need to allow inbound TCP 6112 through their firewall/router. The host starts the match from the lobby.

## Build from source

Requirements: Rust **1.98.1** (automatically selected by `rust-toolchain.toml`), a C/C++ build toolchain, and on Linux the development packages for **ALSA**, **udev**, **Wayland**, and **xkbcommon** (for example, `libasound2-dev`, `libudev-dev`, `libwayland-dev`, `libxkbcommon-dev`, and `pkg-config` on Ubuntu).

```sh
git clone https://github.com/ripdog/castle-fight-native.git
cd castle-fight-native
cargo build --release --locked -p castle-fight-client -p castle-fight-server -p castle-fight-wc3-assets
cargo run --release -p castle-fight-wc3-assets -- \
  --wc3 "/path/to/Warcraft III" \
  --map docs/original_map/5329_Castle_Fight_DE_beta9.27_w3p.w3x \
  --castle-fight \
  --output assets/wc3
cargo run --release -p castle-fight-client
```

For source builds, generated `assets/wc3` content is ignored by Git. Installed binary builds look for `assets` beside the executable, while source builds fall back to the checkout's `assets` directory. Set `CASTLE_FIGHT_ASSETS_DIR` to override the location.

You can run the simulation and other workspace tests with `cargo test --workspace`. The optional dedicated server can be started with:

```sh
cargo run --release -p castle-fight-server -- --bind 0.0.0.0:6112
```

## Project layout

| Path | Purpose |
| --- | --- |
| `crates/sim` | Deterministic game simulation and versioned Castle Fight content |
| `crates/client` | Bevy renderer, inputs, menus and multiplayer client |
| `crates/server`, `crates/protocol` | Authoritative networking and protocol |
| `crates/wc3-assets` | Local Warcraft III SD asset converter |
| `crates/sim-bench`, `crates/debug-viewer` | Profiling and development tools |
| `docs/spec` | Engine and gameplay architecture |
| `docs/original_map` | Original map archives and extracted compatibility evidence |
| `docs/verification` | Implementation verification and fidelity checklists |
| `tools/wc3-map` | Map extraction and provenance tooling |

Map data is the source of truth for mechanics. The native simulation is authoritative; animation playback and renderer timing do not determine game outcomes.

## Licensing and attribution

Original code is dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option. **This license does not cover third-party Warcraft III content or the archived Castle Fight maps.** Castle Fight DE is credited in the original map to **Frotty**; Warcraft III is a Blizzard Entertainment property. This is an independent compatibility project and is not affiliated with or endorsed by the original authors or Blizzard. Use an appropriately licensed Warcraft III installation for local asset extraction.
