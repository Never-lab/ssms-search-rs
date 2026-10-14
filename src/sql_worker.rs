//! Dedicated SQL worker: one Tokio runtime + optional live `SqlClient` (GUI session).

use anyhow::{Context, Result};
use ssms_search_rs::connection::{connect, is_connection_error, ping, ConnectArgs, SqlClient};
use ssms_search_rs::functions::{self, ColumnInfo, PlanMode, QueryExecution};
use ssms_search_rs::{Catalog, CatalogObject};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

pub enum SqlCommand {
    Connect {
        args: ConnectArgs,
        refresh: bool,
    },
    ExpandDatabase {
        db_id: u64,
        database: String,
        server: String,
        catalog: Catalog,
    },
    LoadTableChildren {
        database: String,
        server: String,
        schema: String,
        table_name: String,
        cols_id: u64,
        idx_id: u64,
        start_id: u64,
    },
    Describe {
        object_id: u64,
        database: String,
        schema: String,
        name: String,
    },
    Script {
        database: String,
        schema: String,
        name: String,
    },
    Execute {
        sql: String,
        mode: PlanMode,
    },
    Disconnect,
}

pub enum SqlResponse {
    Connect {
        refresh: bool,
        result: Result<(Catalog, String, String, String), String>,
    },
    ExpandDatabase {
        db_id: u64,
        result: Result<(Catalog, String), String>,
    },
    LoadTableChildren {
        cols_id: u64,
        idx_id: u64,
        result: Result<Vec<CatalogObject>, String>,
    },
    Describe {
        object_id: u64,
        schema: String,
        name: String,
        result: Result<Vec<ColumnInfo>, String>,
    },
    Script {
        schema: String,
        name: String,
        result: Result<String, String>,
    },
    Execute {
        result: Result<QueryExecution, String>,
    },
    Disconnected,
}

pub struct SqlWorkerHandle {
    cmd_tx: Sender<SqlCommand>,
    res_rx: Receiver<SqlResponse>,
}

impl SqlWorkerHandle {
    pub fn spawn() -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (res_tx, res_rx) = mpsc::channel();
        thread::Builder::new()
            .name("ssms-sql".into())
            .spawn(move || worker_loop(cmd_rx, res_tx))
            .expect("spawn ssms-sql worker");
        Self { cmd_tx, res_rx }
    }

    pub fn submit(&self, cmd: SqlCommand) -> Result<(), String> {
        self.cmd_tx
            .send(cmd)
            .map_err(|_| "SQL worker stopped".to_string())
    }

    pub fn try_recv(&self) -> Option<SqlResponse> {
        self.res_rx.try_recv().ok()
    }
}

fn worker_loop(cmd_rx: Receiver<SqlCommand>, res_tx: Sender<SqlResponse>) {
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            let _ = res_tx.send(SqlResponse::Connect {
                refresh: false,
                result: Err(format!("tokio runtime: {e}")),
            });
            return;
        }
    };

    let mut client: Option<SqlClient> = None;
    let mut session: Option<ConnectArgs> = None;

    while let Ok(cmd) = cmd_rx.recv() {
        let response = rt.block_on(handle_command(&mut client, &mut session, cmd));
        if res_tx.send(response).is_err() {
            break;
        }
    }
}

async fn handle_command(
    client: &mut Option<SqlClient>,
    session: &mut Option<ConnectArgs>,
    cmd: SqlCommand,
) -> SqlResponse {
    match cmd {
        SqlCommand::Disconnect => {
            *client = None;
            *session = None;
            SqlResponse::Disconnected
        }
        SqlCommand::Connect { args, refresh } => {
            let result = async {
                let mut c = connect(&args).await?;
                let (server, db, login) = ping(&mut c).await?;
                let catalog =
                    functions::load_server_catalog(&mut c, &server, &args.database).await?;
                Ok::<_, anyhow::Error>((c, catalog, server, db, login))
            }
            .await;
            match result {
                Ok((c, catalog, server, db, login)) => {
                    *client = Some(c);
                    *session = Some(args);
                    SqlResponse::Connect {
                        refresh,
                        result: Ok((catalog, server, db, login)),
                    }
                }
                Err(e) => {
                    *client = None;
                    if !refresh {
                        *session = None;
                    }
                    SqlResponse::Connect {
                        refresh,
                        result: Err(format!("{e:#}")),
                    }
                }
            }
        }
        SqlCommand::ExpandDatabase {
            db_id,
            database,
            server,
            catalog,
        } => {
            let result = run_expand(client, session, db_id, &database, &server, catalog).await;
            SqlResponse::ExpandDatabase {
                db_id,
                result: result.map_err(|e| format!("{e:#}")),
            }
        }
        SqlCommand::LoadTableChildren {
            database,
            server,
            schema,
            table_name,
            cols_id,
            idx_id,
            start_id,
        } => {
            let result = run_load_table_children(
                client,
                session,
                &database,
                &server,
                &schema,
                &table_name,
                cols_id,
                idx_id,
                start_id,
            )
            .await;
            SqlResponse::LoadTableChildren {
                cols_id,
                idx_id,
                result: result.map_err(|e| format!("{e:#}")),
            }
        }
        SqlCommand::Describe {
            object_id,
            database,
            schema,
            name,
        } => {
            let result = run_describe(client, session, &database, &schema, &name).await;
            SqlResponse::Describe {
                object_id,
                schema,
                name,
                result: result.map_err(|e| format!("{e:#}")),
            }
        }
        SqlCommand::Script {
            database,
            schema,
            name,
        } => {
            let result = run_script(client, session, &database, &schema, &name).await;
            SqlResponse::Script {
                schema,
                name,
                result: result.map_err(|e| format!("{e:#}")),
            }
        }
        SqlCommand::Execute { sql, mode } => {
            let result = run_execute(client, session, &sql, mode).await;
            SqlResponse::Execute {
                result: result.map_err(|e| format!("{e:#}")),
            }
        }
    }
}

async fn ensure_connected(
    client: &mut Option<SqlClient>,
    session: &Option<ConnectArgs>,
) -> Result<()> {
    if client.is_some() {
        return Ok(());
    }
    let args = session
        .as_ref()
        .context("not connected — no session on SQL worker")?;
    *client = Some(connect(args).await?);
    Ok(())
}

async fn run_expand(
    client: &mut Option<SqlClient>,
    session: &Option<ConnectArgs>,
    db_id: u64,
    database: &str,
    server: &str,
    mut catalog: Catalog,
) -> Result<(Catalog, String)> {
    ensure_connected(client, session).await?;
    match functions::ensure_database_loaded(
        client.as_mut().unwrap(),
        server,
        database,
        &mut catalog,
        db_id,
    )
    .await
    {
        Ok(()) => Ok((catalog, database.to_string())),
        Err(e) if is_connection_error(&e) => {
            *client = None;
            ensure_connected(client, session).await?;
            functions::ensure_database_loaded(
                client.as_mut().unwrap(),
                server,
                database,
                &mut catalog,
                db_id,
            )
            .await?;
            Ok((catalog, database.to_string()))
        }
        Err(e) => Err(e),
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_load_table_children(
    client: &mut Option<SqlClient>,
    session: &Option<ConnectArgs>,
    database: &str,
    server: &str,
    schema: &str,
    table_name: &str,
    cols_id: u64,
    idx_id: u64,
    start_id: u64,
) -> Result<Vec<CatalogObject>> {
    ensure_connected(client, session).await?;
    match functions::load_table_children(
        client.as_mut().unwrap(),
        database,
        server,
        schema,
        table_name,
        cols_id,
        idx_id,
        start_id,
    )
    .await
    {
        Ok(v) => Ok(v),
        Err(e) if is_connection_error(&e) => {
            *client = None;
            ensure_connected(client, session).await?;
            functions::load_table_children(
                client.as_mut().unwrap(),
                database,
                server,
                schema,
                table_name,
                cols_id,
                idx_id,
                start_id,
            )
            .await
        }
        Err(e) => Err(e),
    }
}

async fn run_describe(
    client: &mut Option<SqlClient>,
    session: &Option<ConnectArgs>,
    database: &str,
    schema: &str,
    name: &str,
) -> Result<Vec<ColumnInfo>> {
    ensure_connected(client, session).await?;
    match functions::describe_object(client.as_mut().unwrap(), database, schema, name).await {
        Ok(v) => Ok(v),
        Err(e) if is_connection_error(&e) => {
            *client = None;
            ensure_connected(client, session).await?;
            functions::describe_object(client.as_mut().unwrap(), database, schema, name).await
        }
        Err(e) => Err(e),
    }
}

async fn run_script(
    client: &mut Option<SqlClient>,
    session: &Option<ConnectArgs>,
    database: &str,
    schema: &str,
    name: &str,
) -> Result<String> {
    ensure_connected(client, session).await?;
    match functions::object_definition(client.as_mut().unwrap(), database, schema, name).await {
        Ok(v) => Ok(v),
        Err(e) if is_connection_error(&e) => {
            *client = None;
            ensure_connected(client, session).await?;
            functions::object_definition(client.as_mut().unwrap(), database, schema, name).await
        }
        Err(e) => Err(e),
    }
}

async fn run_execute(
    client: &mut Option<SqlClient>,
    session: &Option<ConnectArgs>,
    sql: &str,
    mode: PlanMode,
) -> Result<QueryExecution> {
    ensure_connected(client, session).await?;
    match functions::run_query_ex(client.as_mut().unwrap(), sql, 500, mode).await {
        Ok(v) => Ok(v),
        Err(e) if is_connection_error(&e) => {
            *client = None;
            ensure_connected(client, session).await?;
            functions::run_query_ex(client.as_mut().unwrap(), sql, 500, mode).await
        }
        Err(e) => Err(e),
    }
}
