//! One server per data directory.
//!
//! **What goes wrong without this.** Two copies pointed at one directory start
//! happily today, and nothing about it looks broken until it is: two embedding
//! workers race on the same queue, two checkpoint workers take the database
//! exclusively at unrelated moments, two task registries each show half the
//! jobs, and — the one that actually loses data from the user's point of view —
//! each holds its *own* in-memory copy of the vector index, so a paper added in
//! one is unsearchable in the other until both are restarted.
//!
//! None of that produces an error message. It produces a library that behaves
//! differently depending on which port you happened to open.
//!
//! **Why an advisory lock rather than a PID file.** A PID file has to be
//! cleaned up, and a program that was killed never cleans up — so every stale
//! file becomes a judgement call about whether that process is still alive,
//! which is a different question on every platform and is racy on all of them.
//! An advisory lock is released by the kernel when the process ends, however it
//! ends. There is nothing to clean up and nothing to guess.
//!
//! **What is written in the file.** Whatever is known about the holder, so the
//! refusal can name it. The lock does not depend on the contents — a truncated
//! or empty file locks exactly as well — which is why writing it is allowed to
//! fail quietly.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};

use fs2::FileExt;

/// A held lock. Dropping it, or the process ending, releases it.
#[derive(Debug)]
pub struct Lock {
    _file: File,
    path: PathBuf,
}

impl Lock {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Why a directory could not be claimed.
#[derive(Debug)]
pub enum Denied {
    /// Somebody else has it. The string describes them as far as is known.
    Held(String),
    /// The lock file itself could not be made or opened.
    Unusable(std::io::Error),
}

impl std::fmt::Display for Denied {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Held(who) => write!(
                f,
                "another Yinkote is already using this data directory ({who}).\n\
                 Two servers sharing one library corrupt each other's search index \
                 quietly, so this one will not start.\n\
                 Stop the other, or pass a different --data-dir."
            ),
            Self::Unusable(e) => write!(f, "could not lock the data directory: {e}"),
        }
    }
}

/// The file that represents the claim.
pub fn lock_path(data_dir: &Path) -> PathBuf {
    data_dir.join(".lock")
}

/// Describe this process, for whoever is refused next.
fn describe(port: u16) -> String {
    format!("pid {}, port {port}", std::process::id())
}

/// Take exclusive use of a data directory, or say who has it.
pub fn acquire(data_dir: &Path, port: u16) -> Result<Lock, Denied> {
    let path = lock_path(data_dir);
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(Denied::Unusable)?;

    if file.try_lock_exclusive().is_err() {
        // Read who has it *before* reporting. The holder wrote this; we hold
        // no lock, so it may be mid-write or empty, and neither is a reason to
        // fail differently.
        let mut who = String::new();
        let _ = file.read_to_string(&mut who);
        let who = who.trim();
        return Err(Denied::Held(if who.is_empty() {
            "unknown process".to_string()
        } else {
            who.to_string()
        }));
    }

    // Ours now. Leaving the previous holder's line here would make the next
    // refusal name a process that has been gone for a week.
    let _ = file.set_len(0);
    let _ = file.rewind();
    let _ = file.write_all(describe(port).as_bytes());
    let _ = file.flush();

    Ok(Lock { _file: file, path })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("yk-lock-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_first_caller_gets_it() {
        let dir = scratch("first");
        let lock = acquire(&dir, 23130).expect("a fresh directory is free");
        assert_eq!(lock.path(), lock_path(&dir));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_second_is_refused_and_told_who_has_it() {
        let dir = scratch("second");
        let _held = acquire(&dir, 23130).unwrap();

        match acquire(&dir, 23131) {
            Err(Denied::Held(who)) => {
                // Naming the holder is the difference between a message
                // somebody can act on and one they can only be annoyed by.
                assert!(who.contains("pid"), "who = {who:?}");
                assert!(who.contains("23130"), "the port it is serving on: {who:?}");
            }
            other => panic!("expected a refusal, got {other:?}"),
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn releasing_it_lets_the_next_one_in() {
        // The point of an advisory lock: nothing is cleaned up, and the next
        // caller does not have to work out whether a recorded pid is alive.
        let dir = scratch("released");
        let first = acquire(&dir, 23130).unwrap();
        drop(first);
        acquire(&dir, 23131).expect("the directory is free again");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_leftover_file_from_a_killed_process_is_not_an_obstacle() {
        // A process that was killed wrote its line and never removed it. With
        // a pid file that is a judgement call; with a lock it is nothing at
        // all.
        let dir = scratch("stale");
        std::fs::write(lock_path(&dir), "pid 999999, port 1").unwrap();
        acquire(&dir, 23130).expect("a file nobody holds is just a file");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_holders_description_replaces_the_previous_one() {
        let dir = scratch("replaced");
        drop(acquire(&dir, 23130).unwrap());
        drop(acquire(&dir, 23199).unwrap());
        let contents = std::fs::read_to_string(lock_path(&dir)).unwrap();
        assert!(contents.contains("23199"), "{contents:?}");
        // Otherwise the next refusal names a process that left long ago.
        assert!(!contents.contains("23130"), "{contents:?}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn different_directories_do_not_contend() {
        let a = scratch("dir-a");
        let b = scratch("dir-b");
        let _one = acquire(&a, 23130).unwrap();
        let _two = acquire(&b, 23131).expect("a different library is a different lock");
        std::fs::remove_dir_all(&a).ok();
        std::fs::remove_dir_all(&b).ok();
    }

    #[test]
    fn the_refusal_says_what_to_do_about_it() {
        let dir = scratch("message");
        let _held = acquire(&dir, 23130).unwrap();
        let message = acquire(&dir, 23131).unwrap_err().to_string();
        assert!(message.contains("--data-dir"), "no way out is offered: {message}");
        assert!(message.contains("search index"), "no reason is given: {message}");
        std::fs::remove_dir_all(&dir).ok();
    }
}
