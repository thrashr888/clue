use cider_ai::sqlite::{Database, SearchOptions};
use rusqlite::Connection;
use std::time::Duration;

fn fixture() -> (tempfile::TempDir, std::path::PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("fixture.db");
    let writer = Connection::open(&path).unwrap();
    writer
        .execute_batch(include_str!("../examples/projects.sql"))
        .unwrap();
    (temp, path)
}
fn options() -> SearchOptions {
    SearchOptions {
        table: "issues".into(),
        id_column: "id".into(),
        title_column: "title".into(),
        columns: vec!["description".into(), "project".into()],
        scan_limit: 1000,
        limit: 10,
    }
}
#[test]
fn schema_search_and_join_preserve_ids_and_values() {
    let (_temp, path) = fixture();
    let db = Database::open(&path, Duration::from_secs(2)).unwrap();
    assert_eq!(db.schema().unwrap()["tables"].as_array().unwrap().len(), 2);
    let result = db.search("offline notebook sharing", &options()).unwrap();
    assert_eq!(
        result["results"][0]["title"],
        "Share notebooks across devices"
    );
    assert_eq!(result["partial"], false);
    let rows = db.query("SELECT i.id, i.title, p.language, i.description AS text FROM issues i JOIN projects p ON p.name=i.project WHERE i.status='open' ORDER BY i.id",50).unwrap();
    assert_eq!(rows.values.len(), 4);
    let items = db.candidates(&rows.values, "id", "title", "join").unwrap();
    assert_eq!(items.len(), 4);
    assert!(items[0].id.contains("join"));
    let values = db
        .query(
            "SELECT NULL AS empty, 9223372036854775807 AS large, x'0102' AS data, '東京' AS city",
            1,
        )
        .unwrap();
    assert!(values.values[0]["empty"].is_null());
    assert_eq!(values.values[0]["large"].as_i64(), Some(i64::MAX));
    assert_eq!(values.values[0]["data"]["omitted"], true);
    assert_eq!(values.values[0]["city"], "東京");
}
#[test]
fn readonly_rejects_writes_attachments_and_side_effect_functions() {
    let (temp, path) = fixture();
    let before = std::fs::read(&path).unwrap();
    let db = Database::open(&path, Duration::from_secs(2)).unwrap();
    for sql in [
        "DELETE FROM issues",
        "UPDATE issues SET title='changed' RETURNING *",
        "PRAGMA query_only=OFF",
        "ATTACH DATABASE '/tmp/never-cider.db' AS other",
        "SELECT load_extension('anything')",
        "SELECT writefile('anything','bad')",
        "SELECT * FROM issues; DELETE FROM issues",
        "SELECT 1) ; DELETE FROM issues; SELECT (1",
    ] {
        assert!(db.query(sql, 10).is_err(), "accepted {sql}");
    }
    assert_eq!(std::fs::read(&path).unwrap(), before);
    let missing = temp.path().join("missing.db");
    assert!(Database::open(&missing, Duration::from_secs(1)).is_err());
    assert!(!missing.exists());
}
#[test]
fn scan_caps_bad_columns_and_duplicate_ids_are_explicit() {
    let (_temp, path) = fixture();
    let db = Database::open(&path, Duration::from_secs(2)).unwrap();
    let mut opts = options();
    opts.scan_limit = 1;
    let result = db.search("offline", &opts).unwrap();
    assert_eq!(result["partial"], true);
    assert_eq!(result["coverage"]["scanned"], 1);
    opts.columns = vec!["typo".into()];
    assert!(db.search("sync", &opts).is_err());
    opts = options();
    opts.table = "issues\"; DROP TABLE projects; --".into();
    assert!(db.search("sync", &opts).is_err());
    let rows = db.query("SELECT 1 AS id, title FROM issues", 10).unwrap();
    assert!(db.candidates(&rows.values, "id", "title", "query").is_err());
}
#[test]
fn reads_live_wal_and_interrupts_expensive_queries() {
    let (_temp, path) = fixture();
    let writer = Connection::open(&path).unwrap();
    writer.execute_batch("PRAGMA journal_mode=WAL; INSERT INTO issues VALUES(99,'Cider','Live WAL event','calendar sync','open');").unwrap();
    let db = Database::open(&path, Duration::from_secs(2)).unwrap();
    assert_eq!(
        db.query("SELECT title FROM issues WHERE id=99", 1)
            .unwrap()
            .values[0]["title"],
        "Live WAL event"
    );
    let bounded = Database::open(&path, Duration::from_millis(1)).unwrap();
    assert!(bounded.query("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<1000000000) SELECT sum(x) FROM n",1).is_err());
}
#[test]
fn quoted_identifiers_and_unicode_search_work() {
    let (_temp, path) = fixture();
    let writer = Connection::open(&path).unwrap();
    writer.execute_batch("CREATE TABLE \"odd table\"(\"record id\" TEXT PRIMARY KEY,\"display title\" TEXT,\"body text\" TEXT); INSERT INTO \"odd table\" VALUES('a','東京のノート','共有テスト');").unwrap();
    let db = Database::open(&path, Duration::from_secs(2)).unwrap();
    let opts = SearchOptions {
        table: "odd table".into(),
        id_column: "record id".into(),
        title_column: "display title".into(),
        columns: vec!["body text".into()],
        ..options()
    };
    assert_eq!(
        db.search("東京", &opts).unwrap()["results"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn output_bounds_fail_explicitly_and_empty_results_are_successful() {
    let (_temp, path) = fixture();
    let writer = Connection::open(&path).unwrap();
    writer.execute_batch("CREATE TABLE large_rows(value TEXT); INSERT INTO large_rows VALUES(printf('%0900000d',0)); INSERT INTO large_rows SELECT value FROM large_rows; INSERT INTO large_rows SELECT value FROM large_rows; INSERT INTO large_rows SELECT value FROM large_rows;").unwrap();
    let db = Database::open(&path, Duration::from_secs(2)).unwrap();
    assert!(
        db.query("SELECT value FROM large_rows", 8)
            .unwrap_err()
            .to_string()
            .contains("4 MiB")
    );
    assert!(
        db.query("SELECT printf('%02000000d',0) AS oversized", 1)
            .is_err()
    );
    assert!(
        db.query("SELECT id FROM issues WHERE 0", 1)
            .unwrap()
            .values
            .is_empty()
    );
    assert!(db.query("SELECT id FROM issues", 1).unwrap().truncated);
}
