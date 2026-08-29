//! One [`SearchSource`] per service.
//!
//! Each is a URL and a parser; the shapes they answer with live in
//! [`super::parse`], which is where the tests are. The subject lists are what
//! let a caller — or the assistant — route a question about public health to
//! PubMed and one about topology to arXiv.

use async_trait::async_trait;
use yk_core::Result;

use super::{client, encode, get_json, get_text, Found, SearchSource};

pub struct Arxiv {
    http: reqwest::Client,
}

impl Default for Arxiv {
    fn default() -> Self {
        Self { http: client() }
    }
}

#[async_trait]
impl SearchSource for Arxiv {
    fn id(&self) -> &'static str {
        "arxiv"
    }
    fn label(&self) -> &'static str {
        "arXiv"
    }
    fn subjects(&self) -> &'static [&'static str] {
        &["computer science", "physics", "mathematics", "statistics", "quantitative biology"]
    }

    async fn search(&self, query: &str, limit: usize) -> Result<Vec<Found>> {
        // `all:` searches title, abstract and authors together, which is what
        // somebody typing a topic means. Sorted by relevance rather than by
        // date: the newest paper on a subject is rarely the one to read first.
        let url = format!(
            "https://export.arxiv.org/api/query?search_query=all:{}&start=0&max_results={}\
             &sortBy=relevance",
            encode(query),
            limit
        );
        let body = get_text(&self.http, &url, "arxiv").await?;
        Ok(super::arxiv_feed(&body, limit))
    }
}

pub struct Crossref {
    http: reqwest::Client,
}

impl Default for Crossref {
    fn default() -> Self {
        Self { http: client() }
    }
}

#[async_trait]
impl SearchSource for Crossref {
    fn id(&self) -> &'static str {
        "crossref"
    }
    fn label(&self) -> &'static str {
        "Crossref"
    }
    fn subjects(&self) -> &'static [&'static str] {
        // Everything with a DOI, which is most published work in every field.
        &[]
    }

    async fn search(&self, query: &str, limit: usize) -> Result<Vec<Found>> {
        // `query.bibliographic` is the field Crossref means for "a topic or a
        // citation string", rather than `query` which also matches funders.
        let url = format!(
            "https://api.crossref.org/works?query.bibliographic={}&rows={}\
             &select=DOI,title,author,issued,container-title,URL,abstract",
            encode(query),
            limit
        );
        let body = get_json(&self.http, &url, "crossref").await?;
        Ok(super::crossref_works(&body, limit))
    }
}

pub struct PubMed {
    http: reqwest::Client,
}

impl Default for PubMed {
    fn default() -> Self {
        Self { http: client() }
    }
}

#[async_trait]
impl SearchSource for PubMed {
    fn id(&self) -> &'static str {
        "pubmed"
    }
    fn label(&self) -> &'static str {
        "PubMed"
    }
    fn subjects(&self) -> &'static [&'static str] {
        &["medicine", "public health", "epidemiology", "biology", "nursing", "clinical trials"]
    }

    /// Two requests, because E-utilities separates finding from describing:
    /// `esearch` answers with ids and `esummary` turns ids into records.
    async fn search(&self, query: &str, limit: usize) -> Result<Vec<Found>> {
        let search = format!(
            "https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esearch.fcgi\
             ?db=pubmed&retmode=json&sort=relevance&retmax={}&term={}",
            limit,
            encode(query)
        );
        let found = get_json(&self.http, &search, "pubmed").await?;
        let ids: Vec<String> = found
            .pointer("/esearchresult/idlist")
            .and_then(|v| v.as_array())
            .map(|list| list.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        // No second request for no ids: an empty `id=` is a 400 from NCBI,
        // which would be reported as the service being down.
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        let summary = format!(
            "https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esummary.fcgi\
             ?db=pubmed&retmode=json&id={}",
            ids.join(",")
        );
        let body = get_json(&self.http, &summary, "pubmed").await?;
        Ok(super::pubmed_summaries(&body, limit))
    }
}

pub struct OpenAlex {
    http: reqwest::Client,
}

impl Default for OpenAlex {
    fn default() -> Self {
        Self { http: client() }
    }
}

#[async_trait]
impl SearchSource for OpenAlex {
    fn id(&self) -> &'static str {
        "openalex"
    }
    fn label(&self) -> &'static str {
        "OpenAlex"
    }
    fn subjects(&self) -> &'static [&'static str] {
        // Open and cross-disciplinary; the closest thing to Web of Science or
        // Scopus that can be queried without a subscription.
        &[]
    }

    async fn search(&self, query: &str, limit: usize) -> Result<Vec<Found>> {
        let url = format!(
            "https://api.openalex.org/works?search={}&per-page={}",
            encode(query),
            limit
        );
        let body = get_json(&self.http, &url, "openalex").await?;
        Ok(super::openalex_works(&body, limit))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The routing the assistant depends on: a medical question must find a
    /// medical database, and a mathematics one a mathematics server.
    #[test]
    fn the_subjects_are_specific_enough_to_route_by() {
        assert!(PubMed::default().subjects().contains(&"public health"));
        assert!(Arxiv::default().subjects().contains(&"mathematics"));
        // Empty means "anything", which is true of both of these and is not
        // the same as "unknown".
        assert!(Crossref::default().subjects().is_empty());
        assert!(OpenAlex::default().subjects().is_empty());
    }
}
