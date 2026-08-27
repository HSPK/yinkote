//! Tests for filename rendering.
//!
//! Rendering is pure so it can be previewed, and previewable so a batch rename
//! is something a person can agree to rather than discover afterwards.

use super::*;
use yk_core::model::{Creator, Item, ItemDraft};
use yk_core::Key;

fn item(title: &str, surname: Option<&str>, date: Option<&str>) -> Item {
    let mut draft = ItemDraft::new("journalArticle").with_field("title", title);
    if let Some(date) = date {
        draft = draft.with_field("date", date);
    }
    if let Some(surname) = surname {
        draft = draft.with_creator(Creator {
            last_name: Some(surname.into()),
            first_name: Some("A".into()),
            ..Default::default()
        });
    }
    draft.into_item(Key::parse("AAAA1111").unwrap(), 1, 0)
}

#[test]
fn renders_the_default_the_way_a_folder_should_sort() {
    let got = render(DEFAULT_TEMPLATE, &item("Attention is all you need", Some("Vaswani"), Some("2017-06")), "x");
    assert_eq!(got, "Vaswani 2017 - Attention is all you need");
}

#[test]
fn a_missing_part_collapses_rather_than_leaving_a_hole() {
    // `  2017 - Title` and `Vaswani  - Title` are how a template betrays that
    // the metadata is incomplete, and both look like a bug in the program.
    let no_author = render(DEFAULT_TEMPLATE, &item("A paper", None, Some("2017")), "x");
    assert_eq!(no_author, "2017 - A paper");

    let no_year = render(DEFAULT_TEMPLATE, &item("A paper", Some("Vaswani"), None), "x");
    assert_eq!(no_year, "Vaswani - A paper");
}

#[test]
fn an_item_with_nothing_at_all_is_named_after_its_key() {
    let bare = ItemDraft::new("document").into_item(Key::parse("BBBB2222").unwrap(), 1, 0);
    // The key always exists, which is exactly why it is the fallback.
    assert_eq!(render(DEFAULT_TEMPLATE, &bare, bare.key.as_str()), "BBBB2222");
}

#[test]
fn refuses_characters_that_would_escape_the_directory() {
    let sneaky = item("../../etc/passwd", Some("A/B"), Some("2020"));
    let got = render(DEFAULT_TEMPLATE, &sneaky, "x");
    // The property that matters is that no separator survives: without one,
    // a name cannot leave the directory chosen for it. `storage::safe_filename`
    // applies the same rule again at the point of writing, which is where a
    // guard belongs.
    assert!(!got.contains('/'), "{got}");
    assert!(!got.contains('\\'), "{got}");
}

#[test]
fn uses_the_strictest_rules_of_the_three_platforms() {
    // The same library gets opened on Windows, macOS and Linux. A name that is
    // legal on one and not another turns a synced folder into a stream of
    // errors, so the rule is the strictest.
    let got = render("{title}", &item(r#"What: is "this" | that?"#, None, None), "x");
    for bad in [':', '"', '|', '?', '*', '<', '>', '\\'] {
        assert!(!got.contains(bad), "{bad} survived in {got}");
    }
}

#[test]
fn drops_a_trailing_dot_that_windows_would_silently_drop() {
    // Legal on Linux, quietly removed by Windows — which is worse than being
    // refused, because the file is then not where the database says it is.
    let got = render("{title}", &item("Is this a paper.", None, None), "x");
    assert!(!got.ends_with('.'), "{got}");
}

#[test]
fn handles_a_newline_in_a_title() {
    // Publishers put them in more often than one would believe.
    let got = render("{title}", &item("A title\nwith a break", None, None), "x");
    assert_eq!(got, "A title with a break");
}

#[test]
fn shortens_a_very_long_title_on_a_character_boundary() {
    let long = "扩散模型".repeat(80);
    let got = render("{title}", &item(&long, None, None), "x");
    // Slicing by byte would panic here; a name this long is scrolled past
    // rather than read anyway.
    assert!(got.chars().count() <= 120);
}

#[test]
fn keeps_the_extension_the_file_already_opens_with() {
    let parent = item("A paper", Some("Vaswani"), Some("2017"));
    assert_eq!(
        filename_for(DEFAULT_TEMPLATE, &parent, "1-s2.0-S009286742030121X-main.pdf"),
        "Vaswani 2017 - A paper.pdf"
    );
    // A rename is not the moment to start guessing about formats.
    assert_eq!(filename_for("{key}", &parent, "notes.txt"), "AAAA1111.txt");
    assert_eq!(filename_for("{key}", &parent, "no-extension"), "AAAA1111");
}

#[test]
fn an_unknown_placeholder_is_empty_rather_than_printed() {
    // A file called `{авторы} 2017.pdf` looks like corruption.
    let got = render("{nonesuch} {year}", &item("A", None, Some("2017")), "x");
    assert_eq!(got, "2017");
}

#[test]
fn an_unclosed_brace_is_a_typo_not_a_failure() {
    let got = render("{author} {year", &item("A", Some("Vaswani"), Some("2017")), "x");
    assert_eq!(got, "Vaswani");
}

#[test]
fn lists_a_few_authors_when_asked_but_not_eleven() {
    let mut draft = ItemDraft::new("journalArticle").with_field("title", "Many hands");
    for i in 0..11 {
        draft = draft.with_creator(Creator {
            last_name: Some(format!("Author{i}")),
            ..Default::default()
        });
    }
    let many = draft.into_item(Key::parse("CCCC3333").unwrap(), 1, 0);

    // A file named after eleven people is one whose title cannot be read.
    let got = render("{authors}", &many, "x");
    assert_eq!(got.matches(',').count(), 2);
}
