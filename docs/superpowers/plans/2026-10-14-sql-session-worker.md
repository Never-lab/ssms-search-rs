# SQL session worker Implementation Plan

> **For agentic workers:** Implement task-by-task. Spec: `docs/superpowers/specs/2026-10-14-sql-session-worker-design.md`.

**Goal:** Persistent TDS `SqlClient` on a dedicated worker thread; GUI never `join()`s SQL work (#3 + #4).

**Architecture:** Binary `sql_worker` module owns Tokio runtime + optional client; `gui` submits commands and `try_recv`s results each frame.

**Tech Stack:** `std::sync::mpsc`, Tokio current_thread, Tiberius `SqlClient`, egui.

## Global Constraints

- One client, one job in flight; silent reconnect once on stale connection
- Cancel out of scope; CLI unchanged; no new deps
- `Closes #3` `Closes #4`

---

### Task 1: `sql_worker.rs` + wire `main.rs`

- [x] Command/Response enums mirroring current `sql_job` call sites
- [x] Worker loop: ensure client, run job, stale→reconnect once→retry
- [x] `SqlWorkerHandle { submit, try_recv }` spawned from `SsmsApp::new`
- [x] `mod sql_worker` in `main.rs`

### Task 2: Refactor `gui.rs`

- [x] Remove `sql_job`; add `pending` + poll in `update`
- [x] Convert Connect/Execute/Expand/Describe/Script/LoadTableChildren to submit
- [x] Apply responses (preserve refresh-keeps-tree, status strings)
- [x] `load_demo` / disconnect path sends `Disconnect`

### Task 3: Verify

- [x] `cargo test --lib` (67 passed)
- [x] `cargo clippy --all-targets -- -D warnings`
- [x] `cargo check --bins`
