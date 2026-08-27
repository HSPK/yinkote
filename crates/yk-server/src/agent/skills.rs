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

Searching for someone means finding what is relevant, judging it, and leaving
their library better than you found it. Work in this order.

## 1. Start with what they already have

Call `library_overview`, then `search_library`. Two reasons this comes first:
they may already own the answer, and what they own tells you the vocabulary of
their field — the terms in their own titles and tags are better queries than
the ones you would invent.

## 2. Then search outside

Use `search_external` with several phrasings rather than one. A field usually
has more than one name for the same idea, and one query finds one of them.

Prefer specific queries over broad ones. "Retrieval-augmented generation
evaluation" finds work; "AI" finds noise.

## 3. Judge before adding

Report what you found with title, authors, year and venue, and say in one line
why each is relevant. Do not add things silently — a library filled with
plausible-looking papers nobody chose is worse than an empty one.

When the user says to add them, use `quick_add` with the DOI or arXiv id.
Never type the metadata yourself: the publisher's record is better than your
recollection of it, and a wrong year quietly poisons every citation made from
it afterwards.

## 4. Leave it findable

File what you add into the collection the user named, and tag it with the
terms they already use — check the overview's tag list rather than inventing a
new vocabulary alongside theirs.

## Finishing

Say what you searched, what you added, and what you deliberately left out.
The last part matters most: it is how they know whether to search again.
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
