//! Multi-function SQL operations (SSMS-inspired).

use crate::catalog::{Catalog, CatalogObject, ObjectKind};
use crate::connection::SqlClient;
use crate::plan::{parse_showplan_operators, PlanOperator};
use anyhow::{bail, Context, Result};
use futures::stream::TryStreamExt;
use tiberius::QueryItem;
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
    let stream = client
        .query(
            "SELECT name, database_id, state_desc \
             FROM sys.databases \
             WHERE state = 0 \
             ORDER BY name",
            &[],
        )
        .await?;
    let rows = stream.into_first_result().await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(DbInfo {
            name: row.get::<&str, _>(0).unwrap_or("").to_string(),
            database_id: row.get::<i32, _>(1).unwrap_or(0),
            state_desc: row.get::<&str, _>(2).unwrap_or("").to_string(),
        });
    }
    Ok(out)
}

/// Load Object Explorer-shaped catalog for one database (tables/views/procs/functions).
pub async fn load_catalog(client: &mut SqlClient, server: &str, database: &str) -> Result<Catalog> {
    use_database(client, database).await?;

    let q = "\
SELECT s.name AS schema_name, \
       o.name AS object_name, \
       o.type AS type_code, \
       ISNULL(USER_NAME(o.principal_id), s.name) AS owner_name \
FROM sys.objects AS o \
INNER JOIN sys.schemas AS s ON o.schema_id = s.schema_id \
WHERE o.is_ms_shipped = 0 \
  AND o.type IN (N'U', N'V', N'P', N'FN', N'IF', N'TF') \
ORDER BY \
  CASE o.type WHEN N'U' THEN 1 WHEN N'V' THEN 2 WHEN N'P' THEN 3 ELSE 4 END, \
  s.name, o.name";

    let stream = client.query(q, &[]).await?;
    let rows = stream.into_first_result().await?;

    let mut objects = Vec::new();
    let mut next = 1u64;
    let mut add = |parent: Option<u64>,
                   kind: ObjectKind,
                   name: &str,
                   schema: &str,
                   owner: &str,
                   urn: String|
     -> u64 {
        let id = next;
        next += 1;
        objects.push(CatalogObject {
            id,
            parent_id: parent,
            kind,
            name: name.to_string(),
            schema: schema.to_string(),
            owner: owner.to_string(),
            urn,
        });
        id
    };

    let server_esc = escape_urn(server);
    let db_esc = escape_urn(database);
    let server_id = add(
        None,
        ObjectKind::Server,
        server,
        "",
        "",
        format!("Server[@Name='{server_esc}']"),
    );
    let db_id = add(
        Some(server_id),
        ObjectKind::Database,
        database,
        "",
        "",
        format!("Server[@Name='{server_esc}']/Database[@Name='{db_esc}']"),
    );
    let tables_id = add(
        Some(db_id),
        ObjectKind::Folder,
        "Tables",
        "",
        "",
        format!("Server[@Name='{server_esc}']/Database[@Name='{db_esc}']/Table"),
    );
    let views_id = add(
        Some(db_id),
        ObjectKind::Folder,
        "Views",
        "",
        "",
        format!("Server[@Name='{server_esc}']/Database[@Name='{db_esc}']/View"),
    );
    let procs_id = add(
        Some(db_id),
        ObjectKind::Folder,
        "Stored Procedures",
        "",
        "",
        format!("Server[@Name='{server_esc}']/Database[@Name='{db_esc}']/StoredProcedure"),
    );
    let funcs_id = add(
        Some(db_id),
        ObjectKind::Folder,
        "Functions",
        "",
        "",
        format!("Server[@Name='{server_esc}']/Database[@Name='{db_esc}']/UserDefinedFunction"),
    );

    for row in rows {
        let schema = row.get::<&str, _>(0).unwrap_or("dbo");
        let name = row.get::<&str, _>(1).unwrap_or("");
        let type_code = row.get::<&str, _>(2).unwrap_or("U").trim();
        let owner = row.get::<&str, _>(3).unwrap_or(schema);
        let (kind, parent, type_urn) = match type_code {
            "U" => (ObjectKind::Table, tables_id, "Table"),
            "V" => (ObjectKind::View, views_id, "View"),
            "P" => (ObjectKind::Procedure, procs_id, "StoredProcedure"),
            "FN" | "IF" | "TF" => (ObjectKind::Function, funcs_id, "UserDefinedFunction"),
            _ => continue,
        };
        let schema_esc = escape_urn(schema);
        let name_esc = escape_urn(name);
        add(
            Some(parent),
            kind,
            name,
            schema,
            owner,
            format!(
                "Server[@Name='{server_esc}']/Database[@Name='{db_esc}']/{type_urn}[@Name='{name_esc}' and @Schema='{schema_esc}']"
            ),
        );
    }

    Ok(Catalog { objects })
}

pub async fn describe_object(
    client: &mut SqlClient,
    database: &str,
    schema: &str,
    name: &str,
) -> Result<Vec<ColumnInfo>> {
    use_database(client, database).await?;
    let stream = client
        .query(
            "SELECT c.name, \
                    t.name AS type_name, \
                    c.max_length, \
                    c.is_nullable \
             FROM sys.columns AS c \
             INNER JOIN sys.types AS t ON c.user_type_id = t.user_type_id \
             INNER JOIN sys.objects AS o ON c.object_id = o.object_id \
             INNER JOIN sys.schemas AS s ON o.schema_id = s.schema_id \
             WHERE s.name = @P1 AND o.name = @P2 \
             ORDER BY c.column_id",
            &[&schema, &name],
        )
        .await?;
    let rows = stream.into_first_result().await?;
    if rows.is_empty() {
        bail!("object not found or has no columns: {schema}.{name}");
    }
    Ok(rows
        .into_iter()
        .map(|row| ColumnInfo {
            name: row.get::<&str, _>(0).unwrap_or("").to_string(),
            data_type: row.get::<&str, _>(1).unwrap_or("").to_string(),
            max_length: row.get::<i16, _>(2).unwrap_or(0),
            is_nullable: row.get::<bool, _>(3).unwrap_or(false),
        })
        .collect())
}

fn assert_readonly_sql(sql: &str) -> Result<()> {
    let upper = sql.trim().to_ascii_uppercase();
    for banned in [
        "INSERT ", "UPDATE ", "DELETE ", "MERGE ", "DROP ", "ALTER ", "TRUNCATE ", "EXEC ",
        "EXECUTE ", "CREATE ", "GRANT ", "REVOKE ", "DENY ",
    ] {
        if upper.starts_with(banned.trim()) || upper.contains(&format!(";{banned}")) {
            bail!("refusing mutating/DDL statement: detected '{banned}'");
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

    let mut messages = Vec::new();
    let started = Instant::now();

    match plan_mode {
        PlanMode::None => {}
        PlanMode::Estimated => {
            client
                .simple_query("SET SHOWPLAN_XML ON;")
                .await?
                .into_results()
                .await?;
            messages.push("SET SHOWPLAN_XML ON — estimated plan (query not executed)".into());
        }
        PlanMode::Actual => {
            client
                .simple_query("SET STATISTICS XML ON;")
                .await?
                .into_results()
                .await?;
            messages.push("SET STATISTICS XML ON — actual execution plan".into());
        }
    }

    let batch_result = collect_query_streams(client, trimmed, max_rows).await;

    // Always try to turn plan settings off (best-effort).
    match plan_mode {
        PlanMode::None => {}
        PlanMode::Estimated => {
            if let Ok(s) = client.simple_query("SET SHOWPLAN_XML OFF;").await {
                let _ = s.into_results().await;
            }
        }
        PlanMode::Actual => {
            if let Ok(s) = client.simple_query("SET STATISTICS XML OFF;").await {
                let _ = s.into_results().await;
            }
        }
    }

    let (mut result, mut plan_xml, mut extra_msgs) = batch_result?;
    messages.append(&mut extra_msgs);

    // SHOWPLAN returns plan as the only "result" — often one XML column.
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

    let plan_operators = plan_xml
        .as_deref()
        .map(parse_showplan_operators)
        .unwrap_or_default();

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
}

async fn collect_query_streams(
    client: &mut SqlClient,
    sql: &str,
    max_rows: usize,
) -> Result<(QueryResult, Option<String>, Vec<String>)> {
    let mut stream = client.simple_query(sql).await?;
    let mut data_columns = Vec::new();
    let mut data_rows = Vec::new();
    let mut current_width = 0usize;
    let mut plan_xml = None;
    let mut messages = Vec::new();
    let mut result_sets = 0u32;

    while let Some(item) = stream.try_next().await? {
        match item {
            QueryItem::Metadata(meta) => {
                result_sets += 1;
                current_width = meta.columns().len().max(1);
                if data_columns.is_empty() {
                    data_columns = meta.columns().iter().map(|c| c.name().to_string()).collect();
                }
            }
            QueryItem::Row(row) => {
                let mut vals = Vec::with_capacity(current_width.max(1));
                for i in 0..current_width.max(1) {
                    vals.push(cell_to_string(&row, i));
                }
                let joined = vals.first().cloned().unwrap_or_default();
                if joined.trim_start().starts_with("<ShowPlanXML")
                    || joined.contains("<ShowPlanXML")
                {
                    plan_xml = Some(joined);
                    messages.push(format!("Captured ShowPlanXML (result set #{result_sets})"));
                } else if data_rows.len() < max_rows {
                    data_rows.push(vals);
                }
            }
        }
    }

    Ok((
        QueryResult {
            columns: data_columns,
            rows: data_rows,
        },
        plan_xml,
        messages,
    ))
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
    use_database(client, database).await?;
    let stream = client
        .query(
            "SELECT OBJECT_DEFINITION(OBJECT_ID(@P1))",
            &[&format!("{schema}.{name}")],
        )
        .await?;
    let row = stream
        .into_row()
        .await?
        .context("no definition returned")?;
    match row.get::<&str, _>(0) {
        Some(s) if !s.is_empty() => Ok(s.to_string()),
        _ => bail!("no MODULE definition for {schema}.{name} (tables have no OBJECT_DEFINITION)"),
    }
}

async fn use_database(client: &mut SqlClient, database: &str) -> Result<()> {
    if !database
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == ' ')
    {
        bail!("unsafe database name: {database}");
    }
    let sql = format!("USE [{}]", database.replace(']', "]]"));
    client.simple_query(sql).await?.into_results().await?;
    Ok(())
}

fn escape_urn(s: &str) -> String {
    s.replace('\'', "''")
}

fn cell_to_string(row: &tiberius::Row, index: usize) -> String {
    // Try common TDS types; fall back to Debug-ish empty.
    if let Some(v) = row.get::<&str, _>(index) {
        return v.to_string();
    }
    if let Some(v) = row.get::<i32, _>(index) {
        return v.to_string();
    }
    if let Some(v) = row.get::<i64, _>(index) {
        return v.to_string();
    }
    if let Some(v) = row.get::<i16, _>(index) {
        return v.to_string();
    }
    if let Some(v) = row.get::<u8, _>(index) {
        return v.to_string();
    }
    if let Some(v) = row.get::<bool, _>(index) {
        return v.to_string();
    }
    if let Some(v) = row.get::<f64, _>(index) {
        return v.to_string();
    }
    if let Some(v) = row.get::<f32, _>(index) {
        return v.to_string();
    }
    String::new()
}
