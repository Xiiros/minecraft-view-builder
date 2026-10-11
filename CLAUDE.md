# CLAUDE.md

Desktop viewer for block-based builds (Java Edition 1.13+ schematics), written in Rust. The user speaks French: talk to them in French; code, comments, commit messages and docs are in English.

## Priorities

Performance first: execution speed, low RAM, small disk footprint. Prefer small, focused dependencies and compact data structures (palette + dense index arrays).

## Layout

- `crates/mvb-core`: data model and file formats. No GPU/UI dependency; everything here must be unit tested.
- `crates/mvb-app`: egui + wgpu desktop app (binary `mvb`). UI strings live in `src/i18n.rs` (one `Texts` constant per language, English and French).

## Commands

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run --release
```

CI (`.github/workflows/ci.yml`) runs fmt, clippy (warnings are errors) and tests on Windows, macOS and Linux.

## Git workflow

- One branch per feature (`feat/...`, `fix/...`, `chore/...`), opened as a GitHub PR into `developpement`.
- Merge the PR once CI is green. When `developpement` is validated, open a PR from `developpement` into `main`.
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
