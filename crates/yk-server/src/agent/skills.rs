//! The skills that ship with Yinkote.
//!
//! Written to disk on start rather than compiled into the prompt, for two
//! reasons: a user can read them, and a user can change them. A skill that
//! only exists inside the binary is a black box that cannot be corrected when
//! it gives bad advice about a particular field.
//!
//! Existing files are left alone. Once a folder is on disk it belongs to
//! whoever has been editing it, and silently reverting their work every
//! restart would be worse than shipping nothing.

use std::path::Path;

use yk_agent::skills::SKILL_FILE;
use yk_core::{Error, Result};

pub use yk_agent::skills::{ReadSkill, Skill, Skills};

/// `(folder, contents)` for each skill that ships with the program.
const BUILTINS: &[(&str, &str)] = &[
    ("literature-search", LITERATURE_SEARCH),
    ("screening", SCREENING),
];

/// Write any built-in skill that is not already on disk.
pub fn install_builtins(dir: &Path) -> Result<()> {
    for (name, body) in BUILTINS {
        let folder = dir.join(name);
        let file = folder.join(SKILL_FILE);
        if file.exists() {
            continue;
        }
        std::fs::create_dir_all(&folder)
            .map_err(|e| Error::internal(format!("{}: {e}", folder.display())))?;
        std::fs::write(&file, body)
            .map_err(|e| Error::internal(format!("{}: {e}", file.display())))?;
    }
    Ok(())
}

const LITERATURE_SEARCH: &str = r#"---
name: literature-search
description: Use when asked to find papers on a topic, survey a field, or fill a gap in the library.
---

# Literature search

"Search for papers on X" means search the world, not the library. The library
is where you start and where the results end up; it is not where the answer
comes from.

Work in this order.

## 1. Start with what they already have

Call `library_overview`, then `search_library`. Two reasons this comes first:
they may already own the answer, and what they own tells you the vocabulary of
their field — the terms in their own titles and tags are better queries than
the ones you would invent.

## 2. Then search outside, with `search_external`

This is the step that finds new work. `search_library` cannot: it only sees
what is already owned.

**Never write metadata from memory.** If you have not searched, you do not
know the year, the venue or the DOI, and a wrong year quietly poisons every
citation made from it afterwards. Searching is not optional politeness — it is
the only way you have of being right.

### Choosing where to search

Omit `sources` and all of them are asked, which is right when you do not know
the field. Name them when you do:

| Field | `sources` |
| --- | --- |
| Computer science, physics, mathematics, statistics | `arxiv` — preprints, months ahead of publication |
| Medicine, public health, epidemiology, nursing, trials | `pubmed` — indexes clinical work nothing else does |
| Anything published, any discipline | `crossref` — the version of record, by DOI |
| Cross-disciplinary, citation counts, open access | `openalex` |

Two habits worth keeping:

- **Search several phrasings, not one.** A field usually has more than one name
  for the same idea, and one query finds one of them. "Wastewater-based
  epidemiology" and "sewage surveillance" are the same literature.
- **Be specific.** "Retrieval-augmented generation evaluation" finds work; "AI"
  finds noise.

If a source is listed under `failed`, say so. "PubMed did not answer" is the
difference between "there is nothing" and "we did not look there", and only
one of those means search again.

## 3. Judge before adding

Report what you found with title, authors, year and venue, and say in one line
why each is relevant. Do not add things silently — a library filled with
plausible-looking papers nobody chose is worse than an empty one.

## 4. Add what they choose

Pass the result's `identifier` to `quick_add`. It is a DOI where there is one,
so the record comes from the publisher rather than from the search result,
which is thinner. Adding also queues the PDF, so the paper is there to read.

## 5. Leave it findable

Make or find the collection with `list_collections` and `create_collection`,
then `file_items` what you added into it. Tag with `tag_items`, using the terms
they already use — check the overview's tag list rather than starting a second
vocabulary alongside theirs.

A paper added to no collection and given no tags is a paper they will never
find again, and it will be you who put it there.

## Finishing

Say what you searched, which sources answered, what you added and where you
filed it, and what you deliberately left out. The last part matters most: it is
how they know whether to search again.
"#;

const SCREENING: &str = r#"---
name: screening
description: Use when asked to screen, triage or sort a set of papers against criteria.
---

# Screening a set of papers

## 1. Get the criteria in writing

Before reading anything, state the include and exclude rules as a short list
and show them to the user. Criteria that stay in your head drift halfway
through, and nobody can tell afterwards which rule a paper failed.

## 2. Work from the abstracts

`search_library` with a collection or tag filter gives you the set. For each
paper, decide include, exclude or unsure, and give a one-line reason quoting
the part of the abstract that decided it.

"Unsure" is a real answer. A screening pass that forces every borderline paper
into a bucket hides exactly the papers worth a human's attention.

## 3. Record it where it survives

Write the table to a file with `write_file` — one row per paper, with the key,
the decision and the reason. A decision that exists only in a chat message is
lost the moment the conversation scrolls.

Then tag the papers so the result lives in the library too: `screened:include`,
`screened:exclude`, `screened:unsure`.

## 4. Report the shape, not the list

Say how many landed in each bucket and what the common reason for exclusion
was. If a criterion excluded almost everything, say so — it usually means the
criterion was wrong rather than the field.
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_parses() {
        // A shipped skill that does not parse would be skipped at load with a
        // warning nobody reads, and the feature would look like it is off.
        for (name, body) in BUILTINS {
            let skill = Skill::parse(body)
                .unwrap_or_else(|e| panic!("built-in skill '{name}' does not parse: {e}"));
            assert_eq!(&skill.name, name, "folder name must match the skill name");
            assert!(!skill.body.is_empty());
        }
    }

    #[test]
    fn descriptions_say_when_to_use_the_skill() {
        // The description is all the model sees unprompted. One that describes
        // the contents rather than the occasion never gets picked.
        for (_, body) in BUILTINS {
            let skill = Skill::parse(body).unwrap();
            assert!(
                skill.description.to_lowercase().starts_with("use when"),
                "'{}' should say when it applies: {}",
                skill.name,
                skill.description
            );
        }
    }

    #[test]
    fn installing_writes_them_once_and_then_leaves_them_alone() {
        let dir = tempfile::tempdir().unwrap();
        install_builtins(dir.path()).unwrap();

        let file = dir.path().join("literature-search").join(SKILL_FILE);
        assert!(file.exists());

        // An edited skill belongs to whoever edited it.
        std::fs::write(&file, "---\nname: literature-search\ndescription: Use when mine\n---\nx")
            .unwrap();
        install_builtins(dir.path()).unwrap();
        assert!(std::fs::read_to_string(&file).unwrap().contains("Use when mine"));
    }

    #[test]
    fn what_is_installed_is_what_loads() {
        let dir = tempfile::tempdir().unwrap();
        install_builtins(dir.path()).unwrap();

        let skills = Skills::load_dir(dir.path());
        assert_eq!(skills.len(), BUILTINS.len());
        assert!(skills.get("literature-search").is_some());
    }
}

#[cfg(test)]
mod tool_names {
    use super::*;
    use std::collections::HashSet;

    /// Every `` `name` `` in a skill body that is shaped like a tool call.
    ///
    /// Deliberately loose in what it collects and strict about the shape: a
    /// check that cried wolf on `--flag` or `10.1/x` would be switched off.
    fn tools_named(body: &str) -> HashSet<String> {
        body.split('`')
            .skip(1)
            .step_by(2)
            .map(str::trim)
            .filter(|name| {
                !name.is_empty()
                    && name.contains('_')
                    && name.chars().all(|c| c.is_ascii_lowercase() || c == '_')
                    && !name.starts_with('_')
                    && !name.ends_with('_')
            })
            .map(str::to_string)
            .collect()
    }

    /// The names `tools()` builds, without needing a store to ask for them.
    ///
    /// The read-only five are listed because they are constructed from
    /// structs rather than an enum; the rest come from `ACTIONS`, so a tool
    /// added there is covered without touching this.
    fn available() -> HashSet<String> {
        let mut names: HashSet<String> = crate::agent::actions::ACTIONS
            .iter()
            .map(|a| a.name().to_string())
            .collect();
        for fixed in [
            "search_library",
            "get_item",
            "read_paper",
            "list_references",
            "library_overview",
            "read_skill",
            "read_file",
            "write_file",
            "list_files",
            "run_command",
        ] {
            names.insert(fixed.into());
        }
        names
    }

    /// A skill may only name tools that exist.
    ///
    /// `literature-search` told the assistant to call `search_external` from
    /// the day it was written and there was no such tool. Nothing failed
    /// loudly: the model asked for a tool it had not been given, got nothing,
    /// and answered from its own recollection of the field -- which is the one
    /// source of metadata this program exists to replace.
    ///
    /// Nothing else can catch this. The skill is prose, the tool list is built
    /// elsewhere, and the two meet only inside a model's context at runtime.
    #[test]
    fn every_tool_a_skill_names_exists() {
        let available = available();
        let mut missing = Vec::new();
        for (skill, body) in BUILTINS {
            for named in tools_named(body) {
                if !available.contains(&named) {
                    missing.push(format!("{skill} names `{named}`"));
                }
            }
        }
        missing.sort();
        assert!(
            missing.is_empty(),
            "a skill tells the assistant to call a tool that does not exist:\n  {}",
            missing.join("\n  ")
        );
    }

    #[test]
    fn a_tool_name_is_told_from_an_ordinary_backtick() {
        let named = tools_named(
            "Call `search_external`, then `quick_add`. Not `--flag`, `DOI`, `a b`, \
             `_leading`, `trailing_` or `10.1/x`.",
        );
        assert_eq!(named, ["search_external", "quick_add"].map(String::from).into_iter().collect());
    }

    /// Without this the check above could be vacuous and nobody would know.
    #[test]
    fn the_check_would_notice_one_that_is_missing() {
        assert!(!available().contains("no_such_tool"));
        assert!(tools_named("Use `no_such_tool`.").contains("no_such_tool"));
    }
}
