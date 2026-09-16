//! Which part of our spawn configuration costs the time?
//!
//! `CreateProcess` for a 7-13 MB binary measured **306-474 ms** through the
//! supervisor, against **4-5 ms** for the same warm binary via .NET's
//! `Process.Start`. Same file, same machine, ninety times the cost -- so the
//! difference is in how we ask, not in what we are asking for.
//!
//! The supervisor spawns with `CREATE_SUSPENDED | CREATE_NEW_PROCESS_GROUP |
//! CREATE_NO_WINDOW`, three piped stdio handles, a working directory and extra
//! environment. This times each combination against a plain spawn so the cost
//! lands on a specific decision rather than on "our spawn path".
//!
//! Not a test: it launches real programs and reports numbers rather than
//! asserting them, and the answer depends on the machine it runs on.
//!
//!     cargo run -p deck-runtime --bin spawnflags --release -- <path-to-exe>

use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::Instant;

const CREATE_SUSPENDED: u32 = 0x0000_0004;
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Spawns once with the given configuration and returns the `spawn()` cost.
fn measure(exe: &str, flags: u32, piped: bool, cwd: bool) -> Option<u128> {
    let mut cmd = Command::new(exe);
    if flags != 0 {
        cmd.creation_flags(flags);
    }
    if piped {
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
    } else {
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
    }
    if cwd {
        if let Some(parent) = std::path::Path::new(exe).parent() {
            cmd.current_dir(parent);
        }
    }

    let t = Instant::now();
    let mut child = cmd.spawn().ok()?;
    let elapsed = t.elapsed().as_micros();

    // A suspended child never runs and never exits, so it must be killed
    // explicitly -- otherwise this leaks a frozen process per measurement.
    let _ = child.kill();
    let _ = child.wait();
    Some(elapsed)
}

fn run(label: &str, exe: &str, flags: u32, piped: bool, cwd: bool) {
    let mut samples: Vec<u128> = Vec::new();
    for _ in 0..7 {
        if let Some(us) = measure(exe, flags, piped, cwd) {
            samples.push(us);
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    if samples.is_empty() {
        println!("{label:<44} could not spawn");
        return;
    }
    samples.sort_unstable();
    let median = samples[samples.len() / 2];
    println!(
        "{label:<44} median {:>7.1} ms   min {:>7.1} ms",
        median as f64 / 1000.0,
        samples[0] as f64 / 1000.0
    );
}

fn main() {
    // `--self` makes this program spawn its OWN image, to test whether a
    // process launching a copy of itself is treated differently.
    //
    // Launch Deck launching launch-deck.exe measured ~302 ms in `CreateProcess`
    // where every other project cost 4-11 ms, and the same binary spawned from
    // *this* harness cost 4.6 ms. So it is neither the binary nor the flags.
    // The remaining difference is that the parent and the child are the same
    // image -- which is what self-replicating malware looks like, and therefore
    // something an antivirus image-load callback has every reason to inspect
    // more closely.
    //
    // If a plain harness spawning itself is also slow, the cause is the
    // operating system's and not Launch Deck's. The child is spawned suspended
    // and killed immediately, so it never runs and cannot recurse.
    if std::env::args().nth(1).as_deref() == Some("--self") {
        let me = std::env::current_exe().expect("own path");
        let me = me.to_string_lossy().into_owned();
        println!("self-spawn: {me}\n");
        run("a DIFFERENT image (cmd.exe)", "C:\\Windows\\System32\\cmd.exe", CREATE_SUSPENDED, false, false);
        run("its OWN image", &me, CREATE_SUSPENDED, false, false);
        return;
    }

    let Some(exe) = std::env::args().nth(1) else {
        eprintln!("usage: spawnflags <path-to-exe>   |   spawnflags --self");
        std::process::exit(2);
    };

    let size = std::fs::metadata(&exe).map(|m| m.len()).unwrap_or(0);
    println!("{exe}  ({:.1} MB)\n", size as f64 / (1024.0 * 1024.0));

    // Cheapest possible spawn, as the baseline everything else is measured
    // against.
    run("plain, no flags, null stdio", &exe, 0, false, false);
    run("+ piped stdio", &exe, 0, true, false);
    run("+ working directory", &exe, 0, true, true);
    run("+ CREATE_NO_WINDOW", &exe, CREATE_NO_WINDOW, true, true);
    run(
        "+ CREATE_NEW_PROCESS_GROUP",
        &exe,
        CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP,
        true,
        true,
    );
    run(
        "+ CREATE_SUSPENDED  (what we ship)",
        &exe,
        CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP | CREATE_SUSPENDED,
        true,
        true,
    );
    // Isolated, to tell "suspension is expensive" apart from "the combination
    // is expensive".
    run("CREATE_SUSPENDED alone", &exe, CREATE_SUSPENDED, false, false);
}
