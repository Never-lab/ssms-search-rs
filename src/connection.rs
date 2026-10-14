//! SQL Server connection (TDS via tiberius).

use anyhow::{bail, Context, Result};
use clap::Args;
use tiberius::{AuthMethod, Client, Config, EncryptionLevel};
use tokio::net::TcpStream;
use tokio_util::compat::{Compat, TokioAsyncWriteCompatExt};

pub type SqlClient = Client<Compat<TcpStream>>;

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

    /// Trust server certificate (dev / self-signed)
    #[arg(long, env = "MSSQL_TRUST_CERT", default_value_t = true)]
    pub trust_cert: bool,

    /// Application name shown in SSMS/sp_who
    #[arg(long, default_value = "ssms-search-rs")]
    pub app_name: String,
}

impl ConnectArgs {
    pub fn to_config(&self) -> Result<Config> {
        let mut config = Config::new();

        let (host, port) = parse_server_host_port(&self.server, self.port);
        config.host(host);
        config.port(port);
        config.database(&self.database);
        config.application_name(&self.app_name);

        if self.trust_cert {
            config.trust_cert();
        }
        // Prefer encryption when available; TrustCert covers lab boxes.
        config.encryption(EncryptionLevel::Required);

        match (&self.user, &self.password) {
            (Some(u), Some(p)) => {
                config.authentication(AuthMethod::sql_server(u, p));
            }
            (None, None) => {
                bail!(
                    "SQL auth required: set --user/--password or MSSQL_USER/MSSQL_PASSWORD \
                     (Windows Integrated Auth not wired yet on this toolchain)"
                );
            }
            _ => bail!("both --user and --password are required for SQL authentication"),
        }

        Ok(config)
    }
}

/// Parse `host`, `host,port`, `host:port`, or `host\instance` (instance keeps default port).
pub fn parse_server_host_port(server: &str, default_port: u16) -> (String, u16) {
    // host,port
    if let Some((h, p)) = server.rsplit_once(',') {
        if let Ok(port) = p.trim().parse::<u16>() {
            return (h.trim().to_string(), port);
        }
    }
    // host:port (non-instance)
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
    let (host, port) = parse_server_host_port(&args.server, args.port);
    crate::trace::timed_result(
        "engine.connect",
        format!("{host}:{port} db={}", args.database),
        async {
            let config = args.to_config()?;
            let tcp = TcpStream::connect(format!("{host}:{port}"))
                .await
                .with_context(|| format!("TCP connect failed to {host}:{port}"))?;
            tcp.set_nodelay(true)?;
            let client = Client::connect(config, tcp.compat_write())
                .await
                .context("TDS login failed")?;
            Ok(client)
        },
    )
    .await
}

pub async fn ping(client: &mut SqlClient) -> Result<(String, String, String)> {
    crate::trace::timed_result("engine.ping", "", async {
        let stream = client
            .query(
                "SELECT @@SERVERNAME AS server_name, DB_NAME() AS db_name, SUSER_SNAME() AS login_name",
                &[],
            )
            .await?;
        let row = stream.into_row().await?.context("empty ping result")?;
        let server: String = row.get::<&str, _>(0).unwrap_or("?").to_string();
        let db: String = row.get::<&str, _>(1).unwrap_or("?").to_string();
        let login: String = row.get::<&str, _>(2).unwrap_or("?").to_string();
        Ok((server, db, login))
    })
    .await
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
        // backslash instance must not be split on ':' inside future forms; comma still wins
        assert_eq!(
            parse_server_host_port(r"HOST\SQLEXPRESS", 1433),
            (r"HOST\SQLEXPRESS".into(), 1433)
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
    fn to_config_requires_both_user_and_password() {
        let only_user = args("localhost", 1433, Some("sa"), None);
        assert!(only_user.to_config().is_err());
        let only_pw = args("localhost", 1433, None, Some("x"));
        assert!(only_pw.to_config().is_err());
        let none = args("localhost", 1433, None, None);
        assert!(none.to_config().is_err());
    }

    #[test]
    fn to_config_accepts_sql_auth() {
        let ok = args("sql.lab,14333", 1433, Some("sa"), Some("secret"));
        assert!(ok.to_config().is_ok());
    }
}
