//! Guards the query plan of the two search statements.
//!
//! Regression test for a 3000x slowdown: given a plain `JOIN`, SQLite drove the
//! search from `items`, scanning all 100k rows and probing the FTS index once
//! per row. Everything still returned correct results — only the latency
//! exploded — so only a plan assertion can catch it.

use rusqlite::{params, Connection};
use yk_search::lexical::critical_statements;
use yk_store::Store;

fn plan(conn: &Connection, sql: &str) -> Vec<String> {
    let mut stmt = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
    // The statements are parameterised; bind placeholders so EXPLAIN can run.
    let rows = stmt
        .query_map(params!["x", 1i64, 10i64], |r| r.get::<_, String>(3))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    rows
}

#[test]
fn search_statements_are_driven_by_the_fts_index() {
    let store = Store::in_memory().unwrap();
    let conn = store.db().conn().unwrap();

    for (virtual_table, sql) in critical_statements() {
        let steps = plan(&conn, &sql);
        let outer = steps.first().unwrap_or_else(|| panic!("no plan for {virtual_table}"));

        assert!(
            outer.starts_with(&format!("SCAN {virtual_table}")),
            "the outer loop of the {virtual_table} query must be the FTS table, got:\n  {}",
            steps.join("\n  ")
        );
        assert!(
            steps.iter().any(|s| s.starts_with("SEARCH i ")),
            "`items` must be reached by primary-key lookup, not scanned:\n  {}",
            steps.join("\n  ")
        );
        assert!(
            !steps.iter().any(|s| s.starts_with("SCAN i ") || s == "SCAN i"),
            "no step may scan `items`; that is the pathological plan:\n  {}",
            steps.join("\n  ")
        );
    }
}
