//! SQL-level retrievers over the FTS5 indexes maintained by `yk-store`.

use rusqlite::{params, Connection};
use yk_core::{text, Result};
use yk_store::sql_err;

/// Column weights for BM25: a title hit matters far more than a body hit.
const W_TITLE: f64 = 10.0;
const W_CREATORS: f64 = 6.0;
const W_BODY: f64 = 1.0;
const W_TAGS: f64 = 4.0;

/// Upper bound on chunk queries per fuzzy search; keeps the worst case bounded.
const MAX_CHUNKS: usize = 5;

/// `CROSS JOIN` is load-bearing in both statements below. With a plain `JOIN`
/// SQLite is free to reorder, and it will happily drive from `items` — scanning
/// every row and probing the FTS index once per row. That turned a 5 ms query
/// into an 18 s one on a 100k-item library. `CROSS JOIN` pins the virtual table
/// as the outer loop. `tests/query_plan.rs` guards this.
/// Scoping a hit to its library costs a rowid lookup per candidate row, and a
/// keyword query has thousands of them. It looks like the obvious thing to
/// optimise and it is not: measured with the server's own pragmas — a 64 MiB
/// page cache and mmap — the join is worth about 7 ms of a 21 ms query, and a
/// covering index carrying only `library_id` and `deleted` saves *nothing*,
/// because those rows are already in memory. An earlier attempt appeared to win
/// 33% only because the probe ran with SQLite's 2 MB default cache.
///
/// Dropping the join would be faster and wrong: the index holds every library.
fn bm25_sql() -> String {
    format!(
        // `INDEXED BY` because ranking reaches `items` once per *match*, not
        // once per returned row: bm25 must score everything before it can keep
        // the best few hundred, so a common word costs twenty thousand
        // lookups. The index carries the only two columns tested here, which
        // keeps those lookups off the table.
        //
        // Named rather than left to the planner. On a library with statistics
        // the planner picks it anyway, but a fresh install has none and
        // chooses the table; naming it makes the two behave the same, and a
        // missing index then fails loudly instead of quietly costing 30%.
        "SELECT items_fts.rowid, bm25(items_fts, {W_TITLE}, {W_CREATORS}, {W_BODY}, {W_TAGS}) AS s
         FROM items_fts
         CROSS JOIN items i INDEXED BY idx_items_live ON i.id = items_fts.rowid
         WHERE items_fts MATCH ?1 AND i.library_id = ?2 AND i.deleted = 0
         ORDER BY s LIMIT ?3"
    )
}

const TRIGRAM_SQL: &str = "SELECT items_trgm.rowid, i.sort_title, i.sort_creator
     FROM items_trgm
     CROSS JOIN items i ON i.id = items_trgm.rowid
     WHERE items_trgm MATCH ?1 AND i.library_id = ?2 AND i.deleted = 0
     LIMIT ?3";

/// An exact phrase, asked of the word index rather than the trigram one.
///
/// Same shape as [`TRIGRAM_SQL`] and a very different cost: see [`fuzzy`].
const PHRASE_SQL: &str = "SELECT items_fts.rowid, i.sort_title, i.sort_creator
     FROM items_fts
     CROSS JOIN items i ON i.id = items_fts.rowid
     WHERE items_fts MATCH ?1 AND i.library_id = ?2 AND i.deleted = 0
     LIMIT ?3";

/// The statements whose query plan must stay FTS-driven.
pub fn critical_statements() -> Vec<(&'static str, String)> {
    vec![
        ("items_fts", bm25_sql()),
        ("items_fts", PHRASE_SQL.to_string()),
        ("items_trgm", TRIGRAM_SQL.to_string()),
    ]
}

/// Escape a token for FTS5 and optionally make it a prefix query.
fn quote(token: &str, prefix: bool) -> String {
    let escaped = token.replace('"', "");
    if prefix {
        format!("\"{escaped}\"*")
    } else {
        format!("\"{escaped}\"")
    }
}

/// Build an FTS5 MATCH expression. The final token becomes a prefix query so
/// as-you-type search feels instant.
pub fn match_expression(input: &str, conjunctive: bool) -> Option<String> {
    let tokens = text::tokenize_query(input);
    if tokens.is_empty() {
        return None;
    }
    let last = tokens.len() - 1;
    let parts: Vec<String> =
        tokens.iter().enumerate().map(|(i, t)| quote(t, i == last)).collect();
    Some(parts.join(if conjunctive { " AND " } else { " OR " }))
}

/// BM25-ranked candidates. Returns `(item_id, score)` best first.
pub fn keyword(
    conn: &Connection,
    library_id: i64,
    query: &str,
    limit: usize,
) -> Result<Vec<(i64, f32)>> {
    let mut tried: Option<String> = None;
    for conjunctive in [true, false] {
        let Some(expr) = match_expression(query, conjunctive) else { return Ok(Vec::new()) };
        // A one-token query joins to the same string either way, so the
        // "fallback" would re-run the identical statement — twice the work to
        // reach the same empty answer, on exactly the queries that already
        // found nothing.
        if tried.as_deref() == Some(expr.as_str()) {
            break;
        }
        let hits = run_bm25(conn, library_id, &expr, limit)?;
        // Fall back to a disjunctive query only when the strict one found
        // nothing, which keeps precision high for multi-word queries.
        if !hits.is_empty() {
            return Ok(hits);
        }
        tried = Some(expr);
    }
    Ok(Vec::new())
}

fn run_bm25(
    conn: &Connection,
    library_id: i64,
    expr: &str,
    limit: usize,
) -> Result<Vec<(i64, f32)>> {
    let mut stmt = conn.prepare_cached(&bm25_sql()).map_err(sql_err)?;
    let rows = stmt
        .query_map(params![expr, library_id, limit as i64], |r| {
            // bm25() is negative, smaller is better; flip it so bigger is better.
            Ok((r.get::<_, i64>(0)?, -r.get::<_, f64>(1)? as f32))
        })
        .map_err(sql_err)?
        .collect::<rusqlite::Result<Vec<_>>>();

    match rows {
        Ok(v) => Ok(v),
        // A malformed MATCH expression is a user-input problem, not a failure.
        Err(rusqlite::Error::SqliteFailure(_, Some(msg))) if msg.contains("fts5") => Ok(Vec::new()),
        Err(e) => Err(sql_err(e)),
    }
}

/// Candidate row for fuzzy re-ranking.
pub struct FuzzyCandidate {
    pub id: i64,
    pub title: String,
    pub creator: String,
}

/// Split a query into overlapping substrings long enough to be selective.
///
/// This is the heart of fast typo tolerance. ORing every trigram of the query
/// is hopeless on a real corpus — trigrams like `the` match nearly every
/// document, so FTS5 merges enormous postings lists. Chunks of 4+ characters
/// are rare enough to stay cheap, and because they overlap, a single typo can
/// only damage the chunks it touches: the others still find the document.
fn chunks(norm: &str) -> Vec<String> {
    let chars: Vec<char> = norm.chars().filter(|c| !c.is_whitespace()).collect();
    let n = chars.len();
    if n < 3 {
        return Vec::new();
    }
    // Long enough to be selective, short enough that a typo cannot break them all.
    let size = n.div_ceil(3).clamp(4, 8).min(n);
    let step = (size / 2).max(1);

    let mut out = Vec::new();
    let mut start = 0;
    while start + size <= n && out.len() < MAX_CHUNKS {
        out.push(chars[start..start + size].iter().collect::<String>());
        start += step;
    }
    if out.is_empty() {
        out.push(chars.iter().collect());
    }
    out
}

/// Typo-tolerant candidate generation.
///
/// Stage 1 asks the trigram index for an exact substring match: fast, precise,
/// and enough for most single-word queries -- but only those, for the reason
/// given below. Stage 2 uses overlapping chunks (see [`chunks`]) rather than
/// raw trigrams. Actual scoring is left to edit distance in the caller.
pub fn fuzzy(
    conn: &Connection,
    library_id: i64,
    query: &str,
    limit: usize,
) -> Result<Vec<FuzzyCandidate>> {
    let norm = text::normalize(query);
    if norm.is_empty() {
        return Ok(Vec::new());
    }

    if norm.chars().count() < 3 {
        return prefix_scan(conn, library_id, &norm, limit);
    }

    // Stage 1 asks for the query as an exact phrase, and *which index* it asks
    // makes the difference between 0.2ms and 36ms.
    //
    // Over the trigram index a phrase is a positional intersection of every
    // trigram it contains. For one word -- especially a misspelt one -- those
    // are rare and the intersection is immediate. For a phrase spanning a
    // space they are all common, and "diffusion model" took 36ms of the 38ms
    // a whole fuzzy search cost, to return two rows.
    //
    // The word index answers the same question in 0.17ms with the same two
    // rows. A trailing prefix keeps what the trigram index gave for free:
    // "diffusion model" still reaches a paper about diffusion *models*.
    //
    // The probe cannot simply be dropped for several words -- the chunks below
    // truncate in rowid order, so the exact match is not reliably among them.
    // A search for a paper by its exact title was losing that paper.
    let mut out = if norm.split_whitespace().nth(1).is_none() {
        trigram_match(conn, library_id, &quote(&norm, false), limit)?
    } else {
        phrase_match(conn, library_id, &quote(&norm, true), limit)?
    };
    if out.len() >= limit.min(8) {
        return Ok(out);
    }

    let mut seen: std::collections::HashSet<i64> = out.iter().map(|c| c.id).collect();
    // Query chunks one at a time and stop early: a separate cheap query per
    // chunk beats one giant OR, and most queries are satisfied by the first.
    for chunk in chunks(&norm) {
        if out.len() >= limit {
            break;
        }
        let found = trigram_match(conn, library_id, &quote(&chunk, false), limit * 2)?;
        for c in found {
            if seen.insert(c.id) {
                out.push(c);
            }
        }
    }
    Ok(out)
}

fn trigram_match(
    conn: &Connection,
    library_id: i64,
    expr: &str,
    limit: usize,
) -> Result<Vec<FuzzyCandidate>> {
    candidates(conn, TRIGRAM_SQL, library_id, expr, limit)
}

fn phrase_match(
    conn: &Connection,
    library_id: i64,
    expr: &str,
    limit: usize,
) -> Result<Vec<FuzzyCandidate>> {
    candidates(conn, PHRASE_SQL, library_id, expr, limit)
}

/// Candidates from either index. They differ only in which one they read, and
/// a malformed FTS expression means no candidates rather than a failed search.
fn candidates(
    conn: &Connection,
    sql: &str,
    library_id: i64,
    expr: &str,
    limit: usize,
) -> Result<Vec<FuzzyCandidate>> {
    let mut stmt = conn.prepare_cached(sql).map_err(sql_err)?;
    let rows = stmt
        .query_map(params![expr, library_id, limit as i64], |r| {
            Ok(FuzzyCandidate { id: r.get(0)?, title: r.get(1)?, creator: r.get(2)? })
        })
        .map_err(sql_err)?
        .collect::<rusqlite::Result<Vec<_>>>();
    match rows {
        Ok(v) => Ok(v),
        Err(rusqlite::Error::SqliteFailure(_, Some(msg))) if msg.contains("fts5") => Ok(Vec::new()),
        Err(e) => Err(sql_err(e)),
    }
}

/// One- and two-character queries cannot use the trigram index.
fn prefix_scan(
    conn: &Connection,
    library_id: i64,
    prefix: &str,
    limit: usize,
) -> Result<Vec<FuzzyCandidate>> {
    let sql = "SELECT id, sort_title, sort_creator FROM items
               WHERE library_id = ?1 AND deleted = 0
                 AND (sort_title LIKE ?2 OR sort_creator LIKE ?2)
               ORDER BY sort_title LIMIT ?3";
    let like = format!("{prefix}%");
    let mut stmt = conn.prepare_cached(sql).map_err(sql_err)?;
    let out = stmt
        .query_map(params![library_id, like, limit as i64], |r| {
            Ok(FuzzyCandidate { id: r.get(0)?, title: r.get(1)?, creator: r.get(2)? })
        })
        .map_err(sql_err)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(sql_err);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_conjunctive_expression_with_prefix_tail() {
        let e = match_expression("neural network", true).unwrap();
        assert_eq!(e, "\"neural\" AND \"network\"*");
    }

    #[test]
    fn builds_disjunctive_expression() {
        let e = match_expression("neural network", false).unwrap();
        assert!(e.contains(" OR "));
    }

    #[test]
    fn empty_query_has_no_expression() {
        assert!(match_expression("   ", true).is_none());
        assert!(match_expression("", true).is_none());
    }

    #[test]
    fn strips_quotes_to_avoid_syntax_errors() {
        let e = match_expression("say \"hello\"", true).unwrap();
        assert!(!e.contains("\"\""));
    }

    #[test]
    fn chunks_are_selective_and_overlapping() {
        let c = chunks("transfromer");
        assert!(!c.is_empty());
        assert!(c.len() <= MAX_CHUNKS);
        assert!(c.iter().all(|s| s.chars().count() >= 4), "{c:?}");
        // Overlap means a typo in one chunk leaves another intact.
        assert!(c.len() >= 2, "{c:?}");
    }

    #[test]
    fn chunks_handle_short_and_cjk_input() {
        assert!(chunks("ab").is_empty(), "too short for the trigram index");
        let cjk = chunks("扩散模型分子生成");
        assert!(!cjk.is_empty());
        assert!(cjk.iter().all(|s| s.chars().count() >= 4), "{cjk:?}");
    }

    #[test]
    fn chunks_never_exceed_the_input() {
        let c = chunks("abcd");
        assert_eq!(c, vec!["abcd".to_string()]);
    }

    #[test]
    fn cjk_becomes_multiple_terms() {
        let e = match_expression("扩散模型", true).unwrap();
        assert!(e.contains(" AND "));
    }
}

#[cfg(test)]
mod single_token_tests {
    use super::*;

    #[test]
    fn one_token_reads_the_same_either_way() {
        // Which is why `keyword` stops after the first pass: the disjunctive
        // "fallback" is the same statement, and it only ever runs when the
        // first one found nothing — so the cost is paid exactly on the misses.
        assert_eq!(
            match_expression("transformer", true),
            match_expression("transformer", false),
        );
    }

    #[test]
    fn more_than_one_token_really_does_differ() {
        let and = match_expression("diffusion alignment", true).unwrap();
        let or = match_expression("diffusion alignment", false).unwrap();
        assert_ne!(and, or);
        assert!(and.contains(" AND "), "{and}");
        assert!(or.contains(" OR "), "{or}");
        // Only the last token is a prefix, so as-you-type stays cheap.
        assert!(and.contains("\"alignment\"*"), "{and}");
        assert!(!and.contains("\"diffusion\"*"), "{and}");
    }
}
