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
//!
//! **The lock also answers the opposite question.** "Is a server running here,
//! and where?" is the same test read the other way round: if the directory can
//! be locked, nobody is serving it, and if it cannot, the holder is alive and
//! has written down where it listens. [`holder`] is that reading, and it is why
//! `yinkote open` needs no separate registry of running servers — and, more
//! importantly, why it never has to guess whether a recorded pid is still
//! alive.

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

/// The process serving a data directory, and where it listens.
///
/// Written by whoever holds the lock and read by whoever is refused it — and by
/// `yinkote open`, which wants the address rather than the refusal. The host is
/// carried because assuming loopback would make `open` confidently produce a
/// URL that answers nothing on a server bound anywhere else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Holder {
    pub pid: u32,
    pub port: u16,
    pub host: String,
}

impl std::fmt::Display for Holder {
    /// A sentence first: this ends up inside a refusal that a person reads.
    /// [`Holder::parse`] is its exact inverse, and `format_and_parse_are_
    /// inverses` keeps them that way.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "pid {}, port {}, host {}", self.pid, self.port, self.host)
    }
}

impl Holder {
    fn describing(port: u16, host: &str) -> Self {
        Self { pid: std::process::id(), port, host: host.to_string() }
    }

    /// Recover a holder from the line it wrote.
    ///
    /// Deliberately tolerant: this file was written by another process, is read
    /// without any lock held, and may be from an older version that wrote
    /// fewer fields. Anything unreadable is `None` rather than an error,
    /// because the caller's next move — say a server is running but not where —
    /// is the same either way.
    pub fn parse(line: &str) -> Option<Self> {
        let field = |name: &str| -> Option<&str> {
            line.split(',').map(str::trim).find_map(|part| {
                let value = part.strip_prefix(name)?.trim();
                (!value.is_empty()).then_some(value)
            })
        };
        Some(Self {
            pid: field("pid")?.parse().ok()?,
            port: field("port")?.parse().ok()?,
            // Older lock files have no host. Loopback is what they meant: it
            // is the default, and it was the only thing this file recorded.
            host: field("host").unwrap_or("127.0.0.1").to_string(),
        })
    }

    /// The address a browser should be pointed at.
    pub fn url(&self) -> String {
        browsable_url(&self.host, self.port)
    }
}

/// An address a person can actually open, from one a socket was bound to.
///
/// A wildcard bind is reachable on loopback but cannot be *typed* as one:
/// `http://0.0.0.0:23130` is not a working URL on any platform, and it is what
/// naively echoing the bind address produces — which the startup line did for
/// a long time, telling everyone who ran `--host 0.0.0.0` to open something
/// that goes nowhere.
pub fn browsable_url(host: &str, port: u16) -> String {
    let host = match host {
        "0.0.0.0" | "" => "127.0.0.1",
        "::" | "[::]" => "[::1]",
        other => other,
    };
    // A bare IPv6 literal has to be bracketed or the port reads as part of
    // the address.
    if host.contains(':') && !host.starts_with('[') {
        format!("http://[{host}]:{port}")
    } else {
        format!("http://{host}:{port}")
    }
}

/// Who is serving this directory, if anyone.
///
/// The liveness test *is* the lock: taking it means nobody holds it, so the
/// file's contents describe a process that has gone. The lock taken here is
/// released immediately — this asks a question, it does not make a claim, and a
/// caller that wants the directory must still go through [`acquire`].
pub fn holder(data_dir: &Path) -> Option<Holder> {
    let path = lock_path(data_dir);
    // Not created if absent: no file means no server has ever run here, and
    // leaving a `.lock` behind after a question was asked is litter.
    let mut file = OpenOptions::new().read(true).write(true).open(&path).ok()?;
    if file.try_lock_exclusive().is_ok() {
        let _ = FileExt::unlock(&file);
        return None;
    }
    let mut line = String::new();
    file.read_to_string(&mut line).ok()?;
    Holder::parse(line.trim())
}

/// Take exclusive use of a data directory, or say who has it.
pub fn acquire(data_dir: &Path, port: u16, host: &str) -> Result<Lock, Denied> {
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
    let _ = file.write_all(Holder::describing(port, host).to_string().as_bytes());
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
        let lock = acquire(&dir, 23130, "127.0.0.1").expect("a fresh directory is free");
        assert_eq!(lock.path(), lock_path(&dir));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_second_is_refused_and_told_who_has_it() {
        let dir = scratch("second");
        let _held = acquire(&dir, 23130, "127.0.0.1").unwrap();

        match acquire(&dir, 23131, "127.0.0.1") {
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
        let first = acquire(&dir, 23130, "127.0.0.1").unwrap();
        drop(first);
        acquire(&dir, 23131, "127.0.0.1").expect("the directory is free again");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_leftover_file_from_a_killed_process_is_not_an_obstacle() {
        // A process that was killed wrote its line and never removed it. With
        // a pid file that is a judgement call; with a lock it is nothing at
        // all.
        let dir = scratch("stale");
        std::fs::write(lock_path(&dir), "pid 999999, port 1").unwrap();
        acquire(&dir, 23130, "127.0.0.1").expect("a file nobody holds is just a file");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_holders_description_replaces_the_previous_one() {
        let dir = scratch("replaced");
        drop(acquire(&dir, 23130, "127.0.0.1").unwrap());
        drop(acquire(&dir, 23199, "127.0.0.1").unwrap());
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
        let _one = acquire(&a, 23130, "127.0.0.1").unwrap();
        let _two = acquire(&b, 23131, "127.0.0.1").expect("a different library is a different lock");
        std::fs::remove_dir_all(&a).ok();
        std::fs::remove_dir_all(&b).ok();
    }

    #[test]
    fn the_refusal_says_what_to_do_about_it() {
        let dir = scratch("message");
        let _held = acquire(&dir, 23130, "127.0.0.1").unwrap();
        let message = acquire(&dir, 23131, "127.0.0.1").unwrap_err().to_string();
        assert!(message.contains("--data-dir"), "no way out is offered: {message}");
        assert!(message.contains("search index"), "no reason is given: {message}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn format_and_parse_are_inverses() {
        // These two are written apart and read apart, by different processes
        // and often different versions. Nothing but this test stops one of
        // them being changed alone.
        let holder = Holder { pid: 4321, port: 23130, host: "127.0.0.1".into() };
        assert_eq!(Holder::parse(&holder.to_string()).as_ref(), Some(&holder));
    }

    #[test]
    fn a_lock_file_without_a_host_still_reads() {
        // Written by a version that recorded only pid and port. Loopback is
        // both the default and the only thing that version could have meant.
        let old = Holder::parse("pid 12, port 23130").expect("an older line is still readable");
        assert_eq!(old.host, "127.0.0.1");
        assert_eq!(old.url(), "http://127.0.0.1:23130");
    }

    #[test]
    fn nonsense_in_the_lock_file_is_not_a_holder() {
        // Read without holding the lock, so a half-written or corrupted line
        // is expected rather than exceptional.
        assert!(Holder::parse("").is_none());
        assert!(Holder::parse("pid , port ").is_none());
        assert!(Holder::parse("pid abc, port 23130").is_none());
        assert!(Holder::parse("pid 12, port 99999999").is_none(), "a port that cannot exist");
    }

    #[test]
    fn a_wildcard_bind_becomes_an_address_a_browser_can_use() {
        // `http://0.0.0.0:23130` is not a working URL anywhere, and it is what
        // naively echoing the bind address would produce.
        let any = |host: &str| Holder { pid: 1, port: 23130, host: host.into() }.url();
        assert_eq!(any("0.0.0.0"), "http://127.0.0.1:23130");
        assert_eq!(any("::"), "http://[::1]:23130");
        assert_eq!(any(""), "http://127.0.0.1:23130");
        // A specific address is kept: assuming loopback for a server bound to
        // the LAN produces a URL that answers nothing.
        assert_eq!(any("192.168.1.4"), "http://192.168.1.4:23130");
        // A bare IPv6 literal needs brackets or the port joins the address.
        assert_eq!(any("fd00::1"), "http://[fd00::1]:23130");
    }

    #[test]
    fn nobody_is_serving_a_directory_that_can_be_locked() {
        let dir = scratch("free");
        // No file at all: no server has ever run here.
        assert!(holder(&dir).is_none());

        // A file left behind by a process that was killed. With a pid file
        // this is the hard case; with a lock it is the same as no file.
        std::fs::write(lock_path(&dir), "pid 999999, port 1, host 127.0.0.1").unwrap();
        assert!(holder(&dir).is_none(), "nothing holds it, so nothing is running");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_running_server_is_found_with_its_address() {
        let dir = scratch("running");
        let _held = acquire(&dir, 23177, "0.0.0.0").unwrap();
        let found = holder(&dir).expect("something holds this directory");
        assert_eq!(found.pid, std::process::id());
        assert_eq!(found.url(), "http://127.0.0.1:23177");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn asking_who_holds_it_does_not_claim_it() {
        // `holder` takes the lock to test it and must give it straight back,
        // or the question would answer itself wrongly the second time and, far
        // worse, `open` would lock out the server it was looking for.
        let dir = scratch("probe");
        std::fs::write(lock_path(&dir), "pid 1, port 2").unwrap();
        assert!(holder(&dir).is_none());
        acquire(&dir, 23130, "127.0.0.1").expect("the probe left it free");
        std::fs::remove_dir_all(&dir).ok();
    }
}
