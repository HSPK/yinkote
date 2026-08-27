//! Naming the file on disk.
//!
//! A library's storage directory is something people open in a file manager,
//! sync between machines and search with `find`. `paper.pdf` a thousand times
//! over is useless there, and the publisher's own names — `1-s2.0-S0092867420301
//! 21X-main.pdf` — are worse.
//!
//! So the name is rendered from a template, and the template is the user's.
//! Rendering is a pure function of the item and the template, which is what
//! lets the workbench show exactly what a rename would produce before doing any
//! of it. A batch rename nobody can preview is one nobody should run.

use yk_core::model::Item;

/// The default, chosen to sort usefully in a file manager.
///
/// Author first because that is how people look for a paper they half
/// remember; year second because it disambiguates the same author; the title
/// last because it is the part that is too long.
pub const DEFAULT_TEMPLATE: &str = "{author} {year} - {title}";

/// The longest a rendered name may be, before the extension.
///
/// Not a filesystem limit — most allow 255 bytes — but a readability one: a
/// name longer than this is not read, it is scrolled past. Truncating on a
/// character boundary matters here, since half the titles in a real library are
/// not ASCII.
const MAX_STEM: usize = 120;

/// Render a filename for an item, without the extension.
///
/// Missing parts collapse rather than leaving holes: an item with no author
/// must not be named ` 2017 - Title`, and one with nothing at all falls back to
/// its key, which always exists.
pub fn render(template: &str, item: &Item, fallback: &str) -> String {
    let mut out = String::with_capacity(template.len() + 32);
    let mut rest = template;

    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let Some(end) = rest[start..].find('}') else {
            // An unclosed brace is a typo in the template. The rest is dropped
            // rather than printed: a file called `Vaswani {year.pdf` reads as
            // corruption, which is a worse way to report a typo than a short
            // name is.
            rest = "";
            break;
        };
        let name = &rest[start + 1..start + end];
        out.push_str(&value(name, item));
        rest = &rest[start + end + 1..];
    }
    out.push_str(rest);

    let cleaned = tidy(&out);
    if cleaned.is_empty() {
        return sanitise(fallback);
    }
    truncate(&cleaned, MAX_STEM)
}

/// What a placeholder stands for.
fn value(name: &str, item: &Item) -> String {
    let field = |key: &str| item.field(key).unwrap_or_default().to_string();
    let raw = match name {
        // The first author's surname. Not the whole list: a file named after
        // eleven people is a file nobody can read the title of.
        "author" => item
            .creators
            .first()
            .map(|c| {
                c.last_name.clone().or_else(|| c.name.clone()).unwrap_or_default()
            })
            .unwrap_or_default(),
        "authors" => item
            .creators
            .iter()
            .filter_map(|c| c.last_name.clone().or_else(|| c.name.clone()))
            .take(3)
            .collect::<Vec<_>>()
            .join(", "),
        "year" => item.year().map(|y| y.to_string()).unwrap_or_default(),
        "title" => item.title().to_string(),
        "journal" => {
            let container = field("publicationTitle");
            if container.is_empty() {
                field("bookTitle")
            } else {
                container
            }
        }
        "key" => item.key.to_string(),
        "type" => item.item_type.clone(),
        // An unknown placeholder is left visibly empty rather than printed
        // literally: a file called `{авторы} 2017.pdf` looks like corruption.
        _ => String::new(),
    };
    sanitise(&raw)
}

/// Remove what a filesystem cannot hold, and what a person cannot read.
///
/// Deliberately conservative: the same library is opened on Windows, macOS and
/// Linux, and a name that is legal on one and not another turns a synced folder
/// into a stream of errors. So the rule is the strictest of the three.
pub fn sanitise(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for c in raw.chars() {
        match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => out.push(' '),
            // Control characters and the separator that would escape the
            // directory. Newlines appear in titles more often than one expects.
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    // A trailing dot or space is legal on Linux and silently dropped by
    // Windows, which is worse than refusing it: the file is then not where the
    // database says it is.
    out.trim().trim_end_matches('.').trim().to_string()
}

/// Collapse the gaps left by missing parts.
fn tidy(raw: &str) -> String {
    let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    // `Author  - Title` with no year leaves a dangling separator; so does a
    // template whose first placeholder is empty.
    collapsed
        .replace(" - - ", " - ")
        .trim()
        .trim_start_matches('-')
        .trim_end_matches('-')
        .trim()
        .to_string()
}

fn truncate(text: &str, limit: usize) -> String {
    match text.char_indices().nth(limit) {
        Some((cut, _)) => text[..cut].trim().to_string(),
        None => text.to_string(),
    }
}

/// The extension to keep, taken from the current name.
///
/// Kept rather than derived from the content type: the file on disk already has
/// one that opens correctly, and a rename is not the moment to start guessing
/// about formats.
pub fn extension_of(filename: &str) -> String {
    std::path::Path::new(filename)
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default()
}

/// The full name a file should have.
pub fn filename_for(template: &str, parent: &Item, current: &str) -> String {
    format!("{}{}", render(template, parent, parent.key.as_str()), extension_of(current))
}

#[cfg(test)]
mod tests;
