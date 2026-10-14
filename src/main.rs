//! Binary entry: desktop GUI by default; CLI subcommands for headless ops.

mod gui;

use anyhow::Context;
use clap::{Parser, Subcommand};
use ssms_search_rs::connection::{connect, ping, ConnectArgs};
use ssms_search_rs::filter::{FilterItem, FilterOperator, SearchConditions};
use ssms_search_rs::{functions, Catalog};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "ssms-search-rs",
    about = "SSMS-inspired Object Explorer (desktop UI) + SQL toolkit"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Open the desktop GUI (default when no subcommand)
    Gui,
    /// Test TDS login
    Connect {
        #[command(flatten)]
        conn: ConnectArgs,
    },
    /// List online databases
    Databases {
        #[command(flatten)]
        conn: ConnectArgs,
        #[arg(long)]
        json: bool,
    },
    /// Filter/search objects
    Search {
        #[command(flatten)]
        conn: ConnectArgs,
        #[arg(short, long, default_value = "Name")]
        column: String,
        #[arg(short, long, default_value = "Contains")]
        operator: String,
        #[arg(short, long)]
        value: String,
        #[arg(long)]
        demo: bool,
        #[arg(long)]
        json: bool,
    },
    /// List objects in a database
    Objects {
        #[command(flatten)]
        conn: ConnectArgs,
        #[arg(long)]
        demo: bool,
        #[arg(long)]
        json: bool,
    },
    /// Describe columns (schema.name)
    Describe {
        #[command(flatten)]
        conn: ConnectArgs,
        object: String,
        #[arg(long)]
        json: bool,
    },
    /// OBJECT_DEFINITION for a module
    Script {
        #[command(flatten)]
        conn: ConnectArgs,
        object: String,
    },
    /// Read-only query
    Query {
        #[command(flatten)]
        conn: ConnectArgs,
        sql: Option<String>,
        #[arg(long)]
        file: Option<PathBuf>,
        #[arg(long, default_value_t = 100)]
        max_rows: usize,
        #[arg(long)]
        json: bool,
    },
    /// Print FilterOperator list
    Operators,
}

fn parse_operator(s: &str) -> FilterOperator {
    FilterOperator::ALL
        .into_iter()
        .find(|o| o.as_str().eq_ignore_ascii_case(s))
        .unwrap_or(FilterOperator::Contains)
}

fn parse_schema_name(object: &str) -> anyhow::Result<(String, String)> {
    let object = object.trim();
    if let Some((s, n)) = object.split_once('.') {
        if !s.is_empty() && !n.is_empty() {
            return Ok((s.to_string(), n.to_string()));
        }
    }
    Ok(("dbo".into(), object.to_string()))
}

fn print_catalog_hits(hits: &[&ssms_search_rs::CatalogObject], json: bool) {
    if json {
        let v: Vec<_> = hits
            .iter()
            .map(|o| {
                serde_json::json!({
                    "kind": o.kind.as_str(),
                    "schema": o.schema,
                    "name": o.name,
                    "owner": o.owner,
                    "urn": o.urn,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&v).unwrap());
    } else {
        for o in hits {
            println!(
                "{:<18} {:<32} {}",
                o.kind.as_str(),
                if o.schema.is_empty() {
                    o.name.clone()
                } else {
                    format!("{}.{}", o.schema, o.name)
                },
                o.urn
            );
        }
    }
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command.unwrap_or(Commands::Gui) {
        // GUI must NOT run under #[tokio::main]: Connect/Execute use their own
        // runtime via block_on; nested runtimes panic and close the window.
        Commands::Gui => {
            gui::run_gui().map_err(|e| anyhow::anyhow!("GUI error: {e}"))?;
            Ok(())
        }
        Commands::Operators => {
            for op in FilterOperator::ALL {
                println!("{}", op.as_str());
            }
            Ok(())
        }
        cmd => {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .context("tokio runtime")?;
            rt.block_on(run_cli(cmd))
        }
    }
}

async fn run_cli(cmd: Commands) -> anyhow::Result<()> {
    match cmd {
        Commands::Gui | Commands::Operators => unreachable!("handled in main"),
        Commands::Connect { conn } => {
            let mut client = connect(&conn).await?;
            let (server, db, login) = ping(&mut client).await?;
            println!("OK  server={server}  database={db}  login={login}");
        }
        Commands::Databases { conn, json } => {
            let mut client = connect(&conn).await?;
            let dbs = functions::list_databases(&mut client).await?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(
                        &dbs.iter()
                            .map(|d| {
                                serde_json::json!({
                                    "name": d.name,
                                    "database_id": d.database_id,
                                    "state_desc": d.state_desc,
                                })
                            })
                            .collect::<Vec<_>>()
                    )?
                );
            } else {
                for d in dbs {
                    println!("{:<8} {:<40} {}", d.database_id, d.name, d.state_desc);
                }
            }
        }
        Commands::Objects { conn, demo, json } => {
            let catalog = load_catalog_or_demo(&conn, demo).await?;
            let hits: Vec<_> = catalog
                .objects
                .iter()
                .filter(|o| {
                    !matches!(
                        o.kind,
                        ssms_search_rs::ObjectKind::Server
                            | ssms_search_rs::ObjectKind::Database
                            | ssms_search_rs::ObjectKind::Folder
                    )
                })
                .collect();
            print_catalog_hits(&hits, json);
        }
        Commands::Search {
            conn,
            column,
            operator,
            value,
            demo,
            json,
        } => {
            let catalog = load_catalog_or_demo(&conn, demo).await?;
            let conditions = SearchConditions {
                items: vec![FilterItem::new(column, parse_operator(&operator), value)],
            };
            let hits = catalog.search(&conditions);
            print_catalog_hits(&hits, json);
        }
        Commands::Describe { conn, object, json } => {
            let (schema, name) = parse_schema_name(&object)?;
            let mut client = connect(&conn).await?;
            let cols =
                functions::describe_object(&mut client, &conn.database, &schema, &name).await?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(
                        &cols
                            .iter()
                            .map(|c| {
                                serde_json::json!({
                                    "name": c.name,
                                    "data_type": c.data_type,
                                    "max_length": c.max_length,
                                    "is_nullable": c.is_nullable,
                                })
                            })
                            .collect::<Vec<_>>()
                    )?
                );
            } else {
                println!("{:<32} {:<20} {:>8} NULL", "COLUMN", "TYPE", "LEN");
                for c in cols {
                    println!(
                        "{:<32} {:<20} {:>8} {}",
                        c.name,
                        c.data_type,
                        c.max_length,
                        if c.is_nullable { "YES" } else { "NO" }
                    );
                }
            }
        }
        Commands::Script { conn, object } => {
            let (schema, name) = parse_schema_name(&object)?;
            let mut client = connect(&conn).await?;
            let def =
                functions::object_definition(&mut client, &conn.database, &schema, &name).await?;
            println!("{def}");
        }
        Commands::Query {
            conn,
            sql,
            file,
            max_rows,
            json,
        } => {
            let sql_text = if let Some(path) = file {
                std::fs::read_to_string(&path)?
            } else {
                sql.context("provide SQL text or --file")?
            };
            let mut client = connect(&conn).await?;
            let result = functions::run_query(&mut client, &sql_text, max_rows).await?;
            if json {
                let rows: Vec<serde_json::Value> = result
                    .rows
                    .iter()
                    .map(|r| {
                        let map: serde_json::Map<String, serde_json::Value> = result
                            .columns
                            .iter()
                            .zip(r.iter())
                            .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                            .collect();
                        serde_json::Value::Object(map)
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&rows)?);
            } else {
                if !result.columns.is_empty() {
                    println!("{}", result.columns.join(" | "));
                    println!("{}", "-".repeat(result.columns.join(" | ").len().min(120)));
                }
                for row in result.rows {
                    println!("{}", row.join(" | "));
                }
            }
        }
    }
    Ok(())
}

async fn load_catalog_or_demo(conn: &ConnectArgs, demo: bool) -> anyhow::Result<Catalog> {
    if demo {
        return Ok(Catalog::demo());
    }
    let mut client = connect(conn).await?;
    let (server, _, _) = ping(&mut client).await?;
    functions::load_catalog(&mut client, &server, &conn.database).await
}
