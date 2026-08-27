//! Skills: instructions the agent loads when it needs them.
//!
//! A skill is a folder with a `SKILL.md` in it. The file opens with YAML-ish
//! frontmatter naming the skill and saying, in one line, when it applies; the
//! rest is the instructions.
//!
//! The point is what *isn't* sent. Putting every procedure the assistant might
//! ever need into the system prompt costs that context on every turn, including
//! the ones that just ask how many papers are about diffusion models. So the
//! prompt carries only the names and the one-line descriptions — enough for the
//! model to recognise a job it has instructions for — and the body is fetched
//! with a tool call when it turns out to be relevant.
//!
//! Skills are files rather than code so that a user can write one. A lab with
//! its own way of screening papers should not need to rebuild the server to
//! teach the assistant about it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};
use yk_ai::{Tool, ToolSpec};
use yk_core::{Error, Result};

/// The file that makes a folder a skill.
pub const SKILL_FILE: &str = "SKILL.md";

/// One skill: what it is called, when to use it, and what to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    pub name: String,
    /// One line. This is what the model sees on every turn, so it has to say
    /// *when* the skill applies, not what it contains.
    pub description: String,
    /// The instructions, sent only when asked for.
    pub body: String,
    /// Where it came from, for diagnostics.
    pub source: Option<PathBuf>,
}

impl Skill {
    /// Parse a `SKILL.md`.
    ///
    /// Frontmatter is delimited by `---` lines and read as flat `key: value`
    /// pairs — deliberately not a YAML parser, because a skill that needs
    /// nested configuration has outgrown being a document.
    pub fn parse(text: &str) -> Result<Self> {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let rest = text
            .trim_start()
            .strip_prefix("---")
            .ok_or_else(|| Error::invalid("a skill must start with --- frontmatter"))?;

        let (front, body) = split_frontmatter(rest)
            .ok_or_else(|| Error::invalid("the frontmatter is never closed with ---"))?;

        let fields = parse_fields(front);
        let name = fields
            .get("name")
            .filter(|v| !v.is_empty())
            .ok_or_else(|| Error::invalid("a skill needs a name"))?
            .clone();
        let description = fields
            .get("description")
            .filter(|v| !v.is_empty())
            .ok_or_else(|| Error::invalid(format!("skill '{name}' needs a description")))?
            .clone();

        Ok(Self { name, description, body: body.trim().to_string(), source: None })
    }

    /// Read one skill folder.
    pub fn read(dir: &Path) -> Result<Self> {
        let path = dir.join(SKILL_FILE);
        let text = std::fs::read_to_string(&path)
            .map_err(|e| Error::invalid(format!("{}: {e}", path.display())))?;
        let mut skill = Self::parse(&text)?;
        skill.source = Some(path);
        Ok(skill)
    }
}

/// Everything after the opening `---`, split at the closing one.
fn split_frontmatter(rest: &str) -> Option<(&str, &str)> {
    let mut offset = 0usize;
    for line in rest.split_inclusive('\n') {
        if line.trim_end() == "---" {
            return Some((&rest[..offset], &rest[offset + line.len()..]));
        }
        offset += line.len();
    }
    None
}

/// Flat `key: value` pairs, quotes stripped.
fn parse_fields(front: &str) -> BTreeMap<String, String> {
    front
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let (key, value) = line.split_once(':')?;
            let value = value.trim();
            let value = value
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
                .unwrap_or(value);
            Some((key.trim().to_ascii_lowercase(), value.to_string()))
        })
        .collect()
}

/// The skills available to an agent.
///
/// Sorted by name so the prompt is stable: a prompt that shuffles between
/// turns defeats every prefix cache in the stack.
#[derive(Debug, Clone, Default)]
pub struct Skills {
    skills: Vec<Skill>,
}

impl Skills {
    pub fn new(mut skills: Vec<Skill>) -> Self {
        skills.sort_by(|a, b| a.name.cmp(&b.name));
        skills.dedup_by(|a, b| a.name == b.name);
        Self { skills }
    }

    /// Load every skill folder under a directory.
    ///
    /// A directory that does not exist is not an error — most installations
    /// will never add one. A skill that does not parse is skipped with a
    /// warning rather than taking the server down with it.
    pub fn load_dir(dir: &Path) -> Self {
        let Ok(entries) = std::fs::read_dir(dir) else { return Self::default() };
        let mut found = Vec::new();
        for entry in entries.flatten() {
            if !entry.path().is_dir() {
                continue;
            }
            match Skill::read(&entry.path()) {
                Ok(skill) => found.push(skill),
                Err(error) => {
                    tracing::warn!(dir = %entry.path().display(), %error, "skipping skill");
                }
            }
        }
        Self::new(found)
    }

    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }

    pub fn len(&self) -> usize {
        self.skills.len()
    }

    pub fn get(&self, name: &str) -> Option<&Skill> {
        self.skills.iter().find(|s| s.name.eq_ignore_ascii_case(name))
    }

    pub fn iter(&self) -> impl Iterator<Item = &Skill> {
        self.skills.iter()
    }

    /// The part of the system prompt that advertises what is available.
    ///
    /// Empty when there are no skills, so an installation without any pays
    /// nothing — not even a paragraph explaining that there are none.
    pub fn prompt_section(&self) -> String {
        if self.skills.is_empty() {
            return String::new();
        }
        let mut out = String::from(
            "\n\nSkills are step-by-step instructions for particular jobs. When one of these \
             matches what you have been asked, call read_skill to get its instructions and \
             follow them before doing anything else:\n",
        );
        for skill in &self.skills {
            out.push_str(&format!("- {}: {}\n", skill.name, skill.description));
        }
        out
    }
}

/// Hands the model the instructions for one skill.
pub struct ReadSkill {
    pub skills: Arc<Skills>,
}

#[async_trait]
impl Tool for ReadSkill {
    fn spec(&self) -> ToolSpec {
        let names: Vec<&str> = self.skills.iter().map(|s| s.name.as_str()).collect();
        ToolSpec {
            name: "read_skill".into(),
            description: format!(
                "Get the full instructions for a skill. Available: {}.",
                names.join(", ")
            ),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "enum": names },
                },
                "required": ["name"],
            }),
        }
    }

    async fn call(&self, _library_id: i64, arguments: Value) -> Result<Value> {
        let name = crate::required_str(&arguments, "name")?;
        let skill = self
            .skills
            .get(&name)
            // Listing what does exist turns a wrong guess into a correction
            // rather than a dead end.
            .ok_or_else(|| {
                Error::invalid(format!(
                    "no skill '{name}'. Available: {}",
                    self.skills.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(", ")
                ))
            })?;
        Ok(json!({ "name": skill.name, "instructions": skill.body }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LITERATURE: &str = r#"---
name: literature-search
description: Finding papers on a topic and adding the good ones to the library.
---

# Literature search

1. Search the library first.
2. Then search externally.
"#;

    #[test]
    fn reads_the_name_and_the_one_line_description() {
        let skill = Skill::parse(LITERATURE).unwrap();
        assert_eq!(skill.name, "literature-search");
        assert!(skill.description.starts_with("Finding papers"));
        assert!(skill.body.starts_with("# Literature search"));
    }

    #[test]
    fn quotes_around_a_value_are_not_part_of_it() {
        let skill = Skill::parse("---\nname: \"x\"\ndescription: 'y'\n---\nbody").unwrap();
        assert_eq!(skill.name, "x");
        assert_eq!(skill.description, "y");
    }

    #[test]
    fn a_colon_in_the_description_survives() {
        // "Use when: a user asks…" is a natural way to write one of these.
        let skill =
            Skill::parse("---\nname: x\ndescription: Use when: asked to screen\n---\nb").unwrap();
        assert_eq!(skill.description, "Use when: asked to screen");
    }

    #[test]
    fn a_skill_without_a_description_is_rejected() {
        // The description is the only part the model sees unprompted, so a
        // skill without one can never be chosen — better to say so at load.
        let err = Skill::parse("---\nname: x\n---\nbody").unwrap_err();
        assert!(err.to_string().contains("description"));
    }

    #[test]
    fn a_plain_markdown_file_is_not_a_skill() {
        assert!(Skill::parse("# Notes\n\nsome prose").is_err());
    }

    #[test]
    fn unclosed_frontmatter_is_rejected_rather_than_swallowing_the_file() {
        assert!(Skill::parse("---\nname: x\ndescription: y\n\nbody").is_err());
    }

    #[test]
    fn the_prompt_lists_what_is_available_and_nothing_else() {
        let skills = Skills::new(vec![Skill::parse(LITERATURE).unwrap()]);
        let prompt = skills.prompt_section();

        assert!(prompt.contains("literature-search"));
        assert!(prompt.contains("Finding papers"));
        // The body is the expensive part and must not be in the prompt.
        assert!(!prompt.contains("Search the library first"));
    }

    #[test]
    fn no_skills_costs_no_prompt() {
        assert_eq!(Skills::default().prompt_section(), "");
    }

    #[test]
    fn skills_are_ordered_so_the_prompt_does_not_shuffle() {
        let make = |n: &str| Skill {
            name: n.into(),
            description: "d".into(),
            body: String::new(),
            source: None,
        };
        let skills = Skills::new(vec![make("z"), make("a"), make("m")]);
        let names: Vec<_> = skills.iter().map(|s| s.name.clone()).collect();
        assert_eq!(names, ["a", "m", "z"]);
    }

    #[tokio::test]
    async fn reading_a_skill_that_does_not_exist_says_what_does() {
        let skills = Arc::new(Skills::new(vec![Skill::parse(LITERATURE).unwrap()]));
        let err = ReadSkill { skills }.call(1, json!({ "name": "nope" })).await.unwrap_err();
        assert!(err.to_string().contains("literature-search"));
    }

    #[tokio::test]
    async fn reading_a_skill_hands_over_the_instructions() {
        let skills = Arc::new(Skills::new(vec![Skill::parse(LITERATURE).unwrap()]));
        let out = ReadSkill { skills }
            .call(1, json!({ "name": "literature-search" }))
            .await
            .unwrap();
        assert!(out["instructions"].as_str().unwrap().contains("Search the library first"));
    }

    #[test]
    fn a_missing_directory_is_simply_no_skills() {
        assert!(Skills::load_dir(Path::new("/nonexistent/skills")).is_empty());
    }
}
