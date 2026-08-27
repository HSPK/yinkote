//! Every query that resolves a fingerprint, checked as a group.
//!
//! This exact fault has now appeared three times on the same table and the
//! same column: given a choice, SQLite resolves `items.fingerprint` through
//! `idx_items_year` — an index whose leading columns match the *whole
//! library* — and scans it. The results are identical every time, so nothing
//! that checks answers can see it. Only the plan can.
//!
//!   * `MISSING_SQL`            8.5 s  → 0.2 ms
//!   * `CITES_SQL`              1585 ms → 0.14 ms
//!   * the duplicate check      66 ms  → 0.15 ms
//!
//! Three separate discoveries, months of latency apart, each found by
//! accident while measuring something else. So the rule is a list rather than
//! a habit: a query that joins `items` by fingerprint goes in here, and this
//! test fails if the hint is ever tidied away.

use rusqlite::Connection;
use yk_store::Store;

/// `(name, sql, parameter count)` for each statement that must seek by
/// fingerprint.
fn fingerprint_statements() -> Vec<(&'static str, String, usize)> {
    vec![
        ("missing works", yk_store::plans::MISSING_SQL.to_string(), 2),
        ("bibliography", yk_store::plans::CITES_SQL.to_string(), 3),
        ("duplicate check", yk_store::plans::fingerprint_sql("?,?"), 3),
    ]
}

fn plan(conn: &Connection, sql: &str, params: usize) -> String {
    let mut stmt = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
    // Values are irrelevant to the plan; their presence is not. Text binds
    // for everything, since SQLite does not type-check a plan.
    let bound: Vec<String> = (0..params).map(|_| "1".to_string()).collect();
    stmt.query_map(rusqlite::params_from_iter(bound), |r| r.get::<_, String>(3))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect::<Vec<_>>()
        .join(" | ")
}

#[test]
fn every_fingerprint_lookup_seeks_rather_than_scans() {
    let store = Store::in_memory().unwrap();
    let conn = store.db().conn().unwrap();

    for (name, sql, params) in fingerprint_statements() {
        let plan = plan(&conn, &sql, params);
        assert!(
            plan.contains("idx_items_fingerprint"),
            "the {name} query stopped using the fingerprint index:\n  {plan}"
        );
        assert!(
            !plan.contains("idx_items_year"),
            "the {name} query fell back to the year index — this is the 8.5-second bug:\n  {plan}"
        );
    }
}
