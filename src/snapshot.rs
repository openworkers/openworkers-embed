//! The V8 snapshot of the runtime: the bindings of openworkers-runtime-v8,
//! compiled once at build time instead of in each new isolate.
//!
//! openworkers-runtime-v8 reads the file at [`PATH`] when it starts. An empty
//! or missing file means no snapshot: isolates start slower and still work.

use std::path::Path;

/// `RUNTIME_SNAPSHOT_PATH` of the build, where the runtime reads the snapshot.
pub const PATH: &str = env!("RUNTIME_SNAPSHOT_PATH");

/// Make the snapshot and write it to [`PATH`]. The program that reads it must
/// be built with the same openworkers-runtime-v8: write it again after each
/// update of that crate.
pub fn write() -> Result<&'static Path, String> {
    let snapshot = openworkers_runtime_v8::snapshot::create_runtime_snapshot()?;

    std::fs::write(PATH, &snapshot.output).map_err(|e| format!("{PATH}: {e}"))?;

    Ok(Path::new(PATH))
}
