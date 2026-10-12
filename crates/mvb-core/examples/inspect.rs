//! Development tool: loads build files and prints a one-line summary for each.
//! Each file is also saved to `.mvb` and exported to every format, then read back in memory to
//! check that nothing is lost.
//!
//! ```sh
//! cargo run --release -p mvb-core --example inspect -- house.nbt tower.mvb
//! ```

use std::path::Path;
use std::process::ExitCode;

use mvb_core::formats::FileFormat;
use mvb_core::{Build, mvb};

fn load(path: &Path) -> Result<Build, String> {
    let format = path
        .extension()
        .and_then(|e| e.to_str())
        .and_then(FileFormat::from_extension)
        .ok_or("unsupported extension")?;
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    format.read(&bytes).map_err(|e| e.to_string())
}

fn summary(build: &Build) -> String {
    let blocks: usize = build
        .regions
        .iter()
        .flat_map(|r| r.blocks())
        .filter(|&&b| build.palette.get(b).is_some_and(|s| !s.is_air()))
        .count();
    let size = build
        .bounds()
        .map(|(min, max)| {
            format!(
                "{}x{}x{}",
                max.x - min.x + 1,
                max.y - min.y + 1,
                max.z - min.z + 1
            )
        })
        .unwrap_or_else(|| "empty".into());
    let block_entities: usize = build.regions.iter().map(|r| r.block_entities.len()).sum();
    let entities: usize = build.regions.iter().map(|r| r.entities.len()).sum();
    format!(
        "{size}, {blocks} blocks, {} states, {block_entities} block entities, {entities} entities",
        build.palette.len()
    )
}

fn main() -> ExitCode {
    let mut failures = 0;
    for arg in std::env::args().skip(1) {
        let path = Path::new(&arg);
        let result = load(path).and_then(|build| {
            let saved = mvb::write(&build).map_err(|e| format!("save: {e}"))?;
            let reloaded = mvb::read(&saved).map_err(|e| format!("reload: {e}"))?;
            if reloaded != build {
                return Err("reloaded .mvb differs from the original".into());
            }
            for format in FileFormat::ALL {
                let extension = format.extension();
                let bytes = format
                    .write(&build)
                    .map_err(|e| format!("export .{extension}: {e}"))?;
                let reread = format
                    .read(&bytes)
                    .map_err(|e| format!("re-read .{extension}: {e}"))?;
                if !reread.same_content(&build) {
                    return Err(format!("exported .{extension} differs from the original"));
                }
            }
            Ok(build)
        });
        match result {
            Ok(build) => println!("ok    {arg}: {}", summary(&build)),
            Err(error) => {
                failures += 1;
                println!("ERROR {arg}: {error}");
            }
        }
    }
    if failures > 0 {
        eprintln!("{failures} file(s) failed");
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
