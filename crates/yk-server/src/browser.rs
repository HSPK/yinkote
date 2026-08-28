//! Hand a URL to whatever the user's desktop considers a browser.
//!
//! This exists for one person: somebody who ran `yinkote service install`, has
//! a server starting at login, and no habit of typing addresses into a terminal
//! to reach it. Until there is a tray icon, `yinkote open` is how they get to
//! their library.
//!
//! It follows the same two rules as [`crate::routes::reveal`], for the same
//! reasons:
//!
//! 1. **No shell, ever.** [`command`] returns a program and an argument vector
//!    passed straight to `Command`. On Windows the usual advice is `cmd /c
//!    start <url>`, which is a shell parsing a string we assembled — `&` in a
//!    URL ends the command there. `explorer.exe <url>` does the same job with
//!    no interpreter in the middle.
//! 2. **The per-platform choice is a pure function**, because it is exactly the
//!    part that cannot be exercised on the machine running the tests.
//!
//! Note that `xdg-open` is right here and was wrong for revealing a file: given
//! a *file* it launches a PDF reader, but given a *URL* it launches the
//! browser, which is precisely what is wanted.

use std::ffi::OsString;

use yk_core::{Error, Result};

/// Whether there is a desktop session to open anything in.
///
/// Shared with nothing: on Windows and macOS a logged-in user always has one,
/// and on Linux the environment says so.
fn desktop() -> bool {
    if cfg!(any(target_os = "windows", target_os = "macos")) {
        return true;
    }
    ["WAYLAND_DISPLAY", "DISPLAY"]
        .iter()
        .any(|name| std::env::var(name).is_ok_and(|v| !v.is_empty()))
}

/// The program and arguments that open `url` in the user's browser.
pub fn command(os: &str, url: &str) -> (OsString, Vec<OsString>) {
    match os {
        "macos" => ("open".into(), vec![url.into()]),
        // Explorer hands the URL to the default protocol handler. Unlike the
        // reveal case there is no `/select,` to attach it to, so it is simply
        // its own argument.
        "windows" => ("explorer.exe".into(), vec![url.into()]),
        _ => ("xdg-open".into(), vec![url.into()]),
    }
}

/// What happened when we tried.
#[derive(Debug, PartialEq, Eq)]
pub enum Opened {
    /// A browser was launched.
    With(String),
    /// There is no desktop session here. Not an error: on a headless machine
    /// the useful answer is the address itself, and claiming to have opened a
    /// browser that cannot exist is the one response the user cannot check.
    Headless,
}

/// Launch a browser at `url`, if this machine has one.
pub fn open(url: &str) -> Result<Opened> {
    if !desktop() {
        return Ok(Opened::Headless);
    }
    let (program, args) = command(std::env::consts::OS, url);

    // Spawned, not waited on. A cold browser start takes seconds, and some
    // launchers stay in the foreground for the life of the window.
    std::process::Command::new(&program)
        .args(&args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| Error::internal(format!("could not run {}: {e}", program.to_string_lossy())))?;

    Ok(Opened::With(program.to_string_lossy().into_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_platform_gets_its_own_launcher() {
        assert_eq!(command("macos", "http://x/").0, "open");
        assert_eq!(command("windows", "http://x/").0, "explorer.exe");
        assert_eq!(command("linux", "http://x/").0, "xdg-open");
    }

    #[test]
    fn the_url_is_one_argument_and_is_not_rewritten() {
        // A URL with a query string is the case that breaks `cmd /c start`,
        // where `&` separates commands. Here it stays one untouched argument
        // because nothing parses it.
        let url = "http://127.0.0.1:23130/?a=1&b=2";
        for os in ["macos", "windows", "linux"] {
            let (_, args) = command(os, url);
            assert_eq!(args, vec![OsString::from(url)], "{os}");
        }
    }
}
