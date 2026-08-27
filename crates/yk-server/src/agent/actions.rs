//! What the agent may *do*, as opposed to what it may look up.
//!
//! The read-only tools live beside the provider in `agent.rs`. These are the
//! ones that change the library, and they are deliberately in a file of their
//! own: "what can this thing do to my data" should be answerable by reading one
//! list, not by grepping for `store.items`.
//!
//! Two rules shape all of them.
//!
//! **Every change is recorded.** Each call and its result is kept in the
//! conversation and shown in the transcript at the point it happened, so a
//! library that changed can always be traced to the sentence that changed it.
//! An agent with write access and no audit trail would be untrustworthy in a
//! way no amount of prompting could fix.
//!
//! **Reversible by default.** `trash_items` is offered readily; permanent
//! deletion is a separate tool whose description says exactly what it is, and
//! the model is told to prefer the trash — which is the thing the user can undo.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};
use yk_core::model::{CollectionDraft, ItemDraft, ItemPatch, ItemTag};
use yk_ai::{Tool, ToolSpec};
use yk_core::{Error, Key, Result};
use yk_scrape::ScrapeEngine;
use yk_store::{CitationDraft, Store};

use super::summarise;

/// One thing the agent can do to the library.
///
/// The name, the description, the parameter schema and the behaviour are all
/// selected by this enum, so adding an action means one table entry and one
/// match arm — and a reader can see the whole surface at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    CreateItems,
    UpdateItem,
    TrashItems,
    RestoreItems,
    DeleteItems,
    ListCollections,
    CreateCollection,
    FileItems,
    UnfileItems,
    TagItems,
    UntagItems,
    QuickAdd,
    FetchReferences,
    MissingWorks,
}

/// Everything the agent may change, in one list.
pub const ACTIONS: &[Action] = &[
    Action::CreateItems,
    Action::UpdateItem,
    Action::TrashItems,
    Action::RestoreItems,
    Action::DeleteItems,
    Action::ListCollections,
    Action::CreateCollection,
    Action::FileItems,
    Action::UnfileItems,
    Action::TagItems,
    Action::UntagItems,
    Action::QuickAdd,
    Action::FetchReferences,
    Action::MissingWorks,
];

impl Action {
    pub fn name(self) -> &'static str {
        match self {
            Action::CreateItems => "create_items",
            Action::UpdateItem => "update_item",
            Action::TrashItems => "trash_items",
            Action::RestoreItems => "restore_items",
            Action::DeleteItems => "delete_items",
            Action::ListCollections => "list_collections",
            Action::CreateCollection => "create_collection",
            Action::FileItems => "file_items",
            Action::UnfileItems => "unfile_items",
            Action::TagItems => "tag_items",
            Action::UntagItems => "untag_items",
            Action::QuickAdd => "quick_add",
            Action::FetchReferences => "fetch_references",
            Action::MissingWorks => "missing_works",
        }
    }

    /// Whether this changes the library. Used to mark the transcript, so a
    /// reader can see at a glance which steps only looked.
    pub fn writes(self) -> bool {
        !matches!(self, Action::ListCollections | Action::MissingWorks)
    }

    fn description(self) -> &'static str {
        match self {
            Action::CreateItems => {
                "Add items to the library. Each needs an itemType and a title; any other field \
                 the schema knows (DOI, url, date, publicationTitle, abstractNote…) may be given \
                 alongside. Prefer quick_add when you have a DOI, arXiv id or URL — it fetches \
                 the real metadata instead of trusting yours."
            }
            Action::UpdateItem => {
                "Change fields on one item. Only the fields you pass are touched; the rest are \
                 left alone."
            }
            Action::TrashItems => {
                "Move items to the trash. This is the reversible way to remove something and \
                 should be your default."
            }
            Action::RestoreItems => "Take items back out of the trash.",
            Action::DeleteItems => {
                "Delete items permanently, with their files and annotations. This cannot be \
                 undone. Use trash_items unless the user has explicitly asked for permanent \
                 deletion."
            }
            Action::ListCollections => "List the library's collections.",
            Action::CreateCollection => "Create a collection, optionally inside another one.",
            Action::FileItems => "Put items into a collection.",
            Action::UnfileItems => "Take items out of a collection. The items themselves stay.",
            Action::TagItems => "Add tags to items.",
            Action::UntagItems => "Remove tags from items.",
            Action::QuickAdd => {
                "Add a work by identifier — a DOI, arXiv id, ISBN, PubMed id or URL. The metadata \
                 is fetched from the publisher, so this is always better than writing the fields \
                 yourself."
            }
            Action::FetchReferences => {
                "Fetch what a paper cites, from Crossref. The item needs a DOI."
            }
            Action::MissingWorks => {
                "Works the library cites often and does not hold — the strongest signal of what \
                 is worth acquiring next."
            }
        }
    }

    fn parameters(self) -> Value {
        let keys = json!({
            "type": "array",
            "items": { "type": "string" },
            "description": "Item keys.",
        });

        match self {
            Action::CreateItems => json!({
                "type": "object",
                "properties": {
                    "items": {
                        "type": "array",
                        "description": "Items to create, each an object of fields.",
                        "items": { "type": "object" },
                    },
                },
                "required": ["items"],
            }),
            Action::UpdateItem => json!({
                "type": "object",
                "properties": {
                    "key": { "type": "string" },
                    "fields": { "type": "object", "description": "Field names to new values." },
                },
                "required": ["key", "fields"],
            }),
            Action::TrashItems | Action::RestoreItems | Action::DeleteItems => json!({
                "type": "object",
                "properties": { "keys": keys },
                "required": ["keys"],
            }),
            Action::ListCollections => json!({ "type": "object", "properties": {} }),
            Action::CreateCollection => json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "parentKey": { "type": "string", "description": "Optional parent." },
                },
                "required": ["name"],
            }),
            Action::FileItems | Action::UnfileItems => json!({
                "type": "object",
                "properties": { "collectionKey": { "type": "string" }, "keys": keys },
                "required": ["collectionKey", "keys"],
            }),
            Action::TagItems | Action::UntagItems => json!({
                "type": "object",
                "properties": {
                    "keys": keys,
                    "tags": { "type": "array", "items": { "type": "string" } },
                },
                "required": ["keys", "tags"],
            }),
            Action::QuickAdd => json!({
                "type": "object",
                "properties": {
                    "text": { "type": "string", "description": "A DOI, arXiv id, ISBN or URL." },
                },
                "required": ["text"],
            }),
            Action::FetchReferences => json!({
                "type": "object",
                "properties": { "key": { "type": "string" } },
                "required": ["key"],
            }),
            Action::MissingWorks => json!({
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "description": "How many, 1-50. Default 15." },
                },
            }),
        }
    }
}

/// The agent's hands.
pub struct LibraryAction {
    pub action: Action,
    pub store: Store,
    pub scrape: Arc<ScrapeEngine>,
}

#[async_trait]
impl Tool for LibraryAction {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: self.action.name().into(),
            description: self.action.description().into(),
            parameters: self.action.parameters(),
        }
    }

    async fn call(&self, lib: i64, arguments: Value) -> Result<Value> {
        match self.action {
            Action::CreateItems => {
                let drafts: Vec<ItemDraft> = arguments["items"]
                    .as_array()
                    .ok_or_else(|| Error::invalid("items must be an array"))?
                    .iter()
                    .map(draft_from)
                    .collect::<Result<Vec<_>>>()?;

                let results = self.store.items.create_many(lib, drafts).await?;
                let created: Vec<Value> =
                    results.iter().filter_map(|r| r.as_ref().ok()).map(summarise).collect();
                let failed = results.len() - created.len();
                Ok(json!({ "created": created.len(), "failed": failed, "items": created }))
            }

            Action::UpdateItem => {
                let key = key_of(&arguments, "key")?;
                let fields = arguments
                    .get("fields")
                    .cloned()
                    .ok_or_else(|| Error::invalid("fields is required"))?;
                let patch = serde_json::from_value(json!({ "fields": fields }))
                    .map_err(|e| Error::invalid(format!("bad patch: {e}")))?;
                let item = self.store.items.update(lib, &key, patch, None).await?;
                Ok(summarise(&item))
            }

            Action::TrashItems => {
                let n = self.store.items.set_trashed(lib, &keys_of(&arguments)?, true).await?;
                Ok(json!({ "trashed": n }))
            }

            Action::RestoreItems => {
                let n = self.store.items.set_trashed(lib, &keys_of(&arguments)?, false).await?;
                Ok(json!({ "restored": n }))
            }

            Action::DeleteItems => {
                let n = self.store.items.delete(lib, &keys_of(&arguments)?).await?;
                Ok(json!({ "deleted": n, "permanent": true }))
            }

            Action::ListCollections => {
                let all = self.store.collections.list(lib).await?;
                Ok(json!({
                    "collections": all
                        .iter()
                        .map(|c| json!({
                            "key": c.key.as_str(),
                            "name": c.name,
                            "items": c.item_count,
                        }))
                        .collect::<Vec<_>>(),
                }))
            }

            Action::CreateCollection => {
                let name = yk_agent::required_str(&arguments, "name")?;
                let parent = match arguments.get("parentKey").and_then(Value::as_str) {
                    Some(p) if !p.trim().is_empty() => Some(parse_key(p)?),
                    _ => None,
                };
                let created = self
                    .store
                    .collections
                    .create(lib, CollectionDraft { name, parent_key: parent, ..Default::default() })
                    .await?;
                Ok(json!({ "key": created.key.as_str(), "name": created.name }))
            }

            Action::FileItems => {
                let collection = key_of(&arguments, "collectionKey")?;
                let n = self
                    .store
                    .items
                    .add_to_collection(lib, &collection, &keys_of(&arguments)?)
                    .await?;
                Ok(json!({ "filed": n }))
            }

            Action::UnfileItems => {
                let collection = key_of(&arguments, "collectionKey")?;
                let n = self
                    .store
                    .items
                    .remove_from_collection(lib, &collection, &keys_of(&arguments)?)
                    .await?;
                Ok(json!({ "removed": n }))
            }

            Action::TagItems | Action::UntagItems => {
                let keys = keys_of(&arguments)?;
                let tags: Vec<String> = arguments["tags"]
                    .as_array()
                    .ok_or_else(|| Error::invalid("tags must be an array"))?
                    .iter()
                    .filter_map(|t| t.as_str().map(str::to_string))
                    .filter(|t| !t.trim().is_empty())
                    .collect();
                if tags.is_empty() {
                    return Err(Error::invalid("no tags given"));
                }

                let adding = self.action == Action::TagItems;

                // Read the batch, then write the batch. "Tag everything about
                // transformers" is one instruction; doing it as a transaction
                // per item made the agent's cheapest-sounding request the
                // slowest thing it could do.
                let items = self.store.items.get_many(lib, &keys).await?;
                let mut patches: Vec<(Key, ItemPatch)> = Vec::new();
                for item in items {
                    let mut next: Vec<ItemTag> = item.tags.clone();
                    for tag in &tags {
                        let held = next.iter().position(|t| &t.tag == tag);
                        match (adding, held) {
                            // A tag the agent applied is the user's own rather
                            // than an automatic one: they asked for it in words.
                            (true, None) => next.push(ItemTag { tag: tag.clone(), r#type: 0 }),
                            (false, Some(i)) => {
                                next.remove(i);
                            }
                            _ => continue,
                        }
                    }
                    if next.len() != item.tags.len() {
                        let patch = serde_json::from_value(json!({ "tags": next }))
                            .map_err(|e| Error::internal(e.to_string()))?;
                        patches.push((item.key, patch));
                    }
                }

                let changed = self
                    .store
                    .items
                    .update_many(lib, patches)
                    .await?
                    .into_iter()
                    .filter(Result::is_ok)
                    .count();
                Ok(json!({ "changed": changed }))
            }

            Action::QuickAdd => {
                let text = yk_agent::required_str(&arguments, "text")?;
                let resolved = self.scrape.resolve_text(&text, 1).await;
                let found = resolved
                    .into_iter()
                    .next()
                    .ok_or_else(|| Error::invalid(format!("nothing found for {text}")))?;
                let item = self.store.items.create(lib, found.draft).await?;
                Ok(summarise(&item))
            }

            Action::FetchReferences => {
                let key = key_of(&arguments, "key")?;
                let item = self.store.items.get(lib, &key).await?;
                let doi = item
                    .field("DOI")
                    .map(str::trim)
                    .filter(|d| !d.is_empty())
                    .ok_or_else(|| Error::invalid("that paper has no DOI"))?;

                let found = yk_scrape::resolver::Crossref::default().references(doi).await?;
                let drafts: Vec<CitationDraft> = found
                    .iter()
                    .map(|r| CitationDraft {
                        fingerprint: r.fingerprint().unwrap_or_default(),
                        doi: r.doi.clone().unwrap_or_default(),
                        label: r.label(),
                        year: r.year,
                    })
                    .collect();
                let stored = self.store.relations.set_citations(lib, &key, drafts).await?;
                let cites = self.store.relations.cites(lib, &key).await?;
                Ok(json!({
                    "stored": stored,
                    "alreadyInLibrary": cites.iter().filter(|c| c.key.is_some()).count(),
                }))
            }

            Action::MissingWorks => {
                let limit = arguments["limit"].as_u64().unwrap_or(15).clamp(1, 50) as u32;
                Ok(json!({ "works": self.store.relations.missing(lib, limit).await? }))
            }
        }
    }
}

fn parse_key(raw: &str) -> Result<Key> {
    Key::parse(raw.trim()).map_err(|_| Error::invalid(format!("not a valid key: {raw}")))
}

fn key_of(arguments: &Value, field: &str) -> Result<Key> {
    parse_key(&yk_agent::required_str(arguments, field)?)
}

fn keys_of(arguments: &Value) -> Result<Vec<Key>> {
    let list =
        arguments["keys"].as_array().ok_or_else(|| Error::invalid("keys must be an array"))?;
    if list.is_empty() {
        return Err(Error::invalid("no keys given"));
    }
    list.iter().map(|k| parse_key(k.as_str().unwrap_or_default())).collect()
}

/// Turn the model's object into a draft.
///
/// Structure is pulled out by name and everything else is a field — the same
/// rule the browser connector follows, and for the same reason: this project's
/// item types were drawn from Zotero's, so an unlisted field is almost
/// certainly one it has.
fn draft_from(value: &Value) -> Result<ItemDraft> {
    let object = value.as_object().ok_or_else(|| Error::invalid("each item must be an object"))?;
    let item_type = object.get("itemType").and_then(Value::as_str).unwrap_or("document");

    let mut draft = ItemDraft::new(item_type);
    for (name, field) in object {
        if matches!(name.as_str(), "itemType" | "creators" | "tags" | "key") || field.is_null() {
            continue;
        }
        if field.is_string() || field.is_number() || field.is_boolean() {
            draft.fields.insert(name.clone(), field.clone());
        }
    }

    if let Some(list) = object.get("creators").and_then(Value::as_array) {
        draft.creators =
            list.iter().filter_map(|c| serde_json::from_value(c.clone()).ok()).collect();
    }
    if let Some(list) = object.get("tags").and_then(Value::as_array) {
        draft.tags = list
            .iter()
            .filter_map(|t| match t {
                Value::String(s) => Some(ItemTag { tag: s.clone(), r#type: 0 }),
                Value::Object(_) => serde_json::from_value(t.clone()).ok(),
                _ => None,
            })
            .collect();
    }

    // A model that has been told to look things up sometimes invents an item
    // anyway. An untitled one is the shape that mistake takes, and it is worth
    // refusing rather than storing.
    if draft.fields.get("title").and_then(Value::as_str).unwrap_or_default().trim().is_empty() {
        return Err(Error::invalid("an item needs a title"));
    }
    Ok(draft)
}

#[cfg(test)]
mod tests;
