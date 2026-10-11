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
