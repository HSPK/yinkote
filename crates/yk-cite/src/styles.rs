//! The styles this project ships.
//!
//! Each is a constant, not a function, which is the whole reason for the
//! segment description in the parent module: adding a style is describing one.
//!
//! Only styles that fit the description honestly are here. Chicago's note form
//! and APA's twenty-author rule need machinery this does not have, and a
//! citation that is subtly wrong is worse than one that was never offered.

use crate::{Given, Invert, Names, Piece::*, Segment, Style};

/// Every style, in the order a chooser should show them.
pub static STYLES: &[&Style] = &[&APA, &MLA, &CHICAGO, &IEEE, &GB_T_7714];

/// Look a style up by id.
pub fn find(id: &str) -> Option<&'static Style> {
    STYLES.iter().copied().find(|s| s.id == id)
}

/// American Psychological Association, 7th edition.
pub static APA: Style = Style {
    id: "apa",
    name: "APA 7th edition",
    numeric: false,
    names: Names {
        invert: Invert::All,
        given: Given::Dotted,
        upper_surname: false,
        max: 20,
        elided_to: 20,
        et_al: ", et al.",
        separator: ", ",
        last_separator: ", & ",
    },
    segments: &[
        Segment::new(Authors, "", " "),
        Segment::new(Year, "(", "). "),
        Segment::new(Title, "", ". "),
        Segment::italic(Container, "", ""),
        Segment::italic(Volume, ", ", ""),
        Segment::new(Issue, "(", ")"),
        Segment::new(Pages, ", ", ""),
        Segment::new(Publisher, ". ", ""),
        Segment::new(Doi, ". https://doi.org/", ""),
        Segment::new(Url, ". ", ""),
    ],
};

/// Modern Language Association, 9th edition.
pub static MLA: Style = Style {
    id: "mla",
    name: "MLA 9th edition",
    numeric: false,
    names: Names {
        invert: Invert::First,
        given: Given::Full,
        upper_surname: false,
        // MLA allows two authors and drops to one plus `et al.` at three. These
        // are the rule, not limits chosen for tidiness.
        max: 2,
        elided_to: 1,
        et_al: ", et al.",
        separator: ", ",
        last_separator: ", and ",
    },
    segments: &[
        Segment::new(Authors, "", ". "),
        Segment::new(Title, "\u{201c}", ".\u{201d} "),
        Segment::italic(Container, "", ", "),
        Segment::new(Publisher, "", ", "),
        Segment::new(Volume, "vol. ", ", "),
        Segment::new(Issue, "no. ", ", "),
        Segment::new(Year, "", ", "),
        Segment::new(Pages, "pp. ", ""),
        Segment::new(Doi, ", https://doi.org/", ""),
        Segment::new(Url, ", ", ""),
    ],
};

/// Chicago, author–date form.
pub static CHICAGO: Style = Style {
    id: "chicago",
    name: "Chicago (author–date)",
    numeric: false,
    names: Names {
        invert: Invert::First,
        given: Given::Full,
        upper_surname: false,
        max: 10,
        elided_to: 7,
        et_al: ", et al.",
        separator: ", ",
        last_separator: ", and ",
    },
    segments: &[
        Segment::new(Authors, "", ". "),
        Segment::new(Year, "", ". "),
        Segment::new(Title, "\u{201c}", ".\u{201d} "),
        Segment::italic(Container, "", " "),
        Segment::new(Volume, "", ""),
        Segment::new(Issue, " (", ")"),
        Segment::new(Pages, ": ", ""),
        Segment::new(Publisher, ". ", ""),
        Segment::new(Doi, ". https://doi.org/", ""),
        Segment::new(Url, ". ", ""),
    ],
};

/// IEEE, as used across engineering.
pub static IEEE: Style = Style {
    id: "ieee",
    name: "IEEE",
    numeric: true,
    names: Names {
        invert: Invert::None,
        given: Given::Dotted,
        upper_surname: false,
        max: 6,
        elided_to: 1,
        et_al: ", et al.",
        separator: ", ",
        last_separator: ", and ",
    },
    segments: &[
        Segment::new(Authors, "", ", "),
        Segment::new(Title, "\u{201c}", ",\u{201d} "),
        Segment::italic(Container, "", ", "),
        Segment::new(Publisher, "", ", "),
        Segment::new(Volume, "vol. ", ", "),
        Segment::new(Issue, "no. ", ", "),
        Segment::new(Pages, "pp. ", ", "),
        Segment::new(Year, "", ""),
        Segment::new(Doi, ", doi: ", ""),
    ],
};

/// GB/T 7714—2015, China's national standard.
///
/// Included because a reference manager that cannot produce the style a Chinese
/// thesis is required to use is not usable for a Chinese thesis. It is also the
/// style that most exercises the description: capitalised surnames, initials
/// without stops, and a bracketed kind marker no Western style has.
pub static GB_T_7714: Style = Style {
    id: "gb7714",
    name: "GB/T 7714—2015",
    numeric: true,
    names: Names {
        invert: Invert::All,
        given: Given::Bare,
        upper_surname: true,
        max: 3,
        elided_to: 3,
        et_al: ", \u{7b49}",
        separator: ", ",
        last_separator: ", ",
    },
    segments: &[
        Segment::new(Authors, "", ""),
        Segment::new(Title, ". ", ""),
        Segment::new(Kind, "", ""),
        Segment::new(Container, ". ", ""),
        Segment::new(Publisher, ". ", ""),
        Segment::new(Year, ", ", ""),
        Segment::new(Volume, ", ", ""),
        Segment::new(Issue, "(", ")"),
        Segment::new(Pages, ": ", ""),
        Segment::new(Doi, ". DOI: ", ""),
    ],
};
