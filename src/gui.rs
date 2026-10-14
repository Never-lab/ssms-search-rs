//! Desktop UI (egui) — SSMS-inspired Object Explorer + Query / Execution Plan.

use anyhow::Context;
use eframe::egui::{
    self, Color32, CornerRadius, Frame, Key, Margin, RichText, Sense, Stroke, Vec2,
};
use ssms_search_rs::connection::{connect, ping, ConnectArgs};
use ssms_search_rs::filter::{FilterItem, FilterOperator, SearchConditions};
use ssms_search_rs::functions::{self, ColumnInfo, PlanMode, QueryExecution, QueryResult};
use ssms_search_rs::plan::PlanOperator;
use ssms_search_rs::{Catalog, CatalogObject, ObjectKind};
use std::collections::HashSet;

mod ssms_colors {
    use eframe::egui::Color32;
    pub const BG: Color32 = Color32::from_rgb(245, 245, 245);
    pub const PANEL: Color32 = Color32::from_rgb(255, 255, 255);
    pub const CHROME: Color32 = Color32::from_rgb(238, 238, 242);
    pub const BORDER: Color32 = Color32::from_rgb(204, 206, 219);
    pub const ACCENT: Color32 = Color32::from_rgb(0, 122, 204);
    pub const TEXT: Color32 = Color32::from_rgb(30, 30, 30);
    pub const MUTED: Color32 = Color32::from_rgb(97, 97, 97);
    pub const SEL: Color32 = Color32::from_rgb(204, 232, 255);
    pub const STATUS: Color32 = Color32::from_rgb(0, 122, 204);
    pub const TITLE: Color32 = Color32::from_rgb(0, 70, 140);
    pub const EXEC: Color32 = Color32::from_rgb(16, 124, 16);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Workspace {
    Explorer,
    Query,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ExplorerTab {
    ObjectExplorerDetails,
    Properties,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum QueryBottomTab {
    Results,
    Messages,
    ExecutionPlan,
}

pub struct SsmsApp {
    catalog: Catalog,
    expanded: HashSet<u64>,
    loaded_folders: HashSet<u64>,
    loaded_databases: HashSet<u64>,
    /// Columns from "Describe columns…" for the object id.
    described: Option<(u64, Vec<ColumnInfo>)>,
    selected_id: Option<u64>,
    workspace: Workspace,
    explorer_tab: ExplorerTab,
    conditions: SearchConditions,
    filter_draft: FilterItem,
    show_filter: bool,
    show_connect: bool,
    conn_server: String,
    conn_port: String,
    conn_database: String,
    conn_user: String,
    conn_password: String,
    connection_label: String,
    is_demo: bool,
    connected: bool,
    status: String,
    status_error: bool,
    object_hits: Vec<u64>,
    // Query workspace
    sql_text: String,
    include_actual_plan: bool,
    query_bottom: QueryBottomTab,
    query_result: QueryResult,
    query_messages: Vec<String>,
    plan_operators: Vec<PlanOperator>,
    plan_xml: Option<String>,
    query_tab_title: String,
}

impl SsmsApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let catalog = Catalog::demo();
        let mut expanded = HashSet::new();
        if let Some(root) = catalog.children(None).first() {
            expanded.insert(root.id);
        }
        let mut app = Self {
            catalog,
            expanded,
            loaded_folders: HashSet::new(),
            loaded_databases: HashSet::new(),
            described: None,
            selected_id: None,
            workspace: Workspace::Explorer,
            explorer_tab: ExplorerTab::ObjectExplorerDetails,
            conditions: SearchConditions::default(),
            filter_draft: FilterItem::new("Name", FilterOperator::Contains, ""),
            show_filter: false,
            show_connect: true,
            conn_server: "localhost".into(),
            conn_port: "1433".into(),
            conn_database: "master".into(),
            conn_user: String::new(),
            conn_password: String::new(),
            connection_label: "Demo catalog".into(),
            is_demo: true,
            connected: false,
            status: "Ready — connect or use demo, then New Query".into(),
            status_error: false,
            object_hits: Vec::new(),
            sql_text: "SELECT TOP (100) name, database_id, create_date\nFROM sys.databases\nORDER BY name;\n".into(),
            include_actual_plan: true,
            query_bottom: QueryBottomTab::Results,
            query_result: QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
            },
            query_messages: Vec::new(),
            plan_operators: Vec::new(),
            plan_xml: None,
            query_tab_title: "SQLQuery1.sql".into(),
        };
        app.refresh_object_hits();
        app
    }

    fn connect_args(&self) -> ConnectArgs {
        let port: u16 = self.conn_port.parse().unwrap_or(1433);
        ConnectArgs {
            server: self.conn_server.clone(),
            port,
            database: self.conn_database.clone(),
            user: Some(self.conn_user.clone()).filter(|s| !s.is_empty()),
            password: Some(self.conn_password.clone()),
            trust_cert: true,
            app_name: "ssms-search-rs".into(),
        }
    }

    fn refresh_object_hits(&mut self) {
        self.object_hits = self
            .catalog
            .search(&self.conditions)
            .into_iter()
            .map(|o| o.id)
            .collect();
        if !self.status_error && self.workspace == Workspace::Explorer {
            let n = self.object_hits.len();
            self.status = if self.conditions.is_empty() {
                format!("{n} object(s)")
            } else {
                format!("{n} object(s) matching filter")
            };
        }
    }

    fn apply_filter(&mut self) {
        self.conditions.items = vec![self.filter_draft.clone()];
        self.show_filter = false;
        self.workspace = Workspace::Explorer;
        self.explorer_tab = ExplorerTab::ObjectExplorerDetails;
        self.status_error = false;
        self.refresh_object_hits();
    }

    fn clear_filter(&mut self) {
        self.conditions.clear();
        self.filter_draft.value.clear();
        self.status_error = false;
        self.refresh_object_hits();
        self.status = "Filter cleared".into();
    }

    fn new_query(&mut self) {
        self.workspace = Workspace::Query;
        self.query_bottom = QueryBottomTab::Results;
        self.status_error = false;
        self.status = format!("Query editor — {}", self.query_tab_title);
        if self.sql_text.trim().is_empty() {
            self.sql_text =
                "SELECT TOP (100) *\nFROM sys.objects\nWHERE is_ms_shipped = 0;\n".into();
        }
    }

    fn load_demo(&mut self) {
        self.catalog = Catalog::demo();
        self.expanded.clear();
        self.loaded_folders.clear();
        self.loaded_databases.clear();
        self.described = None;
        if let Some(root) = self.catalog.children(None).first() {
            self.expanded.insert(root.id);
        }
        self.selected_id = None;
        self.is_demo = true;
        self.connected = false;
        self.connection_label = "Demo catalog".into();
        self.status_error = false;
        self.status = "Loaded demo catalog".into();
        self.show_connect = false;
        self.refresh_object_hits();
    }

    /// Run async SQL work on a dedicated OS thread + owned Tokio runtime.
    /// Avoids "Cannot start a runtime from within a runtime" panics that close the window.
    fn sql_job<T, F>(&self, job: F) -> Result<T, String>
    where
        T: Send + 'static,
        F: FnOnce() -> anyhow::Result<T> + Send + 'static,
    {
        let handle = std::thread::Builder::new()
            .name("ssms-sql".into())
            .spawn(move || std::panic::catch_unwind(std::panic::AssertUnwindSafe(job)))
            .map_err(|e| format!("spawn sql thread: {e}"))?;
        match handle
            .join()
            .map_err(|_| "SQL worker thread join failed".to_string())?
        {
            Ok(Ok(v)) => Ok(v),
            Ok(Err(e)) => Err(format!("{e:#}")),
            Err(panic) => {
                let msg = panic
                    .downcast_ref::<String>()
                    .map(|s| s.as_str())
                    .or_else(|| panic.downcast_ref::<&str>().copied())
                    .unwrap_or("unknown panic");
                Err(format!("SQL worker panicked: {msg}"))
            }
        }
    }

    /// Connect (or `refresh` an existing session: same args, keep old tree on failure).
    fn try_connect(&mut self, refresh: bool) {
        let args = self.connect_args();
        self.status_error = false;
        self.status = if refresh {
            "Refreshing…"
        } else {
            "Connecting…"
        }
        .into();

        let result = self.sql_job(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .context("tokio")?;
            rt.block_on(async {
                let mut client = connect(&args).await?;
                let (server, db, login) = ping(&mut client).await?;
                let catalog =
                    functions::load_server_catalog(&mut client, &server, &args.database).await?;
                Ok::<_, anyhow::Error>((catalog, server, db, login))
            })
        });

        match result {
            Ok((catalog, server, db, login)) => {
                self.catalog = catalog;
                self.expanded.clear();
                self.loaded_folders.clear();
                self.loaded_databases.clear();
                self.described = None;
                if let Some(root) = self.catalog.children(None).first() {
                    self.expanded.insert(root.id);
                    // The preferred database arrives fully populated: expand it.
                    let preloaded: Vec<u64> = self
                        .catalog
                        .children(Some(root.id))
                        .into_iter()
                        .filter(|d| !self.catalog.children(Some(d.id)).is_empty())
                        .map(|d| d.id)
                        .collect();
                    self.expanded.extend(&preloaded);
                    self.loaded_databases.extend(preloaded);
                }
                self.selected_id = None;
                self.is_demo = false;
                self.connected = true;
                self.connection_label = format!("{server} · {db} · {login}");
                self.status_error = false;
                self.status = if refresh { "Refreshed" } else { "Connected" }.into();
                self.show_connect = false;
                self.refresh_object_hits();
            }
            Err(e) => {
                self.connected = refresh && self.connected;
                self.status_error = true;
                self.status = format!(
                    "{} failed: {e}",
                    if refresh { "Refresh" } else { "Connect" }
                );
            }
        }
    }

    fn execute_query(&mut self, mode: PlanMode) {
        if !self.connected {
            self.status_error = true;
            self.status = "Connect to a SQL Server first (File → Connect to Server…)".into();
            self.show_connect = true;
            return;
        }
        let sql = self.sql_text.clone();
        let args = self.connect_args();
        self.status = "Executing…".into();
        self.status_error = false;

        let result = self.sql_job(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .context("tokio")?;
            rt.block_on(async {
                let mut client = connect(&args).await?;
                functions::run_query_ex(&mut client, &sql, 500, mode).await
            })
        });

        match result {
            Ok(exec) => self.apply_execution(exec),
            Err(e) => {
                self.status_error = true;
                self.status = format!("Execute failed: {e}");
                self.query_messages = vec![format!("Error: {e}")];
                self.query_bottom = QueryBottomTab::Messages;
                self.workspace = Workspace::Query;
            }
        }
    }

    fn apply_execution(&mut self, exec: QueryExecution) {
        self.query_result = exec.result;
        self.query_messages = exec.messages;
        self.plan_operators = exec.plan_operators;
        self.plan_xml = exec.plan_xml;
        self.workspace = Workspace::Query;
        self.status_error = false;
        self.status = format!(
            "Query executed in {} ms — {} row(s)",
            exec.elapsed_ms,
            self.query_result.rows.len()
        );
        self.query_bottom = if !self.plan_operators.is_empty()
            && (self.include_actual_plan || self.query_result.rows.is_empty())
        {
            QueryBottomTab::ExecutionPlan
        } else if self.query_result.rows.is_empty() {
            QueryBottomTab::Messages
        } else {
            QueryBottomTab::Results
        };
    }

    fn selected_object(&self) -> Option<&CatalogObject> {
        self.selected_id.and_then(|id| self.catalog.by_id(id))
    }

    fn apply_visuals(ctx: &egui::Context) {
        let mut visuals = egui::Visuals::light();
        visuals.panel_fill = ssms_colors::PANEL;
        visuals.window_fill = ssms_colors::PANEL;
        visuals.faint_bg_color = ssms_colors::CHROME;
        visuals.widgets.noninteractive.bg_fill = ssms_colors::CHROME;
        visuals.widgets.inactive.bg_fill = ssms_colors::CHROME;
        visuals.widgets.hovered.bg_fill = ssms_colors::SEL;
        visuals.selection.bg_fill = ssms_colors::SEL;
        visuals.selection.stroke = Stroke::new(1.0_f32, ssms_colors::ACCENT);
        visuals.widgets.active.bg_fill = ssms_colors::SEL;
        visuals.override_text_color = Some(ssms_colors::TEXT);
        ctx.set_visuals(visuals);
        let mut style = (*ctx.style()).clone();
        style.spacing.item_spacing = Vec2::new(6.0, 4.0);
        style.spacing.window_margin = Margin::same(8);
        ctx.set_style(style);
    }

    fn draw_menu(&mut self, ui: &mut egui::Ui) {
        egui::menu::bar(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("Connect to Server…").clicked() {
                    self.show_connect = true;
                    ui.close_menu();
                }
                if ui
                    .add_enabled(self.connected, egui::Button::new("Refresh"))
                    .clicked()
                {
                    self.try_connect(true);
                    ui.close_menu();
                }
                if ui.button("Load Demo Catalog").clicked() {
                    self.load_demo();
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("New Query").clicked() {
                    self.new_query();
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Exit").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("Query", |ui| {
                if ui.button("Execute (F5)").clicked() {
                    let mode = if self.include_actual_plan {
                        PlanMode::Actual
                    } else {
                        PlanMode::None
                    };
                    self.execute_query(mode);
                    ui.close_menu();
                }
                if ui.button("Display Estimated Execution Plan").clicked() {
                    self.execute_query(PlanMode::Estimated);
                    ui.close_menu();
                }
                ui.checkbox(
                    &mut self.include_actual_plan,
                    "Include Actual Execution Plan",
                );
            });
            ui.menu_button("View", |ui| {
                if ui.button("Object Explorer").clicked() {
                    self.workspace = Workspace::Explorer;
                    ui.close_menu();
                }
                if ui.button("Filter Settings…").clicked() {
                    self.show_filter = true;
                    ui.close_menu();
                }
                if ui.button("Clear Filter").clicked() {
                    self.clear_filter();
                    ui.close_menu();
                }
            });
            ui.menu_button("Help", |ui| {
                ui.label("ssms-search-rs 0.5 — Query + Execution Plan");
            });

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mode = if self.is_demo { "Demo" } else { "Live" };
                ui.label(
                    RichText::new(format!("{mode}  ·  {}", self.connection_label))
                        .color(ssms_colors::MUTED)
                        .small(),
                );
            });
        });
    }

    fn draw_toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui
                .add(egui::Button::new(
                    RichText::new("⚡ Connect").color(ssms_colors::ACCENT),
                ))
                .clicked()
            {
                self.show_connect = true;
            }
            if ui
                .add_enabled(self.connected, egui::Button::new("🔄 Refresh"))
                .on_hover_text("Reload databases and objects (no reconnect dialog)")
                .clicked()
            {
                self.try_connect(true);
            }
            ui.separator();
            if ui
                .add(egui::Button::new(RichText::new("🗒 New Query").strong()))
                .on_hover_text("Open SQL editor")
                .clicked()
            {
                self.new_query();
            }
            let exec_btn = ui.add(
                egui::Button::new(RichText::new("▶ Execute").color(Color32::WHITE))
                    .fill(ssms_colors::EXEC),
            );
            if exec_btn.on_hover_text("Execute (F5)").clicked() {
                let mode = if self.include_actual_plan {
                    PlanMode::Actual
                } else {
                    PlanMode::None
                };
                self.execute_query(mode);
            }
            if ui
                .button("📈 Estimated Plan")
                .on_hover_text("SET SHOWPLAN_XML — does not run the query")
                .clicked()
            {
                self.execute_query(PlanMode::Estimated);
            }
            ui.checkbox(&mut self.include_actual_plan, "Include Actual Plan");
            ui.separator();
            if ui
                .add(egui::Button::new(
                    RichText::new("🔍 Filter").color(ssms_colors::TITLE),
                ))
                .clicked()
            {
                self.show_filter = true;
            }
            if ui.button("Object Explorer").clicked() {
                self.workspace = Workspace::Explorer;
            }
        });
    }

    fn draw_object_explorer(&mut self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new("Object Explorer")
                .color(ssms_colors::TITLE)
                .strong(),
        );
        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            let roots: Vec<u64> = self
                .catalog
                .children(None)
                .into_iter()
                .map(|o| o.id)
                .collect();
            for id in roots {
                self.draw_tree_node(ui, id, 0);
            }
        });
    }

    fn toggle_node(&mut self, id: u64, open: bool) {
        if open {
            self.expanded.remove(&id);
        } else {
            self.expanded.insert(id);
            self.load_database_children(id);
            self.load_folder_children(id);
        }
    }

    /// Nearest Database ancestor (or self) of an object.
    fn database_of(&self, id: u64) -> Option<&CatalogObject> {
        let mut cur = self.catalog.by_id(id);
        while let Some(o) = cur {
            if o.kind == ObjectKind::Database {
                return Some(o);
            }
            cur = o.parent_id.and_then(|p| self.catalog.by_id(p));
        }
        None
    }

    fn server_name(&self) -> Option<String> {
        self.catalog
            .objects
            .iter()
            .find(|o| o.kind == ObjectKind::Server)
            .map(|o| o.name.clone())
    }

    /// Populate a live Database node the first time it is expanded.
    fn load_database_children(&mut self, db_id: u64) {
        if self.is_demo || !self.connected || self.loaded_databases.contains(&db_id) {
            return;
        }
        let Some(db) = self
            .catalog
            .by_id(db_id)
            .filter(|o| o.kind == ObjectKind::Database)
        else {
            return;
        };
        let database = db.name.clone();
        let Some(server) = self.server_name() else {
            return;
        };
        if !self.catalog.children(Some(db_id)).is_empty() {
            self.loaded_databases.insert(db_id);
            return;
        }
        let args = self.connect_args();
        let mut catalog = self.catalog.clone();
        self.status_error = false;
        self.status = format!("Loading database {database}…");

        let result = self.sql_job(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .context("tokio")?;
            rt.block_on(async {
                let mut client = connect(&args).await?;
                functions::ensure_database_loaded(
                    &mut client,
                    &server,
                    &database,
                    &mut catalog,
                    db_id,
                )
                .await?;
                Ok::<_, anyhow::Error>((catalog, database))
            })
        });

        match result {
            Ok((catalog, database)) => {
                self.catalog = catalog;
                self.loaded_databases.insert(db_id);
                self.status = format!("Loaded database {database}");
                self.refresh_object_hits();
            }
            Err(e) => {
                self.status_error = true;
                self.status = format!("Load database failed: {e}");
            }
        }
    }

    /// Properties → "Describe columns…" (Table/View).
    fn describe_selected(&mut self) {
        let Some(o) = self.selected_object().cloned() else {
            return;
        };
        let Some(database) = self.database_of(o.id).map(|d| d.name.clone()) else {
            return;
        };
        let args = self.connect_args();
        let (schema, name) = (o.schema.clone(), o.name.clone());
        self.status_error = false;
        self.status = format!("Describing {schema}.{name}…");
        let result = self.sql_job(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .context("tokio")?;
            rt.block_on(async {
                let mut client = connect(&args).await?;
                functions::describe_object(&mut client, &database, &schema, &name).await
            })
        });
        match result {
            Ok(cols) => {
                self.status = format!("{} column(s) for {}.{}", cols.len(), o.schema, o.name);
                self.described = Some((o.id, cols));
            }
            Err(e) => {
                self.status_error = true;
                self.status = format!("Describe failed: {e}");
            }
        }
    }

    /// Properties → "Script definition…" (Procedure/Function/View/Trigger) into the query editor.
    fn script_selected(&mut self) {
        let Some(o) = self.selected_object().cloned() else {
            return;
        };
        let Some(database) = self.database_of(o.id).map(|d| d.name.clone()) else {
            return;
        };
        let args = self.connect_args();
        let (schema, name) = (o.schema.clone(), o.name.clone());
        self.status_error = false;
        self.status = format!("Scripting {schema}.{name}…");
        let result = self.sql_job(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .context("tokio")?;
            rt.block_on(async {
                let mut client = connect(&args).await?;
                functions::object_definition(&mut client, &database, &schema, &name).await
            })
        });
        match result {
            Ok(sql) => {
                self.sql_text = sql;
                self.new_query();
                self.status = format!("Scripted {}.{}", o.schema, o.name);
            }
            Err(e) => {
                self.status_error = true;
                self.status = format!("Script failed: {e}");
            }
        }
    }

    /// Lazy-load Columns/Indexes for a live table when its folder is first opened.
    fn load_folder_children(&mut self, folder_id: u64) {
        if self.is_demo || !self.connected || self.loaded_folders.contains(&folder_id) {
            return;
        }
        let Some(folder) = self.catalog.by_id(folder_id) else {
            return;
        };
        if folder.kind != ObjectKind::Folder
            || !matches!(folder.name.as_str(), "Columns" | "Indexes")
        {
            return;
        }
        if !self.catalog.children(Some(folder_id)).is_empty() {
            self.loaded_folders.insert(folder_id);
            return;
        }
        let Some(table) = folder
            .parent_id
            .and_then(|p| self.catalog.by_id(p))
            .filter(|t| t.kind == ObjectKind::Table)
        else {
            return;
        };
        let find = |name: &str| {
            self.catalog
                .children(Some(table.id))
                .into_iter()
                .find(|o| o.kind == ObjectKind::Folder && o.name == name)
                .map(|o| o.id)
        };
        let (Some(cols_id), Some(idx_id)) = (find("Columns"), find("Indexes")) else {
            return;
        };
        let Some(server) = self
            .catalog
            .objects
            .iter()
            .find(|o| o.kind == ObjectKind::Server)
            .map(|o| o.name.clone())
        else {
            return;
        };
        let Some(database) = self.database_of(table.id).map(|o| o.name.clone()) else {
            return;
        };
        let (schema, table_name) = (table.schema.clone(), table.name.clone());
        let start_id = self.catalog.objects.iter().map(|o| o.id).max().unwrap_or(0) + 1;
        let args = self.connect_args();
        self.status_error = false;
        self.status = format!("Loading {schema}.{table_name} columns and indexes…");

        let result = self.sql_job(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .context("tokio")?;
            rt.block_on(async {
                let mut client = connect(&args).await?;
                functions::load_table_children(
                    &mut client,
                    &database,
                    &server,
                    &schema,
                    &table_name,
                    cols_id,
                    idx_id,
                    start_id,
                )
                .await
            })
        });

        match result {
            Ok(new) => {
                self.status = format!("Loaded {} column/index item(s)", new.len());
                self.catalog.objects.extend(new);
                self.loaded_folders.insert(cols_id);
                self.loaded_folders.insert(idx_id);
                self.refresh_object_hits();
            }
            Err(e) => {
                self.status_error = true;
                self.status = format!("Load table children failed: {e}");
            }
        }
    }

    fn draw_tree_node(&mut self, ui: &mut egui::Ui, id: u64, depth: usize) {
        let (name, expandable, kind) = {
            let Some(obj) = self.catalog.by_id(id) else {
                return;
            };
            let kind = obj.kind;
            let expandable = matches!(
                kind,
                ObjectKind::Server
                    | ObjectKind::Database
                    | ObjectKind::Folder
                    | ObjectKind::Schema
                    | ObjectKind::Table
            );
            let name = if matches!(kind, ObjectKind::Column | ObjectKind::Index)
                || obj.schema.is_empty()
            {
                format!("{} {}", kind_glyph(kind), obj.name)
            } else {
                format!("{} {}.{}", kind_glyph(kind), obj.schema, obj.name)
            };
            (name, expandable, kind)
        };

        let selected = self.selected_id == Some(id);
        let open = self.expanded.contains(&id);

        ui.horizontal(|ui| {
            ui.add_space((depth as f32) * 14.0);
            if expandable {
                let twisty = if open { "▼" } else { "▶" };
                if ui.add(egui::Button::new(twisty).frame(false)).clicked() {
                    self.toggle_node(id, open);
                }
            } else {
                ui.add_space(18.0);
            }

            let text = if selected {
                RichText::new(name).strong()
            } else {
                RichText::new(name)
            };
            let resp = ui.add(
                egui::Button::new(text)
                    .fill(if selected {
                        ssms_colors::SEL
                    } else {
                        Color32::TRANSPARENT
                    })
                    .frame(selected),
            );
            if resp.clicked() {
                self.selected_id = Some(id);
                if !expandable || kind == ObjectKind::Table {
                    self.workspace = Workspace::Explorer;
                    self.explorer_tab = ExplorerTab::Properties;
                }
            }
            if resp.double_clicked() && expandable {
                self.toggle_node(id, open);
            }
        });

        if expandable && self.expanded.contains(&id) {
            let children: Vec<u64> = self
                .catalog
                .children(Some(id))
                .into_iter()
                .map(|o| o.id)
                .collect();
            for child in children {
                self.draw_tree_node(ui, child, depth + 1);
            }
        }
    }

    fn draw_explorer_workspace(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(
                &mut self.explorer_tab,
                ExplorerTab::ObjectExplorerDetails,
                "Object Explorer Details",
            );
            ui.selectable_value(
                &mut self.explorer_tab,
                ExplorerTab::Properties,
                "Properties",
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("New Query").clicked() {
                    self.new_query();
                }
            });
        });
        ui.separator();
        match self.explorer_tab {
            ExplorerTab::ObjectExplorerDetails => self.draw_object_hits(ui),
            ExplorerTab::Properties => self.draw_properties(ui),
        }
    }

    fn draw_object_hits(&mut self, ui: &mut egui::Ui) {
        let filter = if self.conditions.is_empty() {
            "No filter".to_string()
        } else {
            format!(
                "{} {} '{}'",
                self.filter_draft.column, self.filter_draft.operator, self.filter_draft.value
            )
        };
        ui.label(
            RichText::new(format!(
                "Results ({})  ·  Filter: {}",
                self.object_hits.len(),
                filter
            ))
            .color(ssms_colors::MUTED)
            .small(),
        );

        egui::ScrollArea::both().show(ui, |ui| {
            egui::Grid::new("results_grid")
                .striped(true)
                .num_columns(4)
                .min_col_width(80.0)
                .show(ui, |ui| {
                    ui.strong("Type");
                    ui.strong("Name");
                    ui.strong("Schema");
                    ui.strong("Owner");
                    ui.end_row();

                    let rows: Vec<(u64, String, String, String, String)> = self
                        .object_hits
                        .iter()
                        .filter_map(|id| {
                            let o = self.catalog.by_id(*id)?;
                            let name = if o.schema.is_empty() {
                                o.name.clone()
                            } else {
                                format!("{}.{}", o.schema, o.name)
                            };
                            Some((
                                *id,
                                format!("{} {}", kind_glyph(o.kind), short_type(o.kind)),
                                name,
                                o.schema.clone(),
                                o.owner.clone(),
                            ))
                        })
                        .collect();

                    for (id, ty, name, schema, owner) in rows {
                        let selected = self.selected_id == Some(id);
                        for cell in [ty, name, schema, owner] {
                            let resp = ui.add(
                                egui::Label::new(RichText::new(cell).color(ssms_colors::TEXT))
                                    .sense(Sense::click()),
                            );
                            if selected {
                                ui.painter().rect_filled(
                                    resp.rect.expand(2.0),
                                    CornerRadius::ZERO,
                                    ssms_colors::SEL,
                                );
                            }
                            if resp.clicked() {
                                self.selected_id = Some(id);
                            }
                            if resp.double_clicked() {
                                self.selected_id = Some(id);
                                self.explorer_tab = ExplorerTab::Properties;
                            }
                        }
                        ui.end_row();
                    }
                });
        });
    }

    fn draw_properties(&mut self, ui: &mut egui::Ui) {
        if let Some(o) = self.selected_object().cloned() {
            let mut rows = o.props().to_vec();
            rows.push(("URN", o.urn.as_str()));
            egui::Grid::new("props_grid")
                .num_columns(2)
                .spacing([12.0, 6.0])
                .show(ui, |ui| {
                    for (k, v) in rows {
                        ui.label(RichText::new(k).color(ssms_colors::MUTED).strong());
                        ui.label(v);
                        ui.end_row();
                    }
                });
            ui.add_space(8.0);
            let table_like = matches!(o.kind, ObjectKind::Table | ObjectKind::View);
            let module = matches!(
                o.kind,
                ObjectKind::Procedure
                    | ObjectKind::Function
                    | ObjectKind::View
                    | ObjectKind::Trigger
            );
            ui.horizontal(|ui| {
                if table_like && ui.button("Script as SELECT TOP 100…").clicked() {
                    self.sql_text =
                        format!("SELECT TOP (100) *\nFROM [{}].[{}];\n", o.schema, o.name);
                    self.new_query();
                }
                if table_like
                    && ui
                        .add_enabled(self.connected, egui::Button::new("Describe columns…"))
                        .on_disabled_hover_text("Connect to a SQL Server first")
                        .clicked()
                {
                    self.describe_selected();
                }
                if module
                    && ui
                        .add_enabled(self.connected, egui::Button::new("Script definition…"))
                        .on_disabled_hover_text("Connect to a SQL Server first")
                        .clicked()
                {
                    self.script_selected();
                }
            });
            if let Some((_, cols)) = self.described.as_ref().filter(|(id, _)| *id == o.id) {
                ui.add_space(8.0);
                ui.label(RichText::new("Columns").color(ssms_colors::TITLE).strong());
                egui::Grid::new("describe_grid")
                    .striped(true)
                    .num_columns(4)
                    .spacing([12.0, 4.0])
                    .show(ui, |ui| {
                        for h in ["Name", "Type", "Max length", "Nullable"] {
                            ui.strong(h);
                        }
                        ui.end_row();
                        for c in cols {
                            ui.label(&c.name);
                            ui.label(&c.data_type);
                            ui.label(c.max_length.to_string());
                            ui.label(if c.is_nullable { "yes" } else { "no" });
                            ui.end_row();
                        }
                    });
            }
        } else {
            ui.label(
                RichText::new("Select an object in Object Explorer.")
                    .color(ssms_colors::MUTED)
                    .italics(),
            );
        }
    }

    fn draw_query_workspace(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!(" 🗒 {} ", self.query_tab_title))
                    .color(ssms_colors::TITLE)
                    .strong(),
            );
            ui.separator();
            ui.label(
                RichText::new("F5 Execute · Estimated Plan · Include Actual Plan")
                    .color(ssms_colors::MUTED)
                    .small(),
            );
        });
        ui.separator();

        let total = ui.available_height();
        let editor_h = (total * 0.45).clamp(140.0, 360.0);

        egui::ScrollArea::vertical()
            .id_salt("sql_editor_scroll")
            .max_height(editor_h)
            .show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut self.sql_text)
                        .code_editor()
                        .desired_width(f32::INFINITY)
                        .desired_rows(12)
                        .font(egui::TextStyle::Monospace),
                );
            });

        ui.separator();
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.query_bottom, QueryBottomTab::Results, "Results");
            ui.selectable_value(&mut self.query_bottom, QueryBottomTab::Messages, "Messages");
            ui.selectable_value(
                &mut self.query_bottom,
                QueryBottomTab::ExecutionPlan,
                "Execution Plan",
            );
        });
        ui.separator();

        match self.query_bottom {
            QueryBottomTab::Results => self.draw_query_results(ui),
            QueryBottomTab::Messages => self.draw_query_messages(ui),
            QueryBottomTab::ExecutionPlan => self.draw_execution_plan(ui),
        }
    }

    fn draw_query_results(&mut self, ui: &mut egui::Ui) {
        if self.query_result.columns.is_empty() && self.query_result.rows.is_empty() {
            ui.label(
                RichText::new("No result grid — run Execute, or open Execution Plan / Messages.")
                    .color(ssms_colors::MUTED)
                    .italics(),
            );
            return;
        }
        ui.label(
            RichText::new(format!("{} row(s)", self.query_result.rows.len()))
                .color(ssms_colors::MUTED)
                .small(),
        );
        let cols = self.query_result.columns.clone();
        let rows = self.query_result.rows.clone();
        egui::ScrollArea::both().show(ui, |ui| {
            egui::Grid::new("query_result_grid")
                .striped(true)
                .num_columns(cols.len().max(1))
                .min_col_width(72.0)
                .show(ui, |ui| {
                    for c in &cols {
                        ui.strong(c);
                    }
                    if !cols.is_empty() {
                        ui.end_row();
                    }
                    for row in &rows {
                        for cell in row {
                            ui.label(cell);
                        }
                        ui.end_row();
                    }
                });
        });
    }

    fn draw_query_messages(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            if self.query_messages.is_empty() {
                ui.label(
                    RichText::new("No messages yet.")
                        .color(ssms_colors::MUTED)
                        .italics(),
                );
            }
            for msg in &self.query_messages {
                ui.monospace(msg);
            }
        });
    }

    fn draw_execution_plan(&mut self, ui: &mut egui::Ui) {
        if self.plan_operators.is_empty() {
            ui.label(
                RichText::new(
                    "No plan captured. Enable “Include Actual Plan” and Execute, or use Estimated Plan.",
                )
                .color(ssms_colors::MUTED)
                .italics(),
            );
            return;
        }
        ui.label(
            RichText::new(format!(
                "{} operator(s) from ShowPlanXML",
                self.plan_operators.len()
            ))
            .color(ssms_colors::MUTED)
            .small(),
        );
        let ops = self.plan_operators.clone();
        egui::ScrollArea::both().show(ui, |ui| {
            egui::Grid::new("plan_grid")
                .striped(true)
                .num_columns(5)
                .min_col_width(70.0)
                .show(ui, |ui| {
                    ui.strong("NodeId");
                    ui.strong("Physical Op");
                    ui.strong("Logical Op");
                    ui.strong("Est. Rows");
                    ui.strong("Subtree Cost");
                    ui.end_row();
                    for op in &ops {
                        ui.label(&op.node_id);
                        ui.label(&op.physical_op);
                        ui.label(&op.logical_op);
                        ui.label(&op.estimate_rows);
                        ui.label(&op.estimate_cost);
                        ui.end_row();
                    }
                });
        });
        if let Some(xml) = &self.plan_xml {
            ui.add_space(6.0);
            ui.collapsing("Raw ShowPlanXML", |ui| {
                egui::ScrollArea::vertical()
                    .max_height(160.0)
                    .show(ui, |ui| {
                        ui.monospace(xml);
                    });
            });
        }
    }

    fn draw_status(&self, ui: &mut egui::Ui) {
        ui.label(RichText::new(&self.status).color(Color32::WHITE));
    }

    fn draw_connect_modal(&mut self, ctx: &egui::Context) {
        egui::Window::new("Connect to Server")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(
                    RichText::new("SQL Server Authentication")
                        .color(ssms_colors::TITLE)
                        .strong(),
                );
                ui.add_space(6.0);
                egui::Grid::new("conn_grid")
                    .num_columns(2)
                    .spacing([10.0, 6.0])
                    .show(ui, |ui| {
                        ui.label("Server name:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.conn_server).desired_width(220.0),
                        );
                        ui.end_row();
                        ui.label("Port:");
                        ui.add(egui::TextEdit::singleline(&mut self.conn_port).desired_width(80.0));
                        ui.end_row();
                        ui.label("Database:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.conn_database)
                                .desired_width(220.0),
                        );
                        ui.end_row();
                        ui.label("Login:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.conn_user).desired_width(220.0),
                        );
                        ui.end_row();
                        ui.label("Password:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.conn_password)
                                .password(true)
                                .desired_width(220.0),
                        );
                        ui.end_row();
                    });
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::Button::new(RichText::new("Connect").color(Color32::WHITE))
                                .fill(ssms_colors::ACCENT),
                        )
                        .clicked()
                    {
                        self.try_connect(false);
                    }
                    if ui.button("Cancel").clicked() {
                        self.show_connect = false;
                    }
                    if ui.button("Use Demo").clicked() {
                        self.load_demo();
                    }
                });
            });
    }

    fn draw_filter_modal(&mut self, ctx: &egui::Context) {
        egui::Window::new("Filter Settings")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(
                    RichText::new("Filter the list of objects shown under the selected node.")
                        .color(ssms_colors::MUTED),
                );
                ui.add_space(8.0);
                egui::Grid::new("filter_grid")
                    .num_columns(3)
                    .spacing([8.0, 6.0])
                    .show(ui, |ui| {
                        ui.strong("Property");
                        ui.strong("Operator");
                        ui.strong("Value");
                        ui.end_row();
                        egui::ComboBox::from_id_salt("filter_col")
                            .selected_text(&self.filter_draft.column)
                            .show_ui(ui, |ui| {
                                for c in ["Name", "Schema", "Owner", "Type", "Date"] {
                                    ui.selectable_value(
                                        &mut self.filter_draft.column,
                                        c.to_string(),
                                        c,
                                    );
                                }
                            });
                        let op_label = self.filter_draft.operator.as_str().to_string();
                        egui::ComboBox::from_id_salt("filter_op")
                            .selected_text(op_label)
                            .show_ui(ui, |ui| {
                                for op in FilterOperator::ALL {
                                    ui.selectable_value(
                                        &mut self.filter_draft.operator,
                                        op,
                                        op.as_str(),
                                    );
                                }
                            });
                        ui.add(
                            egui::TextEdit::singleline(&mut self.filter_draft.value)
                                .desired_width(180.0),
                        );
                        ui.end_row();
                    });
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::Button::new(RichText::new("OK").color(Color32::WHITE))
                                .fill(ssms_colors::ACCENT),
                        )
                        .clicked()
                    {
                        self.apply_filter();
                    }
                    if ui.button("Cancel").clicked() {
                        self.show_filter = false;
                    }
                    if ui.button("Clear All").clicked() {
                        self.clear_filter();
                        self.show_filter = false;
                    }
                });
            });
    }
}

impl eframe::App for SsmsApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        Self::apply_visuals(ctx);

        // F5 = Execute (SSMS)
        if ctx.input(|i| i.key_pressed(Key::F5)) {
            let mode = if self.include_actual_plan {
                PlanMode::Actual
            } else {
                PlanMode::None
            };
            self.workspace = Workspace::Query;
            self.execute_query(mode);
        }

        egui::TopBottomPanel::top("menu")
            .frame(
                Frame::new()
                    .fill(ssms_colors::CHROME)
                    .inner_margin(Margin::symmetric(6, 2)),
            )
            .show(ctx, |ui| {
                self.draw_menu(ui);
                ui.add_space(2.0);
                self.draw_toolbar(ui);
            });

        let status_fill = if self.status_error {
            Color32::from_rgb(180, 40, 40)
        } else {
            ssms_colors::STATUS
        };
        egui::TopBottomPanel::bottom("status")
            .exact_height(24.0)
            .frame(Frame::new().fill(status_fill))
            .show(ctx, |ui| {
                self.draw_status(ui);
            });

        egui::SidePanel::left("object_explorer")
            .default_width(300.0)
            .width_range(220.0..=480.0)
            .frame(
                Frame::new()
                    .fill(ssms_colors::PANEL)
                    .stroke(Stroke::new(1.0_f32, ssms_colors::BORDER))
                    .inner_margin(Margin::same(8)),
            )
            .show(ctx, |ui| {
                self.draw_object_explorer(ui);
            });

        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(ssms_colors::BG)
                    .inner_margin(Margin::same(8)),
            )
            .show(ctx, |ui| {
                Frame::new()
                    .fill(ssms_colors::PANEL)
                    .stroke(Stroke::new(1.0_f32, ssms_colors::BORDER))
                    .inner_margin(Margin::same(10))
                    .show(ui, |ui| match self.workspace {
                        Workspace::Explorer => self.draw_explorer_workspace(ui),
                        Workspace::Query => self.draw_query_workspace(ui),
                    });
            });

        if self.show_connect {
            self.draw_connect_modal(ctx);
        }
        if self.show_filter {
            self.draw_filter_modal(ctx);
        }
    }
}

fn kind_glyph(kind: ObjectKind) -> &'static str {
    match kind {
        ObjectKind::Server => "🖥",
        ObjectKind::Database => "🗄",
        ObjectKind::Schema => "📦",
        ObjectKind::Folder => "📁",
        ObjectKind::Table => "▦",
        ObjectKind::View => "▤",
        ObjectKind::Procedure => "⚙",
        ObjectKind::Function => "ƒ",
        ObjectKind::Column => "▤",
        ObjectKind::Index => "⌗",
        ObjectKind::Synonym => "⇄",
        ObjectKind::Trigger => "⚡",
    }
}

fn short_type(kind: ObjectKind) -> &'static str {
    match kind {
        ObjectKind::Table => "Table",
        ObjectKind::View => "View",
        ObjectKind::Procedure => "Proc",
        ObjectKind::Function => "Fn",
        ObjectKind::Schema => "Schema",
        ObjectKind::Database => "Database",
        ObjectKind::Server => "Server",
        ObjectKind::Folder => "Folder",
        ObjectKind::Column => "Column",
        ObjectKind::Index => "Index",
        ObjectKind::Synonym => "Synonym",
        ObjectKind::Trigger => "Trigger",
    }
}

pub fn run_gui() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1360.0, 860.0])
            .with_min_inner_size([960.0, 600.0])
            .with_title("SQL Server Management — ssms-search-rs"),
        ..Default::default()
    };
    eframe::run_native(
        "ssms-search-rs",
        options,
        Box::new(|cc| Ok(Box::new(SsmsApp::new(cc)))),
    )
}
