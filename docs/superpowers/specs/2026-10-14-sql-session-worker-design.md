# Persistent TDS session + non-blocking SQL worker — design

Date: 2026-10-14  
Status: approved (brainstorming)  
Repo: `ssms-search-rs`  
Issues: #3 (reuse TDS connection), #4 (non-blocking SQL worker)

## Goal

Stop reconnecting and freezing the egui UI on every SQL operation.

- **#3:** Keep one live `SqlClient` for the GUI session after Connect; reuse for Execute / Expand DB / Describe / Script / lazy catalog.
- **#4:** Never `join()` SQL work on the UI thread; status bar shows Working… then result/error; no nested-runtime panics.

## Decisions (from brainstorming)

| Topic | Choice |
|-------|--------|
| Architecture | **A** — dedicated worker actor thread |
| Stale session | Silent auto-reconnect once on next op |
| Concurrency | Single job in flight (FIFO of one); no queue while pending |
| Cancel | Out of scope (#9) |
| CLI | Unchanged — still connect-per-invocation |
| Pool | One client, not a pool |

## Architecture

```
egui update  --submit(Command)-->  SqlWorker OS thread  --try_recv(Result)-->  SsmsApp
                 (non-blocking)         |
                                        ├─ owns Tokio current_thread runtime
                                        ├─ owns Option<SqlClient>
                                        ├─ owns Option<ConnectArgs> (session)
                                        └─ block_on(job); reconnect if dead
```

### Module layout

- New `src/sql_worker.rs` as a **binary module** (`mod sql_worker;` next to `mod gui;` in `main.rs`) — not part of the public `ssms_search_rs` lib. Owns worker thread, command/result enums, reconnect helper.
- Channels: `std::sync::mpsc` (unbounded cmd + result). No extra crate.
- `src/gui.rs` — remove `sql_job`; submit + `try_recv` each frame; `pending` + `request_repaint`.
- Optional tiny helper in `src/connection.rs` (e.g. `is_connection_error`) for stale detection — only if it stays small; otherwise keep the matcher private in `sql_worker`.
- `functions.rs` / CLI unchanged.

### Lifecycle

1. App start → spawn worker (idle, no client).
2. Connect / Refresh → `Command::Connect { args, refresh }` → on success UI stores catalog / `session_args` semantics as today (refresh keeps old tree on failure).
3. Subsequent ops → commands that **require** an open session; worker reuses client.
4. Disconnect → `Command::Disconnect` → drop client + clear session on worker; UI resets as today.
5. App exit → drop sender / join worker (best-effort).

## Commands and results

Map 1:1 to current `sql_job` call sites in `gui.rs`. Command payloads **copy the fields each closure already captures today** (args, db name/id, object URN/id, sql text, plan mode, max rows, etc.) — no new engine APIs.

| Command | Worker work | UI applies |
|---------|-------------|------------|
| `Connect { args, refresh }` | `connect` + `ping` + `load_server_catalog` | catalog, labels, session; refresh failure keeps tree |
| `ExpandDatabase { db_id, db_name }` | reuse → load DB catalog | merge children / mark loaded |
| `LoadTableChildren { table_id, … }` | reuse → `load_table_children` | merge columns/indexes |
| `Describe { object_id, … }` | reuse → describe | `described` |
| `Script { object_id, … }` | reuse → script | open/fill query text |
| `Execute { sql, plan_mode, database, … }` | reuse → `run_query_ex` | results / messages / plan |
| `Disconnect` | drop client + session | UI disconnect path |

### UI pending state

- `pending: Option<JobKind>` (or equivalent) while a job is in flight.
- Status: Connecting… / Working… / Refreshing… then Ready or error (`status_error`).
- While `pending.is_some()`, do not submit another command; `ctx.request_repaint()` so `try_recv` runs.

## Reconnect policy

1. Hot path: no `engine.connect` if client is live.
2. If client is `None` but session args exist → connect once before the job.
3. If a job fails and the error looks like a dead session (IO / connection / TDS closed — pragmatic match on error `Display`/`source` chain) → drop client, `connect(session_args)`, **retry the same job once**. No UI banner on success (silent). Trace still records `engine.connect` on that reconnect.
4. If retry fails → surface error in status bar; leave client cleared or failed so next op can try again.

Disconnect and explicit Refresh remain the clean reconnect paths for the user.

## Engine trace acceptance

- Connect / Refresh / silent reconnect: spans include `engine.connect`.
- Execute / Expand / Describe / Script on a warm session: **no** full login span; total op time drops vs alpha traces where connect dominated short queries.

## Non-goals

- Query cancel (#9)
- Connection pool
- Windows Integrated Auth (#1)
- Multi-row filter (#2)
- Changing CLI connection model
- Nested / parallel SQL jobs from the UI

## Testing / verification

- `cargo test --lib`, `clippy -D warnings`, `fmt`
- Manual GUI: Connect → Expand → Execute → Engine Trace shows connect only on connect/reconnect
- Manual: UI stays responsive (status updates, window drag) during a multi-second query
- No “Cannot start a runtime from within a runtime” / worker panic closing the window

## PR

Title focused on session worker. Body: `Closes #3` and `Closes #4`.
