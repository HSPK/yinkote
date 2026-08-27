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
//! The fourth, co-citation, was written with the hint already in place —
//! which is what the list is for.
//!
//! Three separate discoveries, months of latency apart, each found by
//! accident while measuring something else. So the rule is a list rather than
//! a habit: a query that joins `items` by fingerprint goes in here, and this
//! test fails if the hint is ever tidied away.

use rusqlite::Connection;
use yk_store::Store;

/// Give the planner a library with statistics, because the real one has them.
///
/// `PRAGMA optimize` runs on a live database and leaves `sqlite_stat1` and
/// `sqlite_stat4` behind, so in production SQLite chooses with numbers in
/// front of it. An empty in-memory database has none, and falls back to
/// heuristics — which is a different planner making a different choice.
///
/// This was not a theory: the duplicate scan's assertion passed with the index
/// hint *and* without it, because on an empty table SQLite reached for the
/// fingerprint index of its own accord while a real library drove the query
/// from `parent_id` and sorted a hundred thousand rows. A plan test on an
/// empty database is an alarm wired to a doorbell nobody presses.
fn seeded() -> Store {
    let store = Store::in_memory().unwrap();
    {
        let conn = store.db().conn().unwrap();
        conn.execute_batch("BEGIN").unwrap();
        {
            let mut insert = conn
                .prepare(
                    "INSERT INTO items(library_id, key, item_type, parent_id, fingerprint, \
                                       year, date_added, date_modified) \
                     VALUES (1, ?1, ?2, ?3, ?4, ?5, 0, 0)",
                )
                .unwrap();
            for i in 0..4000 {
                // Mostly papers, with duplicates among them, plus the
                // attachments that hang off them: the shape decides the plan.
                let top = i % 4 != 0;
                insert
                    .execute(rusqlite::params![
                        format!("K{i:07}"),
                        if top { "journalArticle" } else { "attachment" },
                        if top { None } else { Some(1_i64) },
                        if top { format!("t:paper {}|a:x|y:2020", i / 2) } else { String::new() },
                        2020,
                    ])
                    .unwrap();
            }
        }
        conn.execute_batch("COMMIT; ANALYZE;").unwrap();
    }
    store
}

/// `(name, sql, parameter count)` for each statement that must seek by
/// fingerprint.
fn fingerprint_statements() -> Vec<(&'static str, String, usize)> {
    vec![
        ("missing works", yk_store::plans::MISSING_SQL.to_string(), 2),
        ("bibliography", yk_store::plans::CITES_SQL.to_string(), 3),
        ("duplicate check", yk_store::plans::fingerprint_sql("?,?"), 3),
        ("co-citation", yk_store::plans::COCITATION_SQL.to_string(), 5),
        // Not a lookup by fingerprint but a scan *in* fingerprint order, which
        // wants the same index for the same reason: left alone, SQLite drives
        // it from `parent_id IS NULL` and sorts the library into groups.
        ("duplicate scan", yk_store::plans::DUPLICATE_SCAN_SQL.to_string(), 2),
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
    let store = seeded();
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

/// The scan has one more thing to prove than the lookups do: reading the
/// fingerprint index in order means the grouping is already done, where driving
/// from `parent_id IS NULL` sorts the whole library into groups instead — same
/// answer, two temporary b-trees, and a cost that grows with the library rather
/// than with the number of duplicates. Measured on the 130k benchmark library:
/// 62ms without the hint, 52ms with it.
///
/// Honest about its own reach: at any size this test can seed, SQLite reaches
/// for the fingerprint index unprompted, so removing the hint does *not* make
/// this fail. It guards the statement's shape — a rewrite that groups by
/// something unindexed, or that loses the index — and the production plan is
/// checked by `scripts/bench.mjs` against a real library. Saying so beats
/// leaving a reader to assume an alarm is armed when it is not.
#[test]
fn the_duplicate_scan_groups_by_reading_an_index_in_order() {
    let store = seeded();
    let conn = store.db().conn().unwrap();
    let plan = plan(&conn, yk_store::plans::DUPLICATE_SCAN_SQL, 2);
    assert!(
        !plan.contains("TEMP B-TREE FOR GROUP BY"),
        "the duplicate scan is sorting the library to group it:\n  {plan}"
    );
}
