//! SQL Server connection (TDS via `mssql-tds`, same driver choice as Quervio).

use anyhow::{bail, Context, Result};
use clap::Args;
use mssql_tds::connection::client_context::{ClientContext, TdsAuthenticationMethod};
use mssql_tds::connection::tds_client::{ResultSet, StatementResult, TdsClient};
use mssql_tds::connection_provider::tds_connection_provider::TdsConnectionProvider;
use mssql_tds::core::EncryptionSetting;
use std::time::Duration;

use crate::format;

/// Cap hung TCP/TDS logins so the GUI can show an error and retry.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

pub type SqlClient = TdsClient;

#[derive(Debug, Clone, Args)]
pub struct ConnectArgs {
    /// Host or host\\instance (also MSSQL_SERVER)
    #[arg(long, env = "MSSQL_SERVER", default_value = "localhost")]
    pub server: String,

    /// Port (ignored when server already contains ,port)
    #[arg(long, env = "MSSQL_PORT", default_value_t = 1433)]
    pub port: u16,

    /// Initial database (also MSSQL_DATABASE)
    #[arg(long, short = 'd', env = "MSSQL_DATABASE", default_value = "master")]
    pub database: String,

    /// SQL login (also MSSQL_USER). Required unless --trusted.
    #[arg(long, short = 'U', env = "MSSQL_USER")]
    pub user: Option<String>,

    /// SQL password (also MSSQL_PASSWORD)
    #[arg(long, short = 'P', env = "MSSQL_PASSWORD")]
    pub password: Option<String>,

    /// Windows Integrated Auth / Trusted Connection (SSPI). Ignores -U/-P.
    #[arg(long, env = "MSSQL_TRUSTED", default_value_t = false)]
    pub trusted: bool,

    /// Trust server certificate (dev / self-signed)
    #[arg(long, env = "MSSQL_TRUST_CERT", default_value_t = true)]
    pub trust_cert: bool,

    /// Application name shown in SSMS/sp_who
    #[arg(long, default_value = "ssms-search-rs")]
    pub app_name: String,
}

impl ConnectArgs {
    /// Address string accepted by `mssql-tds` (`host`, `host,port`, `host\instance`).
    pub fn server_address(&self) -> String {
        let (host, port) = parse_server_host_port(&self.server, self.port);
        if self.server.contains(',') {
            return self.server.trim().to_string();
        }
        if host.contains('\\') {
            return host;
        }
        format!("{host},{port}")
    }

    pub fn to_context(&self) -> Result<ClientContext> {
        let mut context = ClientContext::default();
        context.application_name = self.app_name.clone();
        context.login_timeout = Some(CONNECT_TIMEOUT.as_secs() as u32);
        context.database = self.database.clone();
        context.encryption_options.mode = EncryptionSetting::Required;
        context.encryption_options.trust_server_certificate = self.trust_cert;

        if self.trusted {
            #[cfg(windows)]
            {
                context.tds_authentication_method = TdsAuthenticationMethod::SSPI;
            }
            #[cfg(not(windows))]
            {
                bail!(
                    "Windows Integrated Auth (--trusted) is only available on Windows \
                     (mssql-tds SSPI)"
                );
            }
        } else {
            match (&self.user, &self.password) {
                (Some(u), Some(p)) => {
                    context.tds_authentication_method = TdsAuthenticationMethod::Password;
                    context.user_name = u.clone();
                    context.password = p.clone();
                }
                (None, None) => {
                    bail!(
                        "SQL auth required: set --user/--password (or MSSQL_USER/MSSQL_PASSWORD), \
                         or use --trusted for Windows Integrated Auth"
                    );
                }
                _ => bail!("both --user and --password are required for SQL authentication"),
            }
        }

        Ok(context)
    }
}

/// Parse `host`, `host,port`, `host:port`, or `host\instance` (instance keeps default port).
pub fn parse_server_host_port(server: &str, default_port: u16) -> (String, u16) {
    if let Some((h, p)) = server.rsplit_once(',') {
        if let Ok(port) = p.trim().parse::<u16>() {
            return (h.trim().to_string(), port);
        }
    }
    if !server.contains('\\') {
        if let Some((h, p)) = server.rsplit_once(':') {
            if let Ok(port) = p.trim().parse::<u16>() {
                return (h.trim().to_string(), port);
            }
        }
    }
    (server.to_string(), default_port)
}

pub async fn connect(args: &ConnectArgs) -> Result<SqlClient> {
    let address = args.server_address();
    crate::trace::timed_result(
        "engine.connect",
        format!("{address} db={}", args.database),
        async {
            let context = args.to_context()?;
            let fut = TdsConnectionProvider {}.create_client(context, &address, None);
            match tokio::time::timeout(CONNECT_TIMEOUT, fut).await {
                Ok(Ok(client)) => Ok(client),
                Ok(Err(e)) => Err(e).context("TDS login failed"),
                Err(_) => bail!(
                    "connection timed out after {}s to {address}",
                    CONNECT_TIMEOUT.as_secs()
                ),
            }
        },
    )
    .await
}

pub async fn ping(client: &mut SqlClient) -> Result<(String, String, String)> {
    crate::trace::timed_result("engine.ping", "", async {
        let table = query_table(
            client,
            "SELECT @@SERVERNAME AS server_name, DB_NAME() AS db_name, SUSER_SNAME() AS login_name",
            None,
        )
        .await?;
        let row = table.rows.first().context("empty ping result")?;
        Ok((
            row.first().cloned().unwrap_or_else(|| "?".into()),
            row.get(1).cloned().unwrap_or_else(|| "?".into()),
            row.get(2).cloned().unwrap_or_else(|| "?".into()),
        ))
    })
    .await
}

/// Heuristic: error chain looks like a dead TDS/TCP session (safe to reconnect once).
pub fn is_connection_error(err: &anyhow::Error) -> bool {
    for cause in err.chain() {
        let s = cause.to_string().to_ascii_lowercase();
        if s.contains("connection")
            || s.contains("broken pipe")
            || s.contains("connection reset")
            || s.contains("timed out")
            || s.contains("timeout")
            || s.contains("network")
            || s.contains("eof")
            || s.contains("closed")
            || s.contains("transport")
            || s.contains("os error 10054")
            || s.contains("os error 10053")
            || s.contains("not connected")
        {
            return true;
        }
    }
    false
}

#[derive(Debug, Clone, Default)]
pub struct QueryTable {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

/// Escape a Unicode string literal for embedding in T-SQL (`N'…'`).
pub fn sql_nvarchar(value: &str) -> String {
    format!("N'{}'", value.replace('\'', "''"))
}

/// Run a batch and collect the first result set as text cells (empty = SQL NULL).
pub async fn query_table(
    client: &mut SqlClient,
    sql: impl Into<String>,
    max_rows: Option<usize>,
) -> Result<QueryTable> {
    let (table, _messages) = collect_results(client, sql, max_rows, true).await?;
    Ok(table)
}

/// Execute a batch, draining all result sets (e.g. `USE`, `SET …`).
pub async fn execute_ignore(client: &mut SqlClient, sql: impl Into<String>) -> Result<()> {
    let (_table, _messages) = collect_results(client, sql, Some(0), false).await?;
    Ok(())
}

async fn collect_results(
    client: &mut SqlClient,
    sql: impl Into<String>,
    max_rows: Option<usize>,
    first_set_only: bool,
) -> Result<(QueryTable, Vec<String>)> {
    let mut table = QueryTable::default();
    let mut messages = Vec::new();
    let mut kept_set = false;

    let outcome: Result<()> = async {
        let mut step = client.execute(sql.into(), ()).await?;
        loop {
            match step {
                StatementResult::End => break,
                StatementResult::Rows => {
                    let meta = client.get_metadata();
                    let width = meta.len().max(1);
                    let cols: Vec<String> = meta.iter().map(|c| c.column_name.clone()).collect();
                    let take_rows = !first_set_only || !kept_set;
                    if take_rows && table.columns.is_empty() {
                        table.columns = cols;
                    }
                    while let Some(row) = client.next_row().await? {
                        if !take_rows {
                            continue;
                        }
                        if let Some(limit) = max_rows {
                            if table.rows.len() >= limit {
                                continue;
                            }
                        }
                        let cells: Vec<String> = (0..width)
                            .map(|i| row.get(i).map(cell_to_string).unwrap_or_default())
                            .collect();
                        table.rows.push(cells);
                    }
                    if take_rows {
                        kept_set = true;
                    }
                }
                StatementResult::NoRows { rows_affected } => {
                    if let Some(count) = rows_affected {
                        messages.push(rows_affected_message(count));
                    }
                }
            }
            for info in client.take_info_messages() {
                messages.push(info.message);
            }
            step = client.advance().await?;
        }
        Ok(())
    }
    .await;
    let closed = client.close_query().await;
    outcome?;
    closed?;
    Ok((table, messages))
}

/// Collect data grid + ShowPlanXML + INFO messages for the Execute path.
///
/// Rows are streamed with a `max_rows` cap (no full-set buffer). Showplan sets
/// are detected from column metadata, same rule as Quervio.
pub async fn collect_query_batch(
    client: &mut SqlClient,
    sql: impl Into<String>,
    max_rows: usize,
) -> Result<(QueryTable, Option<String>, Vec<String>)> {
    let mut data = QueryTable::default();
    let mut plan_xml = None;
    let mut messages = Vec::new();
    let mut result_sets = 0u32;

    let outcome: Result<()> = async {
        let mut step = client.execute(sql.into(), ()).await?;
        'batch: loop {
            match step {
                StatementResult::End => break,
                StatementResult::Rows => {
                    result_sets += 1;
                    let meta = client.get_metadata();
                    let width = meta.len().max(1);
                    let cols: Vec<String> = meta.iter().map(|c| c.column_name.clone()).collect();
                    let is_plan = is_plan_result(cols.iter().map(String::as_str));

                    if is_plan {
                        while let Some(row) = client.next_row().await? {
                            if plan_xml.is_some() {
                                continue;
                            }
                            for i in 0..width {
                                let cell = row.get(i).map(cell_to_string).unwrap_or_default();
                                if cell.trim_start().starts_with("<ShowPlanXML")
                                    || cell.contains("<ShowPlanXML")
                                {
                                    plan_xml = Some(cell);
                                    messages.push(format!(
                                        "Captured ShowPlanXML (result set #{result_sets})"
                                    ));
                                    break;
                                }
                            }
                        }
                    } else {
                        if data.columns.is_empty() {
                            data.columns = cols;
                        }
                        while let Some(row) = client.next_row().await? {
                            if data.rows.len() >= max_rows {
                                // Match Quervio: Attention instead of draining the rest.
                                let _ = client.cancel_query(0).await;
                                messages.push(format!(
                                    "Stopped at the limit of {max_rows} rows; the rest of the query was cancelled."
                                ));
                                break 'batch;
                            }
                            let cells: Vec<String> = (0..width)
                                .map(|i| row.get(i).map(cell_to_string).unwrap_or_default())
                                .collect();
                            data.rows.push(cells);
                        }
                    }
                }
                StatementResult::NoRows { rows_affected } => {
                    if let Some(count) = rows_affected {
                        messages.push(rows_affected_message(count));
                    }
                }
            }
            for info in client.take_info_messages() {
                messages.push(info.message);
            }
            step = client.advance().await?;
        }
        Ok(())
    }
    .await;
    let closed = client.close_query().await;
    outcome?;
    closed?;
    Ok((data, plan_xml, messages))
}

/// Showplan result set: single XML column from `SET SHOWPLAN_XML` / `STATISTICS XML`.
fn is_plan_result<'a>(mut names: impl Iterator<Item = &'a str>) -> bool {
    matches!(
        (names.next(), names.next()),
        (Some(name), None)
            if name.starts_with("Microsoft SQL Server") && name.ends_with("XML Showplan")
    )
}

fn rows_affected_message(count: u64) -> String {
    if count == 1 {
        "(1 row affected)".into()
    } else {
        format!("({count} rows affected)")
    }
}

fn cell_to_string(value: &mssql_tds::datatypes::column_values::ColumnValues) -> String {
    let mut out = String::new();
    match format::write_value(value, &mut out) {
        Some(()) => out,
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(server: &str, port: u16, user: Option<&str>, password: Option<&str>) -> ConnectArgs {
        ConnectArgs {
            server: server.into(),
            port,
            database: "master".into(),
            user: user.map(str::to_string),
            password: password.map(str::to_string),
            trusted: false,
            trust_cert: true,
            app_name: "ssms-search-rs-test".into(),
        }
    }

    #[test]
    fn parse_plain_host_uses_default_port() {
        assert_eq!(
            parse_server_host_port("localhost", 1433),
            ("localhost".into(), 1433)
        );
        assert_eq!(
            parse_server_host_port("sql.example.com", 1434),
            ("sql.example.com".into(), 1434)
        );
    }

    #[test]
    fn parse_host_comma_port() {
        assert_eq!(
            parse_server_host_port("db.local,14333", 1433),
            ("db.local".into(), 14333)
        );
        assert_eq!(
            parse_server_host_port("  10.0.0.5 , 1500 ", 1433),
            ("10.0.0.5".into(), 1500)
        );
    }

    #[test]
    fn parse_host_colon_port() {
        assert_eq!(
            parse_server_host_port("127.0.0.1:1433", 99),
            ("127.0.0.1".into(), 1433)
        );
    }

    #[test]
    fn parse_named_instance_keeps_default_port() {
        assert_eq!(
            parse_server_host_port(r"localhost\SQLEXPRESS", 1433),
            (r"localhost\SQLEXPRESS".into(), 1433)
        );
        assert_eq!(
            parse_server_host_port(r"HOST\SQLEXPRESS,1435", 1433),
            (r"HOST\SQLEXPRESS".into(), 1435)
        );
    }

    #[test]
    fn parse_invalid_port_suffix_falls_back() {
        assert_eq!(
            parse_server_host_port("host,notaport", 1433),
            ("host,notaport".into(), 1433)
        );
        assert_eq!(
            parse_server_host_port("host:notaport", 1433),
            ("host:notaport".into(), 1433)
        );
    }

    #[test]
    fn server_address_appends_port() {
        let a = args("localhost", 1433, Some("sa"), Some("x"));
        assert_eq!(a.server_address(), "localhost,1433");
    }

    #[test]
    fn server_address_keeps_comma_port() {
        let a = args("db.local,1500", 1433, Some("sa"), Some("x"));
        assert_eq!(a.server_address(), "db.local,1500");
    }

    #[test]
    fn sql_nvarchar_escapes_quotes() {
        assert_eq!(sql_nvarchar("O'Brien"), "N'O''Brien'");
    }

    #[test]
    fn sql_auth_requires_both_user_and_password() {
        assert!(args("localhost", 1433, Some("sa"), None)
            .to_context()
            .is_err());
        assert!(args("localhost", 1433, None, Some("x"))
            .to_context()
            .is_err());
        assert!(args("localhost", 1433, None, None).to_context().is_err());
        assert!(args("localhost", 1433, Some("sa"), Some("x"))
            .to_context()
            .is_ok());
    }

    #[test]
    fn to_context_accepts_trusted_without_user() {
        let mut a = args("localhost", 1433, None, None);
        a.trusted = true;
        #[cfg(windows)]
        assert!(a.to_context().is_ok());
        #[cfg(not(windows))]
        assert!(a.to_context().is_err());
    }

    #[test]
    fn is_connection_error_detects_reset() {
        let e = anyhow::anyhow!("TCP connection reset by peer");
        assert!(is_connection_error(&e));
    }

    #[test]
    fn is_connection_error_ignores_sql_syntax() {
        let e = anyhow::anyhow!("Incorrect syntax near 'SELEC'");
        assert!(!is_connection_error(&e));
    }

    #[test]
    fn only_the_showplan_xml_column_is_a_plan() {
        assert!(is_plan_result(
            ["Microsoft SQL Server 2005 XML Showplan"].into_iter()
        ));
        assert!(!is_plan_result(["ShowplanFlag", "Id"].into_iter()));
        assert!(!is_plan_result(["Showplan"].into_iter()));
        assert!(!is_plan_result(
            ["Microsoft SQL Server 2005 XML Showplan", "Other"].into_iter()
        ));
        assert!(!is_plan_result(std::iter::empty()));
    }

    #[test]
    fn rows_affected_message_pluralizes() {
        assert_eq!(rows_affected_message(0), "(0 rows affected)");
        assert_eq!(rows_affected_message(1), "(1 row affected)");
        assert_eq!(rows_affected_message(2), "(2 rows affected)");
    }
}
