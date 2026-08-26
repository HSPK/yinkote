//! Importing libraries from other reference managers.
//!
//! Each format gets a module that *reads* and nothing else: it produces drafts
//! and leaves persisting them to the caller. That keeps the risky part — the
//! parsing of somebody else's file — free of any code that could write, and it
//! is what lets an import be previewed before it is committed.

pub mod zotero;
