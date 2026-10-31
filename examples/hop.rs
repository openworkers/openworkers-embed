//! The invoke path of openworkers-runtime-v8, without V8: a waiter sends a
//! message to a scheduler task, which spawns an op task, which answers on a
//! channel the waiter reads. Each case puts the waiter and the ops in a
//! different place. Embed runs case 4.
//!
//!     cargo run --release --example hop

use std::time::Instant;
use tokio::sync::mpsc;

const N: u32 = 200_000;

async fn roundtrips(local: bool) -> f64 {
    let (sched_tx, mut sched_rx) = mpsc::unbounded_channel::<u32>();
    let (cb_tx, mut cb_rx) = mpsc::unbounded_channel::<u32>();

    let scheduler = async move {
        while let Some(v) = sched_rx.recv().await {
            let cb_tx = cb_tx.clone();
            let op = async move {
                let _ = cb_tx.send(v + 1);
            };

            if local {
                tokio::task::spawn_local(op);
            } else {
                tokio::spawn(op);
            }
        }
    };

    if local {
        tokio::task::spawn_local(scheduler);
    } else {
        tokio::spawn(scheduler);
    }

    let start = Instant::now();

    for i in 0..N {
        sched_tx.send(i).unwrap();
        let _ = cb_rx.recv().await.unwrap();
    }

    start.elapsed().as_nanos() as f64 / N as f64
}

fn main() {
    for _ in 0..2 {
        // 1. Waiter is a LocalSet task, ops are tokio::spawn (Embed before c4cca79)
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let set = tokio::task::LocalSet::new();
        let ns = set.block_on(&rt, async {
            tokio::task::spawn_local(roundtrips(false)).await.unwrap()
        });
        println!("LocalSet task + tokio::spawn ops:  {ns:.0} ns");

        // 2. Waiter is the block_on future of the LocalSet itself, ops are tokio::spawn
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let set = tokio::task::LocalSet::new();
        let ns = set.block_on(&rt, roundtrips(false));
        println!("LocalSet block_on future + spawn:  {ns:.0} ns");

        // 3. Waiter is the block_on future of the runtime, no LocalSet
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let ns = rt.block_on(roundtrips(false));
        println!("runtime block_on + tokio::spawn:   {ns:.0} ns");

        // 4. Waiter is a runtime task, ops are tokio::spawn (Embed since c4cca79)
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let ns = rt.block_on(async { tokio::spawn(roundtrips(false)).await.unwrap() });
        println!("runtime task + tokio::spawn:       {ns:.0} ns");

        // 5. All spawn_local
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let set = tokio::task::LocalSet::new();
        let ns = set.block_on(&rt, async {
            tokio::task::spawn_local(roundtrips(true)).await.unwrap()
        });
        println!("LocalSet task + spawn_local ops:   {ns:.0} ns");
    }
}
