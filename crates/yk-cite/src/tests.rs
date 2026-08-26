//! Tests for citation rendering.
//!
//! Each style is checked against a reference typed out by hand from its own
//! manual, rather than against whatever this code happens to produce. A test
//! that asserts the current output is a test that will agree with any bug.

use super::*;
use yk_core::model::Item;

fn article() -> Item {
    let json = serde_json::json!({
        "key": "AAAA1111",
        "libraryId": 1,
        "itemType": "journalArticle",
        "title": "Attention is all you need",
        "publicationTitle": "Advances in Neural Information Processing Systems",
        "volume": "30",
        "issue": "1",
        "pages": "5998\u{2013}6008",
        "date": "2017-06-12",
        "DOI": "10.1000/xyz",
        "creators": [
            { "creatorType": "author", "firstName": "Ashish", "lastName": "Vaswani" },
            { "creatorType": "author", "firstName": "Noam", "lastName": "Shazeer" }
        ],
        "version": 1,
        "dateAdded": 0,
        "dateModified": 0
    });
    serde_json::from_value(json).unwrap()
}

fn with(field: &str, value: serde_json::Value) -> Item {
    let mut item = article();
    item.fields.insert(field.to_string(), value);
    item
}

#[test]
fn renders_apa() {
    let got = reference(&article(), &styles::APA, Format::Text);
    assert_eq!(
        got,
        "Vaswani, A., & Shazeer, N. (2017). Attention is all you need. \
         Advances in Neural Information Processing Systems, 30(1), 5998\u{2013}6008. \
         https://doi.org/10.1000/xyz"
    );
}

#[test]
fn renders_ieee() {
    let got = reference(&article(), &styles::IEEE, Format::Text);
    assert_eq!(
        got,
        "A. Vaswani, and N. Shazeer, \u{201c}Attention is all you need,\u{201d} \
         Advances in Neural Information Processing Systems, vol. 30, no. 1, \
         pp. 5998\u{2013}6008, 2017, doi: 10.1000/xyz"
    );
}

#[test]
fn renders_the_chinese_national_standard() {
    let got = reference(&article(), &styles::GB_T_7714, Format::Text);
    // Surnames capitalised, initials without stops, and the bracketed kind
    // marker that tells a Chinese examiner this is a journal article.
    assert!(got.starts_with("VASWANI A, SHAZEER N."), "{got}");
    assert!(got.contains("you need[J]."), "{got}");
    assert!(got.contains(", 2017, 30(1): 5998"), "{got}");
}

#[test]
fn a_missing_part_takes_its_punctuation_with_it() {
    let mut item = article();
    item.fields.remove("issue");
    let got = reference(&item, &styles::APA, Format::Text);
    // Not `30(), 5998`, which is what happens when punctuation lives between
    // pieces rather than with them.
    assert!(got.contains("Systems, 30, 5998"), "{got}");
    assert!(!got.contains("()"), "{got}");
}

#[test]
fn does_not_double_a_stop_the_publisher_already_wrote() {
    let got = reference(&with("title", "Attention is all you need.".into()), &styles::APA, Format::Text);
    assert!(!got.contains("need.."), "{got}");
    assert!(got.contains("need. Advances"), "{got}");
}

#[test]
fn keeps_a_question_mark_and_does_not_follow_it_with_a_stop() {
    let got = reference(&with("title", "Is attention all you need?".into()), &styles::APA, Format::Text);
    assert!(got.contains("need? Advances"), "{got}");
}

#[test]
fn does_not_end_a_reference_by_gluing_a_stop_to_a_link() {
    let got = reference(&article(), &styles::APA, Format::Text);
    // A copied link that ends in a stop is a dead link, which is exactly the
    // kind of error nobody proofreads a bibliography for.
    assert!(got.ends_with("10.1000/xyz"), "{got}");
}

#[test]
fn ends_an_ordinary_reference_like_a_sentence() {
    let mut item = article();
    item.fields.remove("DOI");
    let got = reference(&item, &styles::APA, Format::Text);
    assert!(got.ends_with("5998\u{2013}6008."), "{got}");
}

#[test]
fn shows_a_url_only_when_there_is_no_doi() {
    let item = with("url", "https://example.org/paper".into());
    let with_doi = reference(&item, &styles::APA, Format::Text);
    assert!(!with_doi.contains("example.org"), "a DOI is the stabler address: {with_doi}");

    let mut without = item.clone();
    without.fields.remove("DOI");
    assert!(reference(&without, &styles::APA, Format::Text).contains("example.org"));
}

#[test]
fn elides_a_long_author_list_without_also_saying_and() {
    let mut item = article();
    for i in 0..6 {
        item.creators.push(serde_json::from_value(serde_json::json!({
            "creatorType": "author",
            "firstName": format!("Given{i}"),
            "lastName": format!("Family{i}")
        })).unwrap());
    }

    let got = reference(&item, &styles::MLA, Format::Text);
    assert!(got.starts_with("Vaswani, Ashish, et al."), "{got}");
    assert!(!got.contains("and, et al"), "{got}");
}

#[test]
fn prints_an_institution_whole() {
    let mut item = article();
    item.creators = vec![serde_json::from_value(serde_json::json!({
        "creatorType": "author",
        "name": "World Health Organization"
    }))
    .unwrap()];

    // Splitting a single-field name on a space would invent an author called
    // "Organization" — and would do the same to most CJK names.
    assert!(reference(&item, &styles::APA, Format::Text).starts_with("World Health Organization"));
}

#[test]
fn finds_the_year_in_whatever_shape_the_date_arrived() {
    for date in ["2017", "2017-06-12", "June 2017", "12/06/2017"] {
        let got = reference(&with("date", date.into()), &styles::APA, Format::Text);
        assert!(got.contains("(2017)"), "{date} -> {got}");
    }
}

#[test]
fn an_undated_work_is_still_rendered() {
    let mut item = article();
    item.fields.remove("date");
    let got = reference(&item, &styles::APA, Format::Text);
    assert!(!got.contains("()"), "{got}");
    assert!(got.contains("Attention is all you need"), "{got}");
}

#[test]
fn italics_only_exist_in_html() {
    assert!(reference(&article(), &styles::APA, Format::Html).contains("<i>Advances"));
    assert!(!reference(&article(), &styles::APA, Format::Text).contains('<'));
}

#[test]
fn escapes_metadata_that_would_otherwise_be_markup() {
    let got = reference(&with("title", "Comparing <b>bold</b> & italic".into()), &styles::APA, Format::Html);
    assert!(got.contains("&lt;b&gt;"), "{got}");
    assert!(got.contains("&amp;"), "{got}");
}

#[test]
fn cites_by_number_in_a_numeric_style_and_by_name_otherwise() {
    assert_eq!(citation(&article(), &styles::IEEE, 3), "[3]");
    assert_eq!(citation(&article(), &styles::APA, 3), "(Vaswani & Shazeer, 2017)");
}

#[test]
fn cites_three_or_more_authors_as_et_al() {
    let mut item = article();
    item.creators.push(
        serde_json::from_value(
            serde_json::json!({ "creatorType": "author", "lastName": "Parmar" }),
        )
        .unwrap(),
    );
    assert_eq!(citation(&item, &styles::APA, 1), "(Vaswani et al., 2017)");
}

#[test]
fn numbers_a_bibliography_in_the_order_it_was_given() {
    let items = vec![article(), article()];
    let out = bibliography(&items, &styles::IEEE, Format::Text);
    assert!(out[0].starts_with("[1] "), "{:?}", out[0]);
    assert!(out[1].starts_with("[2] "), "{:?}", out[1]);

    // An author-date style numbers nothing.
    assert!(!bibliography(&items, &styles::APA, Format::Text)[0].starts_with('['));
}

#[test]
fn every_style_renders_every_item_type_without_panicking() {
    for style in STYLES {
        for item_type in ["journalArticle", "book", "thesis", "webpage", "conferencePaper"] {
            let mut item = article();
            item.item_type = item_type.to_string();
            let got = reference(&item, style, Format::Text);
            assert!(!got.is_empty(), "{} / {item_type}", style.id);
        }
    }
}

#[test]
fn a_style_can_be_found_by_id() {
    assert_eq!(find("gb7714").map(|s| s.id), Some("gb7714"));
    assert!(find("nonesuch").is_none());
}
