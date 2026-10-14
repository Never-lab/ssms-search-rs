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

fn parse_server_host_port(server: &str, default_port: u16) -> (String, u16) {
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
    let config = args.to_config()?;
    let tcp = TcpStream::connect(format!("{host}:{port}"))
        .await
        .with_context(|| format!("TCP connect failed to {host}:{port}"))?;
    tcp.set_nodelay(true)?;
    let client = Client::connect(config, tcp.compat_write())
        .await
        .context("TDS login failed")?;
    Ok(client)
}

pub async fn ping(client: &mut SqlClient) -> Result<(String, String, String)> {
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
}
