//! Tests for the download queue.

use super::{state, DownloadDraft};
use crate::Store;

fn draft(key: &str, url: &str) -> DownloadDraft {
    DownloadDraft { item_key: key.into(), url: url.into(), title: "A paper".into() }
}

#[tokio::test]
async fn asking_twice_for_one_file_is_one_request() {
    let s = Store::in_memory().unwrap();
    let lib = s.default_library;

    s.downloads.enqueue(lib, vec![draft("AAAA1111", "https://x/p.pdf")]).await.unwrap();
    s.downloads.enqueue(lib, vec![draft("AAAA1111", "https://x/p.pdf")]).await.unwrap();

    // A double-clicked button is the commonest way this happens.
    assert_eq!(s.downloads.list(lib, 10).await.unwrap().len(), 1);
}

#[tokio::test]
async fn takes_one_download_at_a_time_and_marks_it_taken() {
    let s = Store::in_memory().unwrap();
    let lib = s.default_library;
    s.downloads
        .enqueue(lib, vec![draft("A", "https://x/1.pdf"), draft("B", "https://x/2.pdf")])
        .await
        .unwrap();

    let first = s.downloads.claim(lib).await.unwrap().unwrap();
    let second = s.downloads.claim(lib).await.unwrap().unwrap();

    // Claiming and marking happen together, or two workers take the same row.
    assert_ne!(first.id, second.id);
    assert_eq!(first.state, state::RUNNING);
    assert_eq!(first.attempts, 1);
    assert!(s.downloads.claim(lib).await.unwrap().is_none());
}

#[tokio::test]
async fn a_failure_keeps_the_reason() {
    let s = Store::in_memory().unwrap();
    let lib = s.default_library;
    s.downloads.enqueue(lib, vec![draft("A", "https://x/1.pdf")]).await.unwrap();
    let job = s.downloads.claim(lib).await.unwrap().unwrap();

    s.downloads.fail(job.id, "403 Forbidden").await.unwrap();

    // The reason is what the user needs to judge whether retrying is worth
    // anything; logging it puts it where they will never look.
    let row = &s.downloads.list(lib, 10).await.unwrap()[0];
    assert_eq!(row.state, state::FAILED);
    assert_eq!(row.error, "403 Forbidden");
}

#[tokio::test]
async fn retrying_puts_it_back_and_clears_the_old_reason() {
    let s = Store::in_memory().unwrap();
    let lib = s.default_library;
    s.downloads.enqueue(lib, vec![draft("A", "https://x/1.pdf")]).await.unwrap();
    let job = s.downloads.claim(lib).await.unwrap().unwrap();
    s.downloads.fail(job.id, "timed out").await.unwrap();

    s.downloads.retry(lib, &[job.id]).await.unwrap();

    let row = &s.downloads.list(lib, 10).await.unwrap()[0];
    assert_eq!(row.state, state::WAITING);
    assert_eq!(row.error, "", "a stale reason beside a waiting row is a lie");
    // The attempt count survives, because "this has failed four times" is worth
    // knowing and is exactly what a cleared error would hide.
    assert_eq!(row.attempts, 1);
}

#[tokio::test]
async fn asking_again_for_something_that_failed_is_a_retry() {
    let s = Store::in_memory().unwrap();
    let lib = s.default_library;
    s.downloads.enqueue(lib, vec![draft("A", "https://x/1.pdf")]).await.unwrap();
    let job = s.downloads.claim(lib).await.unwrap().unwrap();
    s.downloads.fail(job.id, "gone").await.unwrap();

    // Pressing the button again on a failed row means "try again", not
    // "nothing happens because I already asked".
    s.downloads.enqueue(lib, vec![draft("A", "https://x/1.pdf")]).await.unwrap();
    assert_eq!(s.downloads.list(lib, 10).await.unwrap()[0].state, state::WAITING);
}

#[tokio::test]
async fn a_finished_download_is_not_asked_for_again() {
    let s = Store::in_memory().unwrap();
    let lib = s.default_library;
    s.downloads.enqueue(lib, vec![draft("A", "https://x/1.pdf")]).await.unwrap();
    let job = s.downloads.claim(lib).await.unwrap().unwrap();
    s.downloads.succeed(job.id, 4096).await.unwrap();

    s.downloads.enqueue(lib, vec![draft("A", "https://x/1.pdf")]).await.unwrap();
    assert!(s.downloads.claim(lib).await.unwrap().is_none(), "the file is already here");
}

#[tokio::test]
async fn puts_what_needs_a_decision_at_the_top() {
    let s = Store::in_memory().unwrap();
    let lib = s.default_library;
    s.downloads
        .enqueue(
            lib,
            vec![draft("A", "https://x/1.pdf"), draft("B", "https://x/2.pdf")],
        )
        .await
        .unwrap();
    let first = s.downloads.claim(lib).await.unwrap().unwrap();
    s.downloads.succeed(first.id, 10).await.unwrap();
    let second = s.downloads.claim(lib).await.unwrap().unwrap();
    s.downloads.fail(second.id, "nope").await.unwrap();

    // A finished row needs nothing from anybody; a failed one needs a decision.
    assert_eq!(s.downloads.list(lib, 10).await.unwrap()[0].state, state::FAILED);
}

#[tokio::test]
async fn clearing_keeps_what_still_needs_attention() {
    let s = Store::in_memory().unwrap();
    let lib = s.default_library;
    s.downloads
        .enqueue(lib, vec![draft("A", "https://x/1.pdf"), draft("B", "https://x/2.pdf")])
        .await
        .unwrap();
    let done = s.downloads.claim(lib).await.unwrap().unwrap();
    s.downloads.succeed(done.id, 1).await.unwrap();

    assert_eq!(s.downloads.clear_finished(lib).await.unwrap(), 1);
    let left = s.downloads.list(lib, 10).await.unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].state, state::WAITING);
}

#[tokio::test]
async fn an_empty_url_is_not_a_download() {
    let s = Store::in_memory().unwrap();
    let lib = s.default_library;
    s.downloads.enqueue(lib, vec![draft("A", "   ")]).await.unwrap();
    assert!(s.downloads.list(lib, 10).await.unwrap().is_empty());
}
