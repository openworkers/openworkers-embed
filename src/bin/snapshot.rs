//! Write the V8 snapshot of the runtime to `RUNTIME_SNAPSHOT_PATH`.
//!
//!     cargo run --release --bin snapshot

fn main() {
    match openworkers_embed::snapshot::write() {
        Ok(path) => println!("snapshot written: {}", path.display()),
        Err(e) => {
            eprintln!("snapshot failed: {e}");
            std::process::exit(1);
        }
    }
}
