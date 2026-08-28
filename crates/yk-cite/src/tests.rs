//! Tests for citation rendering.
//!
//! Each style is checked against a reference typed out by hand from its own
//! manual, rather than against whatever this code happens to produce. A test
//! that asserts the current output is a test that will agree with any bug.

use super::*;
use yk_core::model::{Creator, Item};

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

#[test]
fn a_dateless_work_says_so_rather_than_leaving_a_gap() {
    // Preprints, web pages and working papers routinely have no year. Printing
    // nothing where it belongs gives "Zhang, W. Undated Work.", which reads as
    // a rendering fault rather than as a fact about the source.
    let mut item = article();
    item.fields.remove("date");
    item.fields.insert("title".into(), serde_json::json!("Undated Work"));
    item.creators = vec![Creator::author("Wei", "Zhang")];

    let apa = reference(&item, styles::find("apa").unwrap(), Format::Text);
    assert!(apa.contains("(n.d.)"), "{apa}");

    // A style that prefers to omit it still omits it — this is described per
    // style, not imposed on all of them.
    let mla = reference(&item, styles::find("mla").unwrap(), Format::Text);
    assert!(!mla.contains("n.d."), "{mla}");
}

#[test]
fn an_organisation_author_gets_its_full_stop() {
    // The stop after an APA author list used to arrive by accident, from the
    // dot on the last initial. An author with no initial — every company,
    // agency and university press — silently lost it.
    let mut item = article();
    item.fields.insert("date".into(), serde_json::json!("2022"));
    item.fields.insert("title".into(), serde_json::json!("Standards Report"));
    item.creators = vec![Creator::single("World Health Organization")];

    let apa = reference(&item, styles::find("apa").unwrap(), Format::Text);
    assert!(apa.starts_with("World Health Organization. (2022)."), "{apa}");
}

#[test]
fn a_personal_author_does_not_get_two_stops() {
    // The other half of the same change: the initial already ends in a stop,
    // and the style now adds one too.
    let mut item = article();
    item.fields.insert("date".into(), serde_json::json!("2019"));
    item.fields.insert("title".into(), serde_json::json!("Many Hands"));
    item.creators = vec![Creator::author("Ada", "Lovelace")];

    let apa = reference(&item, styles::find("apa").unwrap(), Format::Text);
    assert!(apa.starts_with("Lovelace, A. (2019)."), "{apa}");
    assert!(!apa.contains(".."), "{apa}");
}

#[test]
fn an_anonymous_work_is_filed_under_its_title() {
    // Where the year is written before the title, dropping an empty author
    // list left the entry starting "(2020). A Paper…" — a stray parenthesis
    // where a reader expects a name. An anonymous work is alphabetised, and
    // read, by its title.
    let mut item = article();
    item.creators.clear();
    item.fields.insert("title".into(), serde_json::json!("A Paper With No Author"));
    item.fields.insert("date".into(), serde_json::json!("2020"));

    let apa = reference(&item, styles::find("apa").unwrap(), Format::Text);
    assert!(apa.starts_with("A Paper With No Author. (2020)."), "{apa}");
    // And it is not printed twice.
    assert_eq!(apa.matches("A Paper With No Author").count(), 1, "{apa}");

    let chicago = reference(&item, styles::find("chicago").unwrap(), Format::Text);
    assert!(chicago.starts_with("A Paper With No Author. 2020."), "{chicago}");
}

#[test]
fn a_style_that_already_leads_with_the_title_is_untouched() {
    // MLA and IEEE write the title straight after the author, so an empty
    // author segment already falls away and leaves the title leading. The flag
    // says so rather than being applied everywhere and hoping.
    let mut item = article();
    item.creators.clear();
    item.fields.insert("title".into(), serde_json::json!("Anonymous Work"));

    for id in ["mla", "ieee"] {
        let out = reference(&item, styles::find(id).unwrap(), Format::Text);
        assert_eq!(out.matches("Anonymous Work").count(), 1, "{id}: {out}");
    }
}

#[test]
fn a_promoted_title_keeps_the_emphasis_it_would_have_had() {
    // A book's title is italic wherever it appears, including in the author's
    // place. Rendering it through the author segment must not quietly drop
    // that.
    let mut item = article();
    item.creators.clear();
    item.item_type = "book".into();
    item.fields.remove("publicationTitle");
    item.fields.insert("title".into(), serde_json::json!("An Anonymous Book"));

    let chicago = styles::find("chicago").unwrap();
    let italic = chicago.segments.iter().any(|s| s.piece == Piece::Title && s.emphasis != Emphasis::None);
    let html = reference(&item, chicago, Format::Html);
    if italic {
        assert!(html.contains("<i>An Anonymous Book</i>"), "{html}");
    }
}
