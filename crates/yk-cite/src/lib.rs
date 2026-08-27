//! Rendering citations and bibliographies.
//!
//! This is **not** a CSL processor. CSL is a large specification with a
//! stylesheet language of its own, and pretending to implement it by writing
//! five styles by hand would be a lie told in the type names. What this is: a
//! small description of how a style arranges the parts of a reference, with a
//! handful of well-known styles described in it.
//!
//! The description is the point. Five hand-written formatters would share
//! nothing and drift apart; describing a style as an ordered list of segments
//! means punctuation, name order and emphasis are data, a new style is a
//! constant rather than a function, and every style gets HTML output, name
//! truncation and punctuation tidying for free.
//!
//! Where a style is more subtle than this description allows — Chicago's note
//! form, APA's rule about the twenty-first author — the style is left out
//! rather than approximated. A citation that is subtly wrong is worse than one
//! that was never offered, because nobody proofreads a bibliography.

use yk_core::model::{Creator, Item};

pub mod export;
pub mod import;
mod styles;
pub use styles::{find, STYLES};

#[cfg(test)]
mod tests;

/// A part of a reference, resolved from an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Piece {
    Authors,
    Year,
    Title,
    /// The journal, book or proceedings the work appeared in.
    Container,
    Volume,
    Issue,
    Pages,
    Publisher,
    Doi,
    Url,
    /// GB/T 7714's bracketed kind marker: `[J]` for an article, `[M]` for a
    /// book. No Western style uses one.
    Kind,
}

/// How emphasis is shown. Only HTML output can show any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Emphasis {
    None,
    Italic,
}

/// One part of a reference, with the punctuation that belongs to it.
///
/// The punctuation lives with the piece rather than between pieces so that a
/// missing piece takes its punctuation with it — an article with no issue
/// number must not leave `()` behind.
#[derive(Debug, Clone, Copy)]
pub struct Segment {
    pub piece: Piece,
    pub prefix: &'static str,
    pub suffix: &'static str,
    pub emphasis: Emphasis,
}

impl Segment {
    pub const fn new(piece: Piece, prefix: &'static str, suffix: &'static str) -> Self {
        Self { piece, prefix, suffix, emphasis: Emphasis::None }
    }

    pub const fn italic(piece: Piece, prefix: &'static str, suffix: &'static str) -> Self {
        Self { piece, prefix, suffix, emphasis: Emphasis::Italic }
    }
}

/// Which names are written surname-first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Invert {
    /// Every name: APA, GB/T.
    All,
    /// Only the first, so the list stays alphabetised but reads naturally:
    /// MLA, Chicago.
    First,
    /// None: IEEE writes `A. Vaswani` throughout.
    None,
}

/// How much of a given name is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Given {
    /// `Ashish`
    Full,
    /// `A.`
    Dotted,
    /// `A` — GB/T sets initials without stops.
    Bare,
}

/// How a style writes a list of names.
#[derive(Debug, Clone, Copy)]
pub struct Names {
    pub invert: Invert,
    pub given: Given,
    /// Surnames in capitals, as GB/T requires.
    pub upper_surname: bool,
    /// The most that may be listed in full.
    pub max: usize,
    /// How many are listed once the list is too long — which is not the same
    /// number: MLA allows two authors but drops to **one** plus `et al.` at
    /// three, and a single field cannot say that.
    pub elided_to: usize,
    /// What replaces the rest.
    pub et_al: &'static str,
    pub separator: &'static str,
    /// Before the last name, when they all fit.
    pub last_separator: &'static str,
}

/// A citation style.
pub struct Style {
    pub id: &'static str,
    pub name: &'static str,
    /// Numeric styles cite `[1]`; the rest cite `(Author, year)`.
    pub numeric: bool,
    pub names: Names,
    pub segments: &'static [Segment],
}

/// What a rendered reference is wanted as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Text,
    Html,
}

/// Render one bibliography entry.
pub fn reference(item: &Item, style: &Style, format: Format) -> String {
    let mut out = String::new();
    for segment in style.segments {
        let text = piece(item, segment.piece, style);
        if text.trim().is_empty() {
            continue;
        }
        out.push_str(segment.prefix);
        out.push_str(&emphasise(&escape(&text, format), segment.emphasis, format));
        out.push_str(segment.suffix);
    }
    tidy(&out)
}

/// Render a whole bibliography, in the order given.
///
/// The order is the caller's: a numeric style numbers entries by first
/// appearance in the text, and this cannot know what the text is. Sorting here
/// would silently renumber somebody's paper.
pub fn bibliography(items: &[Item], style: &Style, format: Format) -> Vec<String> {
    items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let body = reference(item, style, format);
            if style.numeric {
                format!("[{}] {body}", i + 1)
            } else {
                body
            }
        })
        .collect()
}

/// Render the marker that goes in the running text.
pub fn citation(item: &Item, style: &Style, number: usize) -> String {
    let body = citation_body(item, style, number);
    if style.numeric {
        format!("[{body}]")
    } else {
        format!("({body})")
    }
}

/// The inside of a citation marker, without its brackets or parentheses.
///
/// A word processor needs to build the marker rather than receive it finished:
/// one field may cite two works (`[1,2]`), and a locator or a prefix — "see
/// Zhang 2020, p. 41" — belongs *inside* the punctuation. Composing that from a
/// finished `(Zhang, 2020)` would mean taking it apart again.
pub fn citation_body(item: &Item, style: &Style, number: usize) -> String {
    if style.numeric {
        return number.to_string();
    }

    let year = year(item);
    let authors = surnames(item);
    let who = match authors.len() {
        0 => item.field("title").unwrap_or_default().to_string(),
        1 => authors[0].clone(),
        2 => format!("{} & {}", authors[0], authors[1]),
        _ => format!("{} et al.", authors[0]),
    };

    if year.is_empty() {
        who
    } else {
        format!("{who}, {year}")
    }
}

fn piece(item: &Item, piece: Piece, style: &Style) -> String {
    let field = |name: &str| item.field(name).unwrap_or_default().to_string();
    match piece {
        Piece::Authors => names(item, &style.names),
        Piece::Year => year(item),
        Piece::Title => field("title"),
        // A book has no container of its own; a chapter's is its book. Falling
        // back keeps one segment list working for both.
        Piece::Container => {
            let container = field("publicationTitle");
            if container.is_empty() {
                field("bookTitle")
            } else {
                container
            }
        }
        Piece::Volume => field("volume"),
        Piece::Issue => field("issue"),
        Piece::Pages => field("pages"),
        Piece::Publisher => field("publisher"),
        Piece::Doi => field("DOI"),
        // A DOI is a stabler address than a URL, so a style that offers both
        // shows only one — printing the same work twice reads as an error.
        Piece::Url => {
            if field("DOI").is_empty() {
                field("url")
            } else {
                String::new()
            }
        }
        Piece::Kind => kind(&item.item_type).to_string(),
    }
}

/// GB/T 7714's kind marker.
fn kind(item_type: &str) -> &'static str {
    match item_type {
        "journalArticle" | "magazineArticle" | "newspaperArticle" => "[J]",
        "book" | "bookSection" => "[M]",
        "conferencePaper" => "[C]",
        "thesis" => "[D]",
        "report" => "[R]",
        "patent" => "[P]",
        "webpage" | "blogPost" => "[EB/OL]",
        _ => "[Z]",
    }
}

/// The year, from whatever shape the date is in.
///
/// Dates arrive as `2017`, `2017-06-12`, `June 2017` and worse, because they
/// come from a dozen publishers. Four consecutive digits are the year in all of
/// them, and nothing else in a date field is four digits.
fn year(item: &Item) -> String {
    let date = item.field("date").unwrap_or_default();
    let bytes: Vec<char> = date.chars().collect();
    bytes
        .windows(4)
        .find(|w| w.iter().all(char::is_ascii_digit))
        .map(|w| w.iter().collect())
        .unwrap_or_default()
}

/// Just the surnames, for an in-text citation.
fn surnames(item: &Item) -> Vec<String> {
    item.creators
        .iter()
        .filter(|c| c.creator_type == "author")
        .map(surname)
        .filter(|s| !s.is_empty())
        .collect()
}

fn surname(creator: &Creator) -> String {
    // A single-field name — an institution, or most CJK names — is indivisible
    // and must be printed whole. Splitting it on a space would turn 张伟 into
    // an author called 伟.
    if let Some(name) = &creator.name {
        return name.clone();
    }
    creator.last_name.clone().unwrap_or_default()
}

/// Write an author list in a style's shape.
fn names(item: &Item, style: &Names) -> String {
    let authors: Vec<&Creator> =
        item.creators.iter().filter(|c| c.creator_type == "author").collect();
    if authors.is_empty() {
        return String::new();
    }

    let elided = authors.len() > style.max;
    let shown = if elided { &authors[..style.elided_to.min(authors.len())] } else { &authors[..] };

    let mut out = String::new();
    for (i, creator) in shown.iter().enumerate() {
        if i > 0 {
            // The last separator is only reached when nothing was elided; a
            // list ending in "et al." must not also say "and".
            let last = !elided && i == shown.len() - 1;
            out.push_str(if last { style.last_separator } else { style.separator });
        }
        let inverted = match style.invert {
            Invert::All => true,
            Invert::First => i == 0,
            Invert::None => false,
        };
        out.push_str(&name(creator, style, inverted));
    }

    if elided {
        out.push_str(style.et_al);
    }
    out
}

fn name(creator: &Creator, style: &Names, inverted: bool) -> String {
    if let Some(whole) = &creator.name {
        return case(whole, style);
    }

    let last = case(creator.last_name.as_deref().unwrap_or_default(), style);
    let first = creator.first_name.as_deref().unwrap_or_default();
    if first.is_empty() {
        return last;
    }

    let given = match style.given {
        Given::Full => first.to_string(),
        Given::Dotted => initials(first, true),
        Given::Bare => initials(first, false),
    };

    if inverted {
        // GB/T sets `VASWANI A` with a space and no comma; everyone else uses a
        // comma. The absence of a stop after the initial is the tell.
        if style.given == Given::Bare {
            format!("{last} {given}")
        } else {
            format!("{last}, {given}")
        }
    } else {
        format!("{given} {last}")
    }
}

/// `Ashish Kumar` becomes `A. K.`, or `AK` where a style omits the stops.
fn initials(first: &str, dotted: bool) -> String {
    let parts: Vec<String> = first
        .split(|c: char| c.is_whitespace() || c == '-')
        .filter(|p| !p.is_empty())
        .filter_map(|p| p.chars().next())
        .map(|c| c.to_uppercase().to_string())
        .collect();

    if dotted {
        parts.iter().map(|p| format!("{p}.")).collect::<Vec<_>>().join(" ")
    } else {
        parts.concat()
    }
}

fn case(text: &str, style: &Names) -> String {
    if style.upper_surname {
        text.to_uppercase()
    } else {
        text.to_string()
    }
}

fn escape(text: &str, format: Format) -> String {
    if format == Format::Text {
        return text.to_string();
    }
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn emphasise(text: &str, emphasis: Emphasis, format: Format) -> String {
    match (emphasis, format) {
        (Emphasis::Italic, Format::Html) => format!("<i>{text}</i>"),
        _ => text.to_string(),
    }
}

/// Clean up the punctuation that falls out of assembling segments.
///
/// A reference is built from parts that each carry their own punctuation, and
/// real metadata already ends in a stop about half the time — publishers put
/// them in titles. Rather than make every style guess, the seams are tidied
/// once, here.
fn tidy(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        // A style's stop after a title the publisher already ended. Only a stop
        // is swallowed: `et al., 2017` proves a comma after one is deliberate.
        if c == '.' && out.ends_with(['.', '?', '!']) {
            continue;
        }
        if c == ' ' && out.ends_with(' ') {
            continue;
        }
        out.push(c);
    }

    let out = out.trim().trim_end_matches([',', ';', ' ']).to_string();
    if out.is_empty() || out.ends_with(['.', '?', '!', '/', '>']) || ends_in_address(&out) {
        out
    } else {
        format!("{out}.")
    }
}

/// Whether a reference ends in something a full stop would be read as part of.
///
/// A DOI ends in characters a stop would be silently glued to, and a reader
/// copying the link would get a dead one. This is the reason a bibliography
/// entry is the one sentence allowed not to end in a stop.
fn ends_in_address(text: &str) -> bool {
    let last = text.rsplit(' ').next().unwrap_or_default();
    last.contains("://") || last.starts_with("10.")
}
