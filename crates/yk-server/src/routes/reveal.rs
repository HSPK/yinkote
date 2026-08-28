//! Show a stored file in the desktop's own file manager.
//!
//! This only makes sense because of what Yinkote is: a server the user started
//! on their own machine. The same endpoint on a hosted service would be
//! meaningless at best.
//!
//! Two rules hold the security of this whole module, and neither is subtle:
//!
//! 1. **The client never supplies a path.** It supplies an item key; the path
//!    comes from our own storage layout. A `path` parameter here would be a
//!    remote "open anything on the disk" primitive, reachable from any page the
//!    browser happens to have open.
//! 2. **No shell, ever.** [`command`] returns a program and an argument vector
//!    which are passed to `Command` directly. There is no string for a filename
//!    containing `;` or `$(…)` to be interpreted in.
//!
//! It is also honest about failing. On a machine with no desktop session there
//! is no file manager to show anything, and answering "done" would be a lie the
//! user cannot see through — nothing visibly happens either way.

use std::ffi::OsString;
use std::path::{Path as FsPath, PathBuf};

use axum::extract::{Path, State};
use axum::routing::post;
use axum::{Json, Router};
use serde_json::json;
use yk_core::{Error, Result};

use super::key;
use crate::error::ApiResult;
use crate::state::App;

pub fn router() -> Router<App> {
    Router::new().route("/libraries/:lib/items/:key/reveal", post(reveal))
}

async fn reveal(
    State(app): State<App>,
    Path((lib, k)): Path<(i64, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    let target = locate(&app, lib, &key(&k)?).await?;
    let (program, args) = command(std::env::consts::OS, &target, &desktop())?;

    // Spawned, not awaited. A file manager that has to cold-start takes
    // seconds, and the request is finished the moment the process exists —
    // holding the connection open would make a working reveal look broken.
    std::process::Command::new(&program)
        .args(&args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| Error::internal(format!("could not run {}: {e}", program.to_string_lossy())))?;

    Ok(Json(json!({
        "revealed": target.display().to_string(),
        "with": program.to_string_lossy(),
    })))
}

/// Where the file for this item is.
///
/// An attachment reveals its own file; anything else reveals the first
/// attachment it has, because "show me this paper on disk" is what the user
/// means when they ask it of a paper.
async fn locate(app: &App, lib: i64, key: &yk_core::Key) -> Result<PathBuf> {
    let item = app.store().items.get(lib, key).await?;
    let attachment = if item.item_type == "attachment" {
        item
    } else {
        app.store()
            .items
            .children_of(lib, std::slice::from_ref(key))
            .await?
            .into_iter()
            .find(|c| c.item_type == "attachment" && c.field("filename").is_some_and(|f| !f.is_empty()))
            .ok_or_else(|| Error::not_found(format!("{} has no file to show", item.key)))?
    };

    let filename = attachment
        .field("filename")
        .filter(|f| !f.is_empty())
        .ok_or_else(|| Error::not_found(format!("no file recorded for {}", attachment.key)))?;

    let path = app.storage().path(&attachment.key, filename);
    if !path.exists() {
        // The row says there is a file and the disk disagrees. Saying so is
        // more useful than opening the folder and letting the user hunt for
        // something that is not there.
        return Err(Error::not_found(format!("{} is recorded but missing from disk", filename)));
    }
    Ok(path)
}

/// Which desktop session, if any, is available to show something in.
fn desktop() -> Option<String> {
    for name in ["WAYLAND_DISPLAY", "DISPLAY"] {
        if let Ok(value) = std::env::var(name) {
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    None
}

/// The program and arguments that show `path` selected in a file manager.
///
/// Pure, so the interesting part — which is per-platform and impossible to
/// exercise in CI — is testable. Note that every platform is asked to *select*
/// the file rather than open its folder: landing in a directory of two hundred
/// PDFs having to find one by name is not what the user asked for.
fn command(os: &str, path: &FsPath, session: &Option<String>) -> Result<(OsString, Vec<OsString>)> {
    match os {
        // `-R` reveals; plain `open` would launch Preview.
        "macos" => Ok(("open".into(), vec!["-R".into(), path.into()])),

        // Explorer's own syntax: the comma is part of the flag, and the path
        // must be attached to it rather than passed as a separate argument.
        "windows" => {
            let mut select = OsString::from("/select,");
            select.push(path);
            Ok(("explorer.exe".into(), vec![select]))
        }

        // Freedesktop: `xdg-open` on a *file* opens it in a PDF reader, which
        // is emphatically not revealing it. `ShowItems` is the interface that
        // means "select this in the file manager", and every major Linux file
        // manager implements it.
        _ => {
            if session.is_none() {
                return Err(Error::invalid(
                    "no desktop session to show the file in — Yinkote is running headless",
                ));
            }
            let uri = file_uri(path);
            Ok((
                "dbus-send".into(),
                vec![
                    "--session".into(),
                    "--dest=org.freedesktop.FileManager1".into(),
                    "--type=method_call".into(),
                    "/org/freedesktop/FileManager1".into(),
                    "org.freedesktop.FileManager1.ShowItems".into(),
                    OsString::from(format!("array:string:{uri}")),
                    "string:".into(),
                ],
            ))
        }
    }
}

/// A `file://` URI for a local path.
///
/// Percent-encoding is not decoration here: a filename with a space or a `#`
/// produces a URI that names a different file, or no file at all, and library
/// filenames come from paper titles.
fn file_uri(path: &FsPath) -> String {
    let mut out = String::from("file://");
    for byte in path.to_string_lossy().as_bytes() {
        match byte {
            b'/' | b'-' | b'_' | b'.' | b'~' => out.push(*byte as char),
            b if b.is_ascii_alphanumeric() => out.push(*b as char),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> Option<String> {
        Some(":0".to_string())
    }

    #[test]
    fn macos_reveals_rather_than_opens() {
        let (program, args) = command("macos", FsPath::new("/tmp/a.pdf"), &None).unwrap();
        assert_eq!(program, "open");
        // Without -R this opens the PDF in Preview, which is a different
        // feature that happens to look like it worked.
        assert_eq!(args, vec![OsString::from("-R"), OsString::from("/tmp/a.pdf")]);
    }

    #[test]
    fn windows_attaches_the_path_to_the_select_flag() {
        let (program, args) = command("windows", FsPath::new(r"C:\lib\a.pdf"), &None).unwrap();
        assert_eq!(program, "explorer.exe");
        // Explorer wants `/select,C:\path` as one argument. Split into two and
        // it silently opens the user's Documents folder instead.
        assert_eq!(args, vec![OsString::from(r"/select,C:\lib\a.pdf")]);
    }

    #[test]
    fn linux_asks_the_file_manager_to_select_it() {
        let (program, args) = command("linux", FsPath::new("/tmp/a.pdf"), &session()).unwrap();
        assert_eq!(program, "dbus-send");
        assert!(args.iter().any(|a| a == "array:string:file:///tmp/a.pdf"), "{args:?}");
        assert!(args.iter().any(|a| a == "org.freedesktop.FileManager1.ShowItems"));
    }

    #[test]
    fn a_headless_machine_is_told_so_rather_than_lied_to() {
        // Nothing visible happens either way, so "ok" would be indistinguishable
        // from working.
        let err = command("linux", FsPath::new("/tmp/a.pdf"), &None).unwrap_err();
        assert_eq!(err.kind(), yk_core::ErrorKind::Invalid);
        assert!(err.to_string().contains("headless"), "{err}");
    }

    #[test]
    fn filenames_that_need_encoding_survive_the_uri() {
        // Library filenames are made from paper titles, so spaces, colons and
        // hashes are the normal case, not the exotic one.
        assert_eq!(file_uri(FsPath::new("/a b/c#d.pdf")), "file:///a%20b/c%23d.pdf");
        // A literal percent has to become %25, or a filename that already
        // looks encoded decodes into a different one.
        assert_eq!(file_uri(FsPath::new("/x/Zhang%20&.pdf")), "file:///x/Zhang%2520%26.pdf");
    }

    #[test]
    fn a_hostile_filename_stays_one_argument() {
        // There is no shell in this path, and this is the test that says so: a
        // filename full of shell metacharacters comes back as exactly one
        // argument, unmodified.
        let nasty = FsPath::new("/tmp/a;rm -rf ~$(whoami).pdf");
        let (_, args) = command("macos", nasty, &None).unwrap();
        assert_eq!(args[1], OsString::from(nasty));

        let (_, args) = command("linux", nasty, &session()).unwrap();
        let uri = args.iter().find(|a| a.to_string_lossy().starts_with("array:")).unwrap();
        let uri = uri.to_string_lossy();
        assert!(!uri.contains(';') && !uri.contains('$') && !uri.contains(' '), "{uri}");
    }

    #[test]
    fn unicode_filenames_are_encoded_bytewise() {
        // UTF-8 per byte, which is what a file:// URI wants.
        assert_eq!(file_uri(FsPath::new("/\u{4e2d}.pdf")), "file:///%E4%B8%AD.pdf");
    }
}
