//! Let the platform check the file we wrote for it.
//!
//! The unit tests assert what is *in* the generated files, which is worth
//! doing but proves only that they say what I meant them to. Whether systemd
//! accepts a unit is systemd's opinion, and it is the only one that matters —
//! the same reasoning as `yk-scrape`'s live source checks: a recorded shape
//! cannot tell you the real thing agrees.
//!
//! `#[ignore]`d because it shells out to a tool that is not everywhere. Run it
//! where it applies:
//!
//! ```text
//! cargo test -p yk-server --test service_unit -- --ignored --nocapture
//! ```

use std::path::Path;
use yk_server::service::{unit_text, Platform};

#[test]
#[ignore = "needs systemd-analyze; run with --ignored"]
fn systemd_accepts_the_unit_we_generate() {
    if Platform::current() != Some(Platform::Linux) {
        println!("  skipped: not Linux");
        return;
    }

    let dir = std::env::temp_dir().join(format!("yk-unit-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("yinkote.service");
    std::fs::write(
        &path,
        unit_text(Platform::Linux, Path::new("/usr/bin/true"), Path::new("/tmp/lib"), 23119),
    )
    .unwrap();

    let output = match std::process::Command::new("systemd-analyze")
        .arg("verify")
        .arg(&path)
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            println!("  skipped: systemd-analyze unavailable ({e})");
            std::fs::remove_dir_all(&dir).ok();
            return;
        }
    };

    let complaints = String::from_utf8_lossy(&output.stderr);
    // It also validates whatever the unit pulls in, and the distribution's own
    // units are not ours to answer for. Only lines naming our file count.
    let ours: Vec<&str> = complaints
        .lines()
        .filter(|l| l.contains("yinkote.service"))
        .collect();
    std::fs::remove_dir_all(&dir).ok();

    assert!(ours.is_empty(), "systemd rejected our unit:\n  {}", ours.join("\n  "));
    println!("  ok: systemd accepts the generated unit");
}

#[test]
#[ignore = "writes into a temporary HOME; run with --ignored"]
fn plutil_accepts_the_launch_agent() {
    if Platform::current() != Some(Platform::MacOs) {
        println!("  skipped: not macOS");
        return;
    }
    let dir = std::env::temp_dir().join(format!("yk-plist-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("com.yinkote.server.plist");
    std::fs::write(
        &path,
        unit_text(Platform::MacOs, Path::new("/usr/bin/true"), Path::new("/tmp/lib"), 23119),
    )
    .unwrap();

    let output = std::process::Command::new("plutil").arg("-lint").arg(&path).output();
    std::fs::remove_dir_all(&dir).ok();
    match output {
        Ok(o) => assert!(o.status.success(), "plutil rejected the agent: {:?}", o),
        Err(e) => println!("  skipped: plutil unavailable ({e})"),
    }
}
