//! Every background worker that writes must stand aside for a bulk write.
//!
//! The statistics worker asked; the checkpoint worker asked; the embedding
//! worker — the one that writes *most often* — did not. So an import competed
//! with it for the write lock on every batch and lost: `import-archive` failed
//! outright with "database is locked" on any library whose embedding queue was
//! not empty, which is any library still catching up.
//!
//! Restoring an archive is what somebody reaches for after losing something.
//! It is the last operation that should be fragile, and the failure was
//! invisible from inside: the task reported `failed` with a SQLite message,
//! long after whoever asked for it had walked away.
//!
//! A source rule rather than a behavioural test because the failure needs a
//! real server, a populated queue and a race to reproduce — this round found
//! it by running the suite twice, which is not something a test can do.

const WORKERS: &str = include_str!("../src/workers.rs");

/// The workers that write to the database, by name.
///
/// Listed rather than inferred: a worker knows whether it writes, and the
/// alternative is a heuristic over source text that would be wrong quietly.
const WRITERS: [&str; 3] = ["keep_statistics_current", "embedding_worker", "checkpoint_worker"];

/// The body of `fn name(...)`, up to the next top-level `fn`.
fn body_of(name: &str) -> &'static str {
    let start = WORKERS
        .find(&format!("fn {name}("))
        .unwrap_or_else(|| panic!("`{name}` is not in workers.rs any more; this list is stale"));
    let rest = &WORKERS[start..];
    match rest[1..].find("\nfn ") {
        Some(end) => &rest[..=end],
        None => rest,
    }
}

#[test]
fn every_writing_worker_asks_before_writing() {
    let mut deaf = Vec::new();
    for worker in WRITERS {
        if !body_of(worker).contains("bulk_write_running") {
            deaf.push(worker);
        }
    }
    assert!(
        deaf.is_empty(),
        "these workers write during a bulk write and will fight it for the lock: {deaf:?}\n\
         An import that loses that fight fails with \"database is locked\", which is a \
         restore that did nothing."
    );
}

/// The list above is only as good as its accuracy, so it must still describe
/// the file. A worker renamed away would otherwise silently stop being checked.
#[test]
fn the_list_of_writers_still_matches_the_file() {
    for worker in WRITERS {
        assert!(WORKERS.contains(&format!("fn {worker}(")), "{worker} has gone");
    }
    // And nothing else in the file looks like a worker we forgot to classify.
    let declared: Vec<&str> = WORKERS
        .match_indices("\nfn ")
        .map(|(at, _)| {
            let rest = &WORKERS[at + 4..];
            &rest[..rest.find('(').unwrap_or(0)]
        })
        .filter(|name| name.ends_with("_worker") || name.starts_with("keep_"))
        .collect();
    for name in declared {
        assert!(
            WRITERS.contains(&name) || name == "download_worker",
            "`{name}` is a worker nobody has decided about. If it writes to the database it \
             belongs in WRITERS; if it does not, name it here so the next reader knows why."
        );
    }
}
