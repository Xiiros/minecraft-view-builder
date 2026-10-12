# minecraft-view-builder

A fast, lightweight desktop viewer for block-based builds, made to help you reproduce a build in the game layer by layer.

> Work in progress: version 0.1.0 is under development. The final application name is not chosen yet (`mvb` is a placeholder).
>
> Not an official Minecraft product. Not approved by or associated with Mojang or Microsoft.

## Planned features (v1)

- **2D layer view**: one horizontal layer at a time, bottom to top, with zoom, 90° rotation, a reset button, a ghost of the layer below, a coordinate grid and the block name on hover.
- **3D view**: free orbit and zoom, with any combination of layers shown or hidden.
- **Materials list**: blocks needed for the current layer and for the whole build.
- **Import / export**: Sponge Schematic (`.schem`), Litematica (`.litematic`) and vanilla structure (`.nbt`) files, Java Edition 1.13+.
- **Native project format** (`.mvb`) that keeps every detail; exporting to a less detailed format warns about what will be lost.
- English and French interface.
- Windows 10/11, macOS 11+ (Apple Silicon and Intel), Debian/Ubuntu and RHEL/Fedora.

## Installation

Download the file for your system from the [releases](https://github.com/Xiiros/minecraft-view-builder/releases):

| System | File |
|---|---|
| Windows 10/11 (x64) | `mvb-<version>-windows-x64-setup.exe` (installer) or `…-portable.zip` |
| macOS 11+ (Apple Silicon and Intel) | `mvb-<version>-macos-universal.dmg`: drag MVB into Applications |
| Debian 12+, Ubuntu 22.04+ | `mvb_<version>-1_amd64.deb` |
| RHEL / Rocky / Alma 9+, Fedora | `mvb-<version>-1.x86_64.rpm` |
| Other Linux (x64, glibc 2.28+) | `…-linux-x64-portable.tar.gz` |

**macOS:** the app is not notarized by Apple. On first launch macOS refuses to open it: open *System Settings › Privacy & Security* and click *Open Anyway* (once).

**Windows:** SmartScreen may warn about an unknown publisher: *More info › Run anyway*.

## Building from source

Requirements: [Rust](https://rustup.rs) (stable). On Windows, the Visual Studio Build Tools with the "Desktop development with C++" workload are also required.

```sh
cargo run --release            # build and start the application
cargo test --workspace         # run the tests
```

## Project layout

| Path | Content |
|---|---|
| `crates/mvb-core` | Data model and file formats, no GPU dependency, fully unit tested |
| `crates/mvb-app` | Desktop application (egui user interface, wgpu rendering) |

## License

[MIT](LICENSE)
