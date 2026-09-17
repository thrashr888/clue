//! Bounded local reads. Never open with immutable=1: live WAL contents matter.
use crate::{Candidate, clip, search};
use anyhow::{Context, Result, ensure};
use rusqlite::{
    Connection, OpenFlags,
    hooks::{AuthAction, AuthContext, Authorization},
    limits::Limit,
    types::ValueRef,
};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub struct Database {
    connection: Connection,
    path: PathBuf,
}

#[derive(Debug)]
pub struct Rows {
    pub values: Vec<Value>,
    pub truncated: bool,
}

fn identifier(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

impl Database {
    pub fn open(path: &Path, timeout: Duration) -> Result<Self> {
        let path = path
            .canonicalize()
            .context("database must be an existing local file")?;
        ensure!(path.is_file(), "database must be a regular file");
        let connection = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .context("cannot open SQLite database read-only")?;
        connection.busy_timeout(timeout.min(Duration::from_secs(1)))?;
        connection.execute_batch(
            "PRAGMA query_only=ON; PRAGMA trusted_schema=OFF; PRAGMA temp_store=MEMORY;",
        )?;
        connection.set_limit(Limit::SQLITE_LIMIT_LENGTH, 1_048_576)?;
        connection.set_limit(Limit::SQLITE_LIMIT_SQL_LENGTH, 65_536)?;
        connection.set_limit(Limit::SQLITE_LIMIT_ATTACHED, 0)?;
        let start = Instant::now();
        connection.progress_handler(1000, Some(move || start.elapsed() >= timeout))?;
        connection.authorizer(Some(|ctx: AuthContext<'_>| match ctx.action {
            AuthAction::Select | AuthAction::Read { .. } | AuthAction::Recursive => {
                Authorization::Allow
            }
            AuthAction::Pragma {
                pragma_name: "table_info" | "table_xinfo",
                ..
            } => Authorization::Allow,
            AuthAction::Function { function_name }
                if matches!(
                    function_name.to_ascii_lowercase().as_str(),
                    "abs"
                        | "avg"
                        | "count"
                        | "sum"
                        | "total"
                        | "min"
                        | "max"
                        | "round"
                        | "coalesce"
                        | "ifnull"
                        | "nullif"
                        | "iif"
                        | "if"
                        | "typeof"
                        | "length"
                        | "lower"
                        | "upper"
                        | "trim"
                        | "ltrim"
                        | "rtrim"
                        | "substr"
                        | "substring"
                        | "instr"
                        | "replace"
                        | "concat"
                        | "concat_ws"
                        | "like"
                        | "glob"
                        | "date"
                        | "time"
                        | "datetime"
                        | "julianday"
                        | "unixepoch"
                        | "strftime"
                        | "group_concat"
                        | "string_agg"
                        | "hex"
                        | "quote"
                        | "printf"
                        | "format"
                        | "json"
                        | "json_extract"
                        | "json_type"
                        | "json_valid"
                        | "json_array_length"
                        | "json_object"
                        | "json_array"
                        | "json_group_array"
                        | "json_group_object"
                        | "row_number"
                        | "rank"
                        | "dense_rank"
                        | "lag"
                        | "lead"
                        | "first_value"
                        | "last_value"
                ) =>
            {
                Authorization::Allow
            }
            _ => Authorization::Deny,
        }))?;
        Ok(Self { connection, path })
    }

    pub fn path(&self) -> String {
        self.path.display().to_string()
    }

    pub fn query(&self, sql: &str, limit: usize) -> Result<Rows> {
        ensure!((1..=5000).contains(&limit), "row limit must be 1–5000");
        ensure!(
            sql.len() <= 60_000 && !sql.trim().is_empty(),
            "SQL must contain 1–60000 bytes"
        );
        // The subquery enforces one read expression and a cap without parsing or executing a tail.
        let sql = sql.trim().trim_end_matches(';');
        let wrapped = format!("SELECT * FROM (\n{sql}\n) LIMIT {}", limit + 1);
        let mut statement = self
            .connection
            .prepare(&wrapped)
            .context("SQL rejected: use a single SELECT/CTE with allowed read-only functions")?;
        ensure!(statement.readonly(), "SQL must be read-only");
        let columns = statement
            .column_names()
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>();
        ensure!(columns.len() <= 64, "select at most 64 columns");
        let mut unique = HashSet::new();
        ensure!(
            columns.iter().all(|c| unique.insert(c)),
            "use distinct column aliases"
        );
        let mut cursor = statement
            .query([])
            .context("SQL requires concrete values, not unbound parameters")?;
        let mut values = Vec::new();
        let mut bytes = 0;
        let mut truncated = false;
        while let Some(row) = cursor
            .next()
            .context("SQLite read failed or exceeded its time/size budget")?
        {
            if values.len() == limit {
                truncated = true;
                break;
            }
            let mut object = serde_json::Map::new();
            for (i, name) in columns.iter().enumerate() {
                let value = match row.get_ref(i)? {
                    ValueRef::Null => Value::Null,
                    ValueRef::Integer(v) => json!(v),
                    ValueRef::Real(v) => {
                        ensure!(
                            v.is_finite(),
                            "non-finite SQL number; cast it to text explicitly"
                        );
                        json!(v)
                    }
                    ValueRef::Text(v) => json!(
                        std::str::from_utf8(v)
                            .context("non-UTF-8 text; select hex(column) explicitly")?
                    ),
                    ValueRef::Blob(v) => json!({"type":"blob","bytes":v.len(),"omitted":true}),
                };
                object.insert(name.clone(), value);
            }
            let value = Value::Object(object);
            bytes += serde_json::to_vec(&value)?.len();
            ensure!(
                bytes <= 4_194_304,
                "query output exceeds 4 MiB; select fewer/smaller fields"
            );
            values.push(value);
        }
        Ok(Rows { values, truncated })
    }

    pub fn schema(&self) -> Result<Value> {
        let tables = self.query("SELECT name, type FROM sqlite_schema WHERE type IN ('table','view') AND name NOT LIKE 'sqlite_%' ORDER BY name", 200)?;
        let mut output = Vec::new();
        let mut bytes = 0;
        for table in &tables.values {
            let name = table["name"].as_str().context("invalid SQLite schema")?;
            let mut stmt = self.connection.prepare(
                "SELECT name, type, \"notnull\", pk, hidden FROM pragma_table_xinfo(?1)",
            )?;
            let columns = stmt.query_map([name], |row| Ok(json!({"name":row.get::<_,String>(0)?,"type":row.get::<_,String>(1)?,"not_null":row.get::<_,i64>(2)? != 0,"primary_key_position":row.get::<_,i64>(3)?,"hidden":row.get::<_,i64>(4)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;
            let item = json!({"name":name,"type":table["type"],"columns":columns});
            bytes += serde_json::to_vec(&item)?.len();
            ensure!(bytes <= 4_194_304, "schema output exceeds 4 MiB");
            output.push(item);
        }
        Ok(
            json!({"schema_version":1,"ok":true,"database":self.path(),"read_only":true,"tables":output,"truncated":tables.truncated}),
        )
    }

    pub fn search(&self, query: &str, options: &SearchOptions) -> Result<Value> {
        crate::validate_query(query)?;
        ensure!(
            !search::terms(query).is_empty(),
            "query contains no searchable terms"
        );
        ensure!((1..=50).contains(&options.limit), "limit must be 1–50");
        ensure!(
            !options.columns.is_empty() && options.columns.len() <= 16,
            "select 1–16 text columns"
        );
        let mut fields = vec![options.id_column.clone(), options.title_column.clone()];
        fields.extend(options.columns.clone());
        let mut seen = HashSet::new();
        fields.retain(|c| seen.insert(c.clone()));
        // Resolve names before quoting: SQLite's legacy double-quoted string fallback must not hide typos.
        let exists: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name=?1 AND type IN ('table','view'))",
            [&options.table],
            |r| r.get(0),
        )?;
        ensure!(exists, "unknown table/view; run sqlite schema first");
        let mut stmt = self
            .connection
            .prepare("SELECT name FROM pragma_table_xinfo(?1)")?;
        let known = stmt
            .query_map([&options.table], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<HashSet<_>>>()?;
        ensure!(
            fields.iter().all(|f| known.contains(f)),
            "unknown column; run sqlite schema first"
        );
        let sql = format!(
            "SELECT {} FROM {} ORDER BY {}",
            fields
                .iter()
                .map(|s| identifier(s))
                .collect::<Vec<_>>()
                .join(","),
            identifier(&options.table),
            identifier(&options.id_column)
        );
        let rows = self.query(&sql, options.scan_limit)?;
        let mut items = self.candidates(
            &rows.values,
            &options.id_column,
            &options.title_column,
            &options.table,
        )?;
        for item in &mut items {
            item.lexical_score = search::lexical(query, &item.title, &item.text);
            item.text = search::excerpt(&item.text, query);
        }
        items.retain(|c| c.lexical_score > 0.0);
        items.sort_by(|a, b| {
            b.lexical_score
                .total_cmp(&a.lexical_score)
                .then_with(|| a.id.cmp(&b.id))
        });
        let matched = items.len();
        items.truncate(options.limit);
        Ok(
            json!({"schema_version":1,"ok":true,"database":self.path(),"read_only":true,"query":query,"mode":"local_lexical","partial":rows.truncated,"coverage":{"table":options.table,"columns":fields,"scanned":rows.values.len(),"scan_limit":options.scan_limit,"truncated":rows.truncated,"matched":matched,"order":"selected ID column; bounded scan, not a full index"},"results":items}),
        )
    }

    pub fn candidates(
        &self,
        rows: &[Value],
        id_column: &str,
        title_column: &str,
        scope: &str,
    ) -> Result<Vec<Candidate>> {
        let mut seen = HashSet::new();
        rows.iter().map(|row| {
            let id = &row[id_column];
            ensure!(id.is_string() || id.is_i64(), "candidate ID must be non-null text or integer");
            let id = serde_json::to_string(&json!([self.path(),scope,id]))?;
            ensure!(id.len() <= 8192 && seen.insert(id.clone()), "candidate IDs must be unique and bounded; choose a primary key or alias one in SQL");
            let title = row[title_column].as_str().context("candidate title column must contain text")?;
            Ok(Candidate { id:format!("sqlite:{id}"), title:clip(title,500), text:serde_json::to_string(row)?, source:"sqlite".into(), location:Some(self.path()), modified:None, event:None, lexical_score:0.0, relevance:None })
        }).collect()
    }
}

pub struct SearchOptions {
    pub table: String,
    pub id_column: String,
    pub title_column: String,
    pub columns: Vec<String>,
    pub scan_limit: usize,
    pub limit: usize,
}
