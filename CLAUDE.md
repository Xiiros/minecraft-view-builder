# CLAUDE.md

Desktop viewer for block-based builds (Java Edition 1.13+ schematics), written in Rust. The user speaks French: talk to them in French; code, comments, commit messages and docs are in English.

## Priorities

Performance first: execution speed, low RAM, small disk footprint. Prefer small, focused dependencies and compact data structures (palette + dense index arrays).

## Layout

- `crates/mvb-core`: data model and file formats. No GPU/UI dependency; everything here must be unit tested.
  - `nbt`: NBT codec (Java, big-endian, modified UTF-8), hardened against malformed input.
  - `block`: `BlockState` (name + sorted properties) and `Palette` (index 0 is always air).
  - `build`: `Build` → `Region`s of `u16` palette indices, layer-major (`(y * size_z + z) * size_x + x`), plus block entities, entities and `extra` NBT kept for lossless re-export.
  - `mvb`: native project format (magic + version + zstd NBT); the layout is documented at the top of the file.
  - `materials`: blocks counted as placed items (`item_for`: double slab = 2, upper door half / bed head skipped, wall torch = torch, candles/pickles/snow layers counted), `stack_size`, `MaterialIndex` (per-palette lookup, `count`).
  - `formats`: other tools' formats (`structure` = vanilla `.nbt`, `sponge` = WorldEdit `.schem` v1-v3, `litematica` = `.litematic`, multi-region), shared gzip/`ImportError` helpers.
  - Export: `FileFormat::write` + `formats::losses` (what a format cannot keep: merged regions, other tools' data, name/description in .nbt, structure-block size limit) shown to the user before writing. `formats/export.rs` has `merged_region` (single-box formats). Sponge writes v2 unless the source was v3; Litematica writes back the source version, signed region geometry, ticks and timestamps. `Build::same_content` compares builds whatever the region split / palette order (used by round-trip tests and `inspect`).
  - File readers must treat input as untrusted: check lengths before allocating, return errors, never panic.
  - `examples/inspect.rs`: loads files, prints a summary and checks the round trip through `.mvb` and every export format (`cargo run --release -p mvb-core --example inspect -- <files>`). Real test data: the game jar (`%APPDATA%\.minecraft\versions\<v>\<v>.jar`) contains ~1200 vanilla structures under `data/minecraft/structure/`.
- `crates/mvb-app`: egui + wgpu desktop app (binary `mvb`, accepts a file path argument).
  - `i18n`: UI strings, one `Texts` constant per language (English, French).
  - `document`: an opened build + derived display data (bounding box, palette colours, visible regions, `layer_cells`).
  - `colors`: block colour from `block_colors.rs` (top colour, or side colour for logs lying along x/z), name hash for unknown blocks.
  - `block_colors.rs`: **generated**, do not edit. Regenerate after a game update with `cargo run --release -p colorgen -- <path to the client .jar>`.
- `tools/icongen`: dev tool drawing the application icon (PNG, ICO, ICNS).
- `tools/colorgen`: dev tool (not shipped). Reads the client jar: blockstate → unrotated variant model → parents → texture of the highest `up` face and of the first largest element's side → linear-light average of opaque pixels, first animation frame, biome tint (plains) for tinted faces. Only colour values are committed, never textures.
  - `view2d`: layer view; the layer is one nearest-filtered texture (1 px per block) on a rotated quad, rulers/grid/compass painted on top. `draws_every_rotation_without_panicking` runs it headlessly.
  - `materials`: right panel (View menu), counts for the 2D layer / 3D checked layers or the whole build, stacks, copy as tab-separated text; cached until what is counted changes.
  - `shapes`: simplified shape of each block state as boxes in 1/16 (slabs, stairs incl. corners and upside-down, doors, trapdoors, fences/gates, walls, panes/bars, carpets, plates, snow layers, torches, buttons, ladders, chests, beds, rails, chains), written facing north and turned. Used by the 2D view (top projection, shaded by height, up to 16 px per block) and the 3D mesh. `Facing` gives the arrow / dot / cross markers drawn in 2D on blocks that look full from above.
  - 3D view: `scene` (CPU: voxel grid of the shown layers/regions, mesh of faces whose neighbour is empty with per-face shade baked into vertex colours, DDA ray picking), `camera` (orbit camera, glam `camera::rh` with the DirectX/WebGPU projection), `render3d` + `view3d.wgsl` (wgpu pipeline drawn inside egui's pass through `egui_wgpu::Callback`; mesh re-uploaded only when its generation changes), `view3d` (input, tooltip). eframe is started with `depth_buffer: 32` and `multisampling: 4`; the pipeline must match (`render3d::DEPTH_BITS`, `MSAA_SAMPLES`).
  - Visibility of layers in 3D is `Document::layer_visible`; changing it or `region_visible` rebuilds the scene (fine for builds of thousands of blocks).
  - egui's proportional font lacks `▲▼↑↓`: use `⏶⏷⬆⬇`.

## Commands

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run --release
```

CI (`.github/workflows/ci.yml`) runs fmt, clippy (warnings are errors) and tests on Windows, macOS and Linux.

## Packaging and releases

- `.github/workflows/release.yml`: on a `vX.Y.Z` tag (must match the Cargo version) builds the Windows installer (Inno Setup, `packaging/windows/mvb.iss`) + portable zip, the macOS universal `.dmg` (`packaging/macos/make-dmg.sh`, ad-hoc signed, not notarized), Linux `.deb` / `.rpm` (metadata in `crates/mvb-app/Cargo.toml`) + tarball, linked against glibc 2.28 with cargo-zigbuild; then creates a **draft** GitHub release. On PRs touching `packaging/**` or the workflow it only uploads artifacts.
- Icons: `packaging/icons/` generated by `cargo run -p icongen` (also embedded as the window icon).
- The app name "MVB" is a placeholder: change it in `APP_NAME` (main.rs), `mvb.iss`, `Info.plist`, `make-dmg.sh`, `mvb.desktop`.

## Git workflow

- One branch per feature (`feat/...`, `fix/...`, `chore/...`), opened as a GitHub PR into `developpement`.
- Merge the PR once CI is green. When `developpement` is validated, open a PR from `developpement` into `main`.
- Merge methods: **squash** for feature branches into `developpement`, **merge commit** for `developpement` into `main` (a squash there would make the two histories diverge).
- `developpement` and `main` are protected: PR required, the 4 CI checks must pass, no force-push.
- Commit messages in English, prefixed with `feat:`, `fix:` or `chore:` (tooling/config only).

## Versioning

`X.Y.Z` (major / minor = features / patch = fixes). The version stays `0.1.0` during the whole v1 development; `1.0.0` marks the complete v1. Versions are tagged (`vX.Y.Z`). The version is defined once, in the root `Cargo.toml` (`workspace.package.version`).

## Product decisions (v1)

- Java Edition only, 1.13+ (no legacy numeric-ID `.schematic`). Bedrock maybe later.
- Formats: `.schem` (Sponge v2/v3), `.litematic`, `.nbt` (vanilla structure), read and write. Native `.mvb` format: NBT compressed with zstd, magic header + format version, superset of all imported data.
- Exporting to a less detailed format must list what will be lost (merged regions, dropped entities...).
- Keep full block states (properties such as `facing`, `half`). Keep entities (item frames, armor stands). Keep block entity data (chests, signs, banners, heads) as raw NBT without displaying it.
- Litematica regions: displayed together at their real positions, with a panel to show, hide or isolate each one.
- Rendering: flat colour per block (generated average-colour table, never ship game textures), simplified shapes that show orientation, block name on hover.
- Y coordinates are relative to the bottom of the build.
- The application name must not contain "Minecraft" (trademark); `MVB` is a placeholder.
