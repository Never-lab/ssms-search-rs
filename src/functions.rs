//! Multi-function SQL operations (SSMS-inspired).

use crate::catalog::{Catalog, CatalogObject, ObjectKind};
use crate::connection::{
    collect_query_batch, execute_ignore, query_table, sql_nvarchar, SqlClient,
};
use crate::plan::{parse_showplan_operators, PlanOperator};
use anyhow::{bail, Result};
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct DbInfo {
    pub name: String,
    pub database_id: i32,
    pub state_desc: String,
}

#[derive(Debug, Clone)]
pub struct ColumnInfo {
    pub name: String,
    pub data_type: String,
    pub max_length: i16,
    pub is_nullable: bool,
}

#[derive(Debug, Clone)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanMode {
    /// Execute only — no plan XML.
    None,
    /// SET SHOWPLAN_XML (estimated, does not execute).
    Estimated,
    /// SET STATISTICS XML ON then execute (actual plan).
    Actual,
}

#[derive(Debug, Clone)]
pub struct QueryExecution {
    pub result: QueryResult,
    pub messages: Vec<String>,
    pub plan_xml: Option<String>,
    pub plan_operators: Vec<PlanOperator>,
    pub elapsed_ms: u128,
}

pub async fn list_databases(client: &mut SqlClient) -> Result<Vec<DbInfo>> {
    crate::trace::timed_result("engine.list_databases", "", async {
        let table = query_table(
            client,
            "SELECT name, database_id, state_desc \
             FROM sys.databases \
             WHERE state = 0 \
             ORDER BY name",
            None,
        )
        .await?;
        let mut out = Vec::with_capacity(table.rows.len());
        for row in table.rows {
            out.push(DbInfo {
                name: cell(&row, 0).to_string(),
                database_id: cell(&row, 1).parse().unwrap_or(0),
                state_desc: cell(&row, 2).to_string(),
            });
        }
        Ok(out)
    })
    .await
}

/// Appends catalog objects with sequential ids (caller owns the id space).
struct Builder<'a> {
    objects: &'a mut Vec<CatalogObject>,
    next: u64,
}

impl<'a> Builder<'a> {
    fn new(objects: &'a mut Vec<CatalogObject>) -> Self {
        let next = objects.iter().map(|o| o.id).max().unwrap_or(0) + 1;
        Self { objects, next }
    }

    #[allow(clippy::too_many_arguments)]
    fn add(
        &mut self,
        parent: Option<u64>,
        kind: ObjectKind,
        name: &str,
        schema: &str,
        owner: &str,
        urn: String,
        create_date: &str,
    ) -> u64 {
        let id = self.next;
        self.next += 1;
        self.objects.push(CatalogObject {
            id,
            parent_id: parent,
            kind,
            name: name.to_string(),
            schema: schema.to_string(),
            owner: owner.to_string(),
            urn,
            create_date: create_date.to_string(),
        });
        id
    }
}

fn server_urn(server: &str) -> String {
    format!("Server[@Name='{}']", escape_urn(server))
}

fn database_urn(server: &str, database: &str) -> String {
    format!(
        "{}/Database[@Name='{}']",
        server_urn(server),
        escape_urn(database)
    )
}

/// Load Object Explorer-shaped catalog for one database (tables/views/procs/functions).
pub async fn load_catalog(client: &mut SqlClient, server: &str, database: &str) -> Result<Catalog> {
    crate::trace::timed_result(
        "engine.load_catalog",
        format!("{server}/{database}"),
        async {
            let mut catalog = Catalog {
                objects: Vec::new(),
            };
            let mut b = Builder::new(&mut catalog.objects);
            let server_id = b.add(
                None,
                ObjectKind::Server,
                server,
                "",
                "",
                server_urn(server),
                "",
            );
            let db_id = b.add(
                Some(server_id),
                ObjectKind::Database,
                database,
                "",
                "",
                database_urn(server, database),
                "",
            );
            populate_database(client, &mut b, server, database, db_id).await?;
            Ok(catalog)
        },
    )
    .await
}

/// Server node + every online database; only `prefer_database` is fully populated.
/// Other databases are filled lazily via [`ensure_database_loaded`].
pub async fn load_server_catalog(
    client: &mut SqlClient,
    server: &str,
    prefer_database: &str,
) -> Result<Catalog> {
    crate::trace::timed_result(
        "engine.load_server_catalog",
        format!("{server} prefer={prefer_database}"),
        async {
            let dbs = list_databases(client).await?;
            let (mut catalog, db_id, db_name) = server_skeleton(server, &dbs, prefer_database)?;
            let mut b = Builder::new(&mut catalog.objects);
            populate_database(client, &mut b, server, &db_name, db_id).await?;
            Ok(catalog)
        },
    )
    .await
}

/// Pure part of [`load_server_catalog`]: Server + one Database node per `dbs` entry.
/// Returns the catalog plus the id/name of the (case-insensitively) preferred database.
fn server_skeleton(
    server: &str,
    dbs: &[DbInfo],
    prefer_database: &str,
) -> Result<(Catalog, u64, String)> {
    let mut catalog = Catalog {
        objects: Vec::new(),
    };
    let mut b = Builder::new(&mut catalog.objects);
    let server_id = b.add(
        None,
        ObjectKind::Server,
        server,
        "",
        "",
        server_urn(server),
        "",
    );
    let mut prefer_id = None;
    for db in dbs {
        let id = b.add(
            Some(server_id),
            ObjectKind::Database,
            &db.name,
            "",
            "",
            database_urn(server, &db.name),
            "",
        );
        if db.name.eq_ignore_ascii_case(prefer_database) {
            prefer_id = Some((id, db.name.clone()));
        }
    }
    let Some((db_id, db_name)) = prefer_id else {
        bail!("database not found or not online: {prefer_database}");
    };
    Ok((catalog, db_id, db_name))
}

/// Populate a Database node on first expand. No-op if it already has children.
pub async fn ensure_database_loaded(
    client: &mut SqlClient,
    server: &str,
    database: &str,
    catalog: &mut Catalog,
    db_id: u64,
) -> Result<()> {
    if catalog.objects.iter().any(|o| o.parent_id == Some(db_id)) {
        return Ok(());
    }
    crate::trace::timed_result(
        "engine.ensure_database_loaded",
        format!("{server}/{database} id={db_id}"),
        async {
            let Some(db_obj) = catalog.by_id(db_id) else {
                bail!("object {db_id} not found in catalog");
            };
            if db_obj.kind != ObjectKind::Database {
                bail!("object {db_id} is not a Database node");
            }
            if !db_obj.name.eq_ignore_ascii_case(database) {
                bail!(
                    "database name {database:?} does not match Database node name {:?}",
                    db_obj.name
                );
            }
            // Build off to the side; commit only on full success so errors leave `catalog` untouched.
            let mut new = Vec::new();
            let mut b = Builder {
                objects: &mut new,
                next: catalog.objects.iter().map(|o| o.id).max().unwrap_or(0) + 1,
            };
            populate_database(client, &mut b, server, database, db_id).await?;
            catalog.objects.extend(new);
            Ok(())
        },
    )
    .await
}

/// Tables/views/procs/functions/synonyms/triggers folders + leaves under `db_id`.
async fn populate_database(
    client: &mut SqlClient,
    b: &mut Builder<'_>,
    server: &str,
    database: &str,
    db_id: u64,
) -> Result<()> {
    let _span =
        crate::trace::Span::new("engine.populate_database").detail(format!("{server}/{database}"));
    use_database(client, database).await?;

    let q = "\
SELECT s.name AS schema_name, \
       o.name AS object_name, \
       o.type AS type_code, \
       ISNULL(USER_NAME(o.principal_id), s.name) AS owner_name, \
       CONVERT(varchar(33), o.create_date, 126) AS create_date \
FROM sys.objects AS o \
INNER JOIN sys.schemas AS s ON o.schema_id = s.schema_id \
WHERE o.is_ms_shipped = 0 \
  AND o.type IN (N'U', N'V', N'P', N'FN', N'IF', N'TF') \
ORDER BY \
  CASE o.type WHEN N'U' THEN 1 WHEN N'V' THEN 2 WHEN N'P' THEN 3 ELSE 4 END, \
  s.name, o.name";

    let rows = query_table(client, q, None).await?.rows;

    let db_urn = database_urn(server, database);
    let folder = |b: &mut Builder<'_>, name: &str, urn_kind: &str| {
        b.add(
            Some(db_id),
            ObjectKind::Folder,
            name,
            "",
            "",
            format!("{db_urn}/{urn_kind}"),
            "",
        )
    };
    let tables_id = folder(b, "Tables", "Table");
    let views_id = folder(b, "Views", "View");
    let procs_id = folder(b, "Stored Procedures", "StoredProcedure");
    let funcs_id = folder(b, "Functions", "UserDefinedFunction");

    for row in rows {
        let schema = nonempty(cell(&row, 0), "dbo");
        let name = cell(&row, 1);
        let type_code = cell(&row, 2).trim();
        let owner = nonempty(cell(&row, 3), schema);
        let date = cell(&row, 4);
        let (kind, parent, type_urn) = match type_code {
            "U" => (ObjectKind::Table, tables_id, "Table"),
            "V" => (ObjectKind::View, views_id, "View"),
            "P" => (ObjectKind::Procedure, procs_id, "StoredProcedure"),
            "FN" | "IF" | "TF" => (ObjectKind::Function, funcs_id, "UserDefinedFunction"),
            _ => continue,
        };
        let object_urn = format!(
            "{db_urn}/{type_urn}[@Name='{}' and @Schema='{}']",
            escape_urn(name),
            escape_urn(schema)
        );
        let object_id = b.add(
            Some(parent),
            kind,
            name,
            schema,
            owner,
            object_urn.clone(),
            date,
        );
        if kind == ObjectKind::Table {
            for child in ["Columns", "Indexes"] {
                b.add(
                    Some(object_id),
                    ObjectKind::Folder,
                    child,
                    "",
                    "",
                    table_child_folder_urn(&object_urn, child),
                    "",
                );
            }
        }
    }

    let synonyms_id = folder(b, "Synonyms", "Synonym");
    let syn_rows = query_table(
        client,
        "SELECT s.name AS schema_name, syn.name AS object_name, \
             ISNULL(USER_NAME(syn.principal_id), s.name) AS owner_name, \
             CONVERT(varchar(33), syn.create_date, 126) AS create_date \
             FROM sys.synonyms AS syn \
             INNER JOIN sys.schemas AS s ON syn.schema_id = s.schema_id \
             ORDER BY s.name, syn.name",
        None,
    )
    .await?
    .rows;
    for row in syn_rows {
        let schema = nonempty(cell(&row, 0), "dbo");
        let name = cell(&row, 1);
        let owner = nonempty(cell(&row, 2), schema);
        let date = cell(&row, 3);
        b.add(
            Some(synonyms_id),
            ObjectKind::Synonym,
            name,
            schema,
            owner,
            format!(
                "{db_urn}/Synonym[@Name='{}' and @Schema='{}']",
                escape_urn(name),
                escape_urn(schema)
            ),
            date,
        );
    }

    let triggers_id = folder(b, "Triggers", "Trigger");
    let trig_rows = query_table(
        client,
        "SELECT sch.name AS schema_name, t.name AS object_name, \
             ISNULL(USER_NAME(o.principal_id), sch.name) AS owner_name, \
             CONVERT(varchar(33), t.create_date, 126) AS create_date \
             FROM sys.triggers AS t \
             INNER JOIN sys.objects AS o ON t.object_id = o.object_id \
             INNER JOIN sys.schemas AS sch ON o.schema_id = sch.schema_id \
             WHERE t.parent_class = 1 \
             ORDER BY sch.name, t.name",
        None,
    )
    .await?
    .rows;
    for row in trig_rows {
        let schema = nonempty(cell(&row, 0), "dbo");
        let name = cell(&row, 1);
        let owner = nonempty(cell(&row, 2), schema);
        let date = cell(&row, 3);
        b.add(
            Some(triggers_id),
            ObjectKind::Trigger,
            name,
            schema,
            owner,
            format!(
                "{db_urn}/Trigger[@Name='{}' and @Schema='{}']",
                escape_urn(name),
                escape_urn(schema)
            ),
            date,
        );
    }
    Ok(())
}

pub async fn describe_object(
    client: &mut SqlClient,
    database: &str,
    schema: &str,
    name: &str,
) -> Result<Vec<ColumnInfo>> {
    crate::trace::timed_result(
        "engine.describe_object",
        format!("{database}.{schema}.{name}"),
        async {
            use_database(client, database).await?;
            let sql = format!(
                "SELECT c.name, \
                    t.name AS type_name, \
                    c.max_length, \
                    c.is_nullable \
             FROM sys.columns AS c \
             INNER JOIN sys.types AS t ON c.user_type_id = t.user_type_id \
             INNER JOIN sys.objects AS o ON c.object_id = o.object_id \
             INNER JOIN sys.schemas AS s ON o.schema_id = s.schema_id \
             WHERE s.name = {schema} AND o.name = {name} \
             ORDER BY c.column_id",
                schema = sql_nvarchar(schema),
                name = sql_nvarchar(name),
            );
            let rows = query_table(client, sql, None).await?.rows;
            if rows.is_empty() {
                bail!("object not found or has no columns: {schema}.{name}");
            }
            Ok(rows
                .into_iter()
                .map(|row| ColumnInfo {
                    name: cell(&row, 0).to_string(),
                    data_type: cell(&row, 1).to_string(),
                    max_length: cell(&row, 2).parse().unwrap_or(0),
                    is_nullable: cell(&row, 3) == "1"
                        || cell(&row, 3).eq_ignore_ascii_case("true"),
                })
                .collect())
        },
    )
    .await
}

/// Load Columns and Indexes leaves for one table. Ids start at `start_id` (caller owns id space).
#[allow(clippy::too_many_arguments)]
pub async fn load_table_children(
    client: &mut SqlClient,
    database: &str,
    server: &str,
    schema: &str,
    table_name: &str,
    columns_folder_id: u64,
    indexes_folder_id: u64,
    start_id: u64,
) -> Result<Vec<CatalogObject>> {
    crate::trace::timed_result(
        "engine.load_table_children",
        format!("{database}.{schema}.{table_name}"),
        async {
            use_database(client, database).await?;
            let table_urn = format!(
                "Server[@Name='{}']/Database[@Name='{}']/Table[@Name='{}' and @Schema='{}']",
                escape_urn(server),
                escape_urn(database),
                escape_urn(table_name),
                escape_urn(schema)
            );
            let mut out = Vec::new();
            let mut next = start_id;
            let mut push = |parent: u64, kind: ObjectKind, name: &str, leaf: &str| {
                out.push(CatalogObject {
                    id: next,
                    parent_id: Some(parent),
                    kind,
                    name: name.to_string(),
                    schema: schema.to_string(),
                    owner: String::new(),
                    urn: child_leaf_urn(&table_urn, leaf, name),
                    create_date: String::new(),
                });
                next += 1;
            };

            let cols_sql = format!(
                "SELECT c.name \
             FROM sys.columns AS c \
             INNER JOIN sys.types AS t ON c.user_type_id = t.user_type_id \
             INNER JOIN sys.objects AS o ON c.object_id = o.object_id \
             INNER JOIN sys.schemas AS s ON o.schema_id = s.schema_id \
             WHERE s.name = {schema} AND o.name = {table} \
             ORDER BY c.column_id",
                schema = sql_nvarchar(schema),
                table = sql_nvarchar(table_name),
            );
            for row in query_table(client, cols_sql, None).await?.rows {
                push(columns_folder_id, ObjectKind::Column, cell(&row, 0), "Column");
            }

            let idx_sql = format!(
                "SELECT i.name \
             FROM sys.indexes AS i \
             INNER JOIN sys.tables AS t ON i.object_id = t.object_id \
             INNER JOIN sys.schemas AS s ON t.schema_id = s.schema_id \
             WHERE s.name = {schema} AND t.name = {table} AND i.index_id > 0 AND i.name IS NOT NULL \
             ORDER BY i.name",
                schema = sql_nvarchar(schema),
                table = sql_nvarchar(table_name),
            );
            for row in query_table(client, idx_sql, None).await?.rows {
                push(indexes_folder_id, ObjectKind::Index, cell(&row, 0), "Index");
            }
            Ok(out)
        },
    )
    .await
}

/// Skip leading whitespace, `-- …` line comments and `/* … */` blocks.
fn strip_leading_comments(mut s: &str) -> &str {
    loop {
        s = s.trim_start();
        if let Some(rest) = s.strip_prefix("--") {
            s = rest.find('\n').map_or("", |i| &rest[i + 1..]);
        } else if let Some(rest) = s.strip_prefix("/*") {
            s = rest.find("*/").map_or("", |i| &rest[i + 2..]);
        } else {
            return s;
        }
    }
}

/// Reject mutating/DDL batches used by the query helper / GUI Execute path.
pub fn assert_readonly_sql(sql: &str) -> Result<()> {
    let upper = sql.trim().to_ascii_uppercase();
    for stmt in upper.split(';') {
        let s = strip_leading_comments(stmt);
        for banned in [
            "INSERT", "UPDATE", "DELETE", "MERGE", "DROP", "ALTER", "TRUNCATE", "EXEC", "EXECUTE",
            "CREATE", "GRANT", "REVOKE", "DENY",
        ] {
            // Require a non-identifier char after the keyword (or end) so we don't
            // false-positive on names like "CREATED_AT" in a SELECT list mid-stmt —
            // we only inspect statement starts.
            if s == banned
                || s.starts_with(&format!("{banned} "))
                || s.starts_with(&format!("{banned}\t"))
                || s.starts_with(&format!("{banned}\n"))
                || s.starts_with(&format!("{banned}\r"))
            {
                bail!("refusing mutating/DDL statement: detected '{banned}'");
            }
        }
    }
    Ok(())
}

/// Run a read-oriented batch. Rejects obviously dangerous statements for the CLI helper.
pub async fn run_query(client: &mut SqlClient, sql: &str, max_rows: usize) -> Result<QueryResult> {
    Ok(run_query_ex(client, sql, max_rows, PlanMode::None)
        .await?
        .result)
}

/// Execute SQL with optional estimated/actual execution plan (SSMS-style).
pub async fn run_query_ex(
    client: &mut SqlClient,
    sql: &str,
    max_rows: usize,
    plan_mode: PlanMode,
) -> Result<QueryExecution> {
    let trimmed = sql.trim();
    if trimmed.is_empty() {
        bail!("empty query");
    }
    assert_readonly_sql(trimmed)?;

    let plan_label = match plan_mode {
        PlanMode::None => "none",
        PlanMode::Estimated => "estimated",
        PlanMode::Actual => "actual",
    };
    crate::trace::timed_result(
        "engine.run_query_ex",
        format!(
            "plan={plan_label} max_rows={max_rows} bytes={}",
            trimmed.len()
        ),
        async {
            let mut messages = Vec::new();
            let started = Instant::now();

            {
                let _span = crate::trace::Span::new("engine.run_query_ex.plan_on")
                    .detail(plan_label.to_string());
                match plan_mode {
                    PlanMode::None => {}
                    PlanMode::Estimated => {
                        execute_ignore(client, "SET SHOWPLAN_XML ON;").await?;
                        messages.push(
                            "SET SHOWPLAN_XML ON — estimated plan (query not executed)".into(),
                        );
                    }
                    PlanMode::Actual => {
                        execute_ignore(client, "SET STATISTICS XML ON;").await?;
                        messages.push("SET STATISTICS XML ON — actual execution plan".into());
                    }
                }
            }

            let batch_result = {
                let _span = crate::trace::Span::new("engine.run_query_ex.collect");
                collect_query_batch(client, trimmed, max_rows).await
            };

            {
                let _span = crate::trace::Span::new("engine.run_query_ex.plan_off");
                match plan_mode {
                    PlanMode::None => {}
                    PlanMode::Estimated => {
                        let _ = execute_ignore(client, "SET SHOWPLAN_XML OFF;").await;
                    }
                    PlanMode::Actual => {
                        let _ = execute_ignore(client, "SET STATISTICS XML OFF;").await;
                    }
                }
            }

            let (table, mut plan_xml, mut extra_msgs) = batch_result?;
            let mut result = QueryResult {
                columns: table.columns,
                rows: table.rows,
            };
            messages.append(&mut extra_msgs);

            if plan_xml.is_none() {
                if let Some(xml) = extract_plan_from_result(&result) {
                    plan_xml = Some(xml);
                    if plan_mode == PlanMode::Estimated {
                        result = QueryResult {
                            columns: Vec::new(),
                            rows: Vec::new(),
                        };
                    }
                }
            }

            let plan_operators = {
                let _span = crate::trace::Span::new("engine.run_query_ex.parse_plan");
                plan_xml
                    .as_deref()
                    .map(parse_showplan_operators)
                    .unwrap_or_default()
            };

            let elapsed_ms = started.elapsed().as_millis();
            messages.push(format!(
                "Done in {elapsed_ms} ms — {} row(s), {} plan operator(s)",
                result.rows.len(),
                plan_operators.len()
            ));

            Ok(QueryExecution {
                result,
                messages,
                plan_xml,
                plan_operators,
                elapsed_ms,
            })
        },
    )
    .await
}

fn extract_plan_from_result(result: &QueryResult) -> Option<String> {
    for row in &result.rows {
        for cell in row {
            if cell.trim_start().starts_with("<ShowPlanXML") || cell.contains("<ShowPlanXML") {
                return Some(cell.clone());
            }
        }
    }
    None
}

pub async fn object_definition(
    client: &mut SqlClient,
    database: &str,
    schema: &str,
    name: &str,
) -> Result<String> {
    crate::trace::timed_result(
        "engine.object_definition",
        format!("{database}.{schema}.{name}"),
        async {
            use_database(client, database).await?;
            let sql = format!(
                "SELECT OBJECT_DEFINITION(OBJECT_ID(QUOTENAME({schema}) + N'.' + QUOTENAME({name})))",
                schema = sql_nvarchar(schema),
                name = sql_nvarchar(name),
            );
            let table = query_table(client, sql, Some(1)).await?;
            let def = table
                .rows
                .first()
                .and_then(|r| r.first())
                .map(String::as_str)
                .unwrap_or("");
            if def.is_empty() {
                bail!("no MODULE definition for {schema}.{name} (tables have no OBJECT_DEFINITION)")
            }
            Ok(def.to_string())
        },
    )
    .await
}

async fn use_database(client: &mut SqlClient, database: &str) -> Result<()> {
    crate::trace::timed_result("engine.use_database", database.to_string(), async {
        // Safety comes from bracket quoting (`]` -> `]]`); reject only unusable names.
        if database.is_empty() || database.chars().count() > 128 || database.contains('\0') {
            bail!("invalid database name: {database:?}");
        }
        let sql = format!("USE [{}]", database.replace(']', "]]"));
        execute_ignore(client, sql).await?;
        Ok(())
    })
    .await
}

fn table_child_folder_urn(table_urn: &str, folder: &str) -> String {
    format!("{table_urn}/{folder}")
}

fn child_leaf_urn(table_urn: &str, kind: &str, name: &str) -> String {
    format!("{table_urn}/{kind}[@Name='{}']", escape_urn(name))
}

fn escape_urn(s: &str) -> String {
    s.replace('\'', "''")
}

fn cell(row: &[String], index: usize) -> &str {
    row.get(index).map(String::as_str).unwrap_or("")
}

fn nonempty<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    if value.is_empty() {
        fallback
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readonly_allows_select_and_with() {
        assert!(assert_readonly_sql("SELECT 1").is_ok());
        assert!(assert_readonly_sql("  select * from dbo.T  ").is_ok());
        assert!(assert_readonly_sql("WITH cte AS (SELECT 1 AS n) SELECT * FROM cte").is_ok());
        assert!(assert_readonly_sql("SET NOCOUNT ON; SELECT 1").is_ok());
    }

    #[test]
    fn readonly_rejects_mutating_and_ddl_prefixes() {
        for sql in [
            "INSERT INTO t VALUES (1)",
            "UPDATE t SET x=1",
            "DELETE FROM t",
            "MERGE INTO t USING s ON 1=1 WHEN MATCHED THEN UPDATE SET x=1;",
            "DROP TABLE t",
            "ALTER TABLE t ADD c INT",
            "TRUNCATE TABLE t",
            "EXEC sp_help",
            "EXECUTE sp_help",
            "CREATE TABLE t(i INT)",
            "GRANT SELECT ON t TO u",
            "REVOKE SELECT ON t FROM u",
            "DENY SELECT ON t TO u",
        ] {
            assert!(
                assert_readonly_sql(sql).is_err(),
                "expected reject for: {sql}"
            );
        }
    }

    #[test]
    fn readonly_rejects_batch_after_semicolon() {
        assert!(assert_readonly_sql("SELECT 1; DELETE FROM t").is_err());
        assert!(assert_readonly_sql("SELECT 1;DROP TABLE t").is_err());
        assert!(assert_readonly_sql("SELECT 1; EXEC sp_x").is_err());
    }

    #[test]
    fn readonly_ignores_leading_comments() {
        for sql in [
            "-- c\nDROP TABLE t",
            "/* c */ DELETE FROM t",
            "/* a */ -- b\n /* c */ EXEC x",
            "SELECT 1; -- c\nEXEC x",
        ] {
            assert!(
                assert_readonly_sql(sql).is_err(),
                "expected reject for: {sql}"
            );
        }
        assert!(assert_readonly_sql("-- c\nSELECT 1").is_ok());
        assert!(assert_readonly_sql("/* c */ SELECT 1").is_ok());
    }

    fn dbs(names: &[&str]) -> Vec<DbInfo> {
        names
            .iter()
            .enumerate()
            .map(|(i, n)| DbInfo {
                name: n.to_string(),
                database_id: i as i32 + 1,
                state_desc: "ONLINE".into(),
            })
            .collect()
    }

    #[test]
    fn server_skeleton_has_server_and_all_databases() {
        let (cat, db_id, name) = server_skeleton(
            "srv",
            &dbs(&["master", "Sales.2024", "Società"]),
            "SALES.2024",
        )
        .unwrap();
        assert_eq!(name, "Sales.2024");
        assert_eq!(cat.objects.len(), 4);
        let root = cat.children(None);
        assert_eq!(root.len(), 1);
        assert_eq!(root[0].kind, ObjectKind::Server);
        let kids = cat.children(Some(root[0].id));
        assert_eq!(kids.len(), 3);
        assert!(kids.iter().all(|d| d.kind == ObjectKind::Database));
        assert_eq!(cat.by_id(db_id).unwrap().name, "Sales.2024");
        let mut ids: Vec<_> = cat.objects.iter().map(|o| o.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 4);
        assert_eq!(
            cat.by_id(db_id).unwrap().urn,
            "Server[@Name='srv']/Database[@Name='Sales.2024']"
        );
    }

    #[test]
    fn server_skeleton_errors_when_preferred_missing() {
        assert!(server_skeleton("srv", &dbs(&["master"]), "nope").is_err());
    }

    #[test]
    fn builder_continues_after_max_id_and_sets_parent() {
        let (mut cat, _, _) = server_skeleton("s", &dbs(&["a"]), "a").unwrap();
        let max = cat.objects.iter().map(|o| o.id).max().unwrap();
        let mut b = Builder::new(&mut cat.objects);
        let id = b.add(
            Some(2),
            ObjectKind::Folder,
            "Tables",
            "",
            "",
            "u".into(),
            "",
        );
        assert_eq!(id, max + 1);
        assert_eq!(cat.by_id(id).unwrap().parent_id, Some(2));
    }

    #[test]
    fn urns_escape_single_quotes() {
        assert_eq!(server_urn("o'x"), "Server[@Name='o''x']");
        assert_eq!(
            database_urn("s", "d'b"),
            "Server[@Name='s']/Database[@Name='d''b']"
        );
    }

    #[test]
    fn readonly_is_case_insensitive() {
        assert!(assert_readonly_sql("insert into t values (1)").is_err());
        assert!(assert_readonly_sql("DeLeTe FrOm t").is_err());
    }

    #[test]
    fn table_child_folder_urn_appends_segment() {
        let base = "Server[@Name='s']/Database[@Name='d']/Table[@Name='T' and @Schema='dbo']";
        assert_eq!(
            table_child_folder_urn(base, "Columns"),
            format!("{base}/Columns")
        );
        assert_eq!(
            table_child_folder_urn(base, "Indexes"),
            format!("{base}/Indexes")
        );
    }

    #[test]
    fn column_and_index_urn_shapes() {
        let table = "Server[@Name='s']/Database[@Name='d']/Table[@Name='T' and @Schema='dbo']";
        assert_eq!(
            child_leaf_urn(table, "Column", "Id"),
            format!("{table}/Column[@Name='Id']")
        );
        assert_eq!(
            child_leaf_urn(table, "Index", "PK"),
            format!("{table}/Index[@Name='PK']")
        );
        assert_eq!(
            child_leaf_urn(table, "Column", "O'B"),
            format!("{table}/Column[@Name='O''B']")
        );
    }

    #[test]
    fn escape_urn_doubles_single_quotes() {
        assert_eq!(escape_urn("plain"), "plain");
        assert_eq!(escape_urn("O'Brien"), "O''Brien");
        assert_eq!(escape_urn("a'b'c"), "a''b''c");
    }

    #[test]
    fn extract_plan_finds_showplan_cell() {
        let result = QueryResult {
            columns: vec!["Microsoft SQL Server 2005 XML Showplan".into()],
            rows: vec![vec![
                "<ShowPlanXML xmlns=\"http://schemas.microsoft.com/sqlserver/2004/07/showplan\"/>"
                    .into(),
            ]],
        };
        let xml = extract_plan_from_result(&result).unwrap();
        assert!(xml.contains("ShowPlanXML"));
    }

    #[test]
    fn extract_plan_scans_nested_cells() {
        let result = QueryResult {
            columns: vec!["a".into(), "b".into()],
            rows: vec![
                vec!["1".into(), "2".into()],
                vec![
                    "x".into(),
                    " preamble <ShowPlanXML Version=\"1.2\"/>".into(),
                ],
            ],
        };
        assert!(extract_plan_from_result(&result).is_some());
    }

    #[test]
    fn extract_plan_none_when_absent() {
        let result = QueryResult {
            columns: vec!["n".into()],
            rows: vec![vec!["1".into()]],
        };
        assert!(extract_plan_from_result(&result).is_none());
    }

    #[test]
    fn plan_mode_equality() {
        assert_eq!(PlanMode::None, PlanMode::None);
        assert_ne!(PlanMode::Estimated, PlanMode::Actual);
    }

    #[test]
    fn query_execution_holds_parsed_operators() {
        let xml = r#"<ShowPlanXML><RelOp NodeId="0" PhysicalOp="Select" LogicalOp="Select" EstimateRows="1" EstimatedTotalSubtreeCost="0"/></ShowPlanXML>"#;
        let ops = parse_showplan_operators(xml);
        let exec = QueryExecution {
            result: QueryResult {
                columns: vec![],
                rows: vec![],
            },
            messages: vec!["ok".into()],
            plan_xml: Some(xml.into()),
            plan_operators: ops,
            elapsed_ms: 12,
        };
        assert_eq!(exec.plan_operators.len(), 1);
        assert_eq!(exec.elapsed_ms, 12);
        assert_eq!(exec.messages[0], "ok");
    }
}
