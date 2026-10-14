# OE Catalog Children Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add Synonyms/Triggers (eager DB folders) and Columns/Indexes (lazy under Table) to the Object Explorer catalog.

**Architecture:** Extend `ObjectKind` + `Catalog::demo` + `Catalog::search`; enrich `load_catalog`; add `load_table_children`; GUI expands Table and lazy-loads folder children via `loaded_folders: HashSet<u64>`.

**Tech Stack:** Rust, tiberius, egui, existing `ssms_search_rs` modules.

## Global Constraints

- Spec: `docs/superpowers/specs/2026-10-14-oe-catalog-children-design.md`
- Triggers = Database folder only (not under Table)
- Views = no Columns in this tranche
- No new CLI subcommands
- Lazy load failure: status error, do **not** mark folder loaded (retry on next expand)
- `loaded_folders` lives on GUI (`SsmsApp`), not on `Catalog`
- YAGNI: no ChildSource trait, no fetch-on-filter
- Verify: `cargo test --lib` after each task
- Work on branch `feat/oe-catalog-children` (not main)
- Commit after each task

## File map

| File | Role |
|------|------|
| `src/catalog.rs` | `ObjectKind` variants, `search` leaves, demo Columns/Indexes (+ optional Synonym/Trigger) |
| `src/functions.rs` | `load_catalog` Synonyms/Triggers + empty Columns/Indexes folders; `load_table_children` |
| `src/lib.rs` | Re-export `load_table_children` if needed |
| `src/gui.rs` | glyphs; Table expandable; `loaded_folders`; lazy expand handler |

---

### Task 1: ObjectKind + search + demo skeleton

**Files:**
- Modify: `src/catalog.rs`
- Test: `src/catalog.rs` (`#[cfg(test)]`)

**Interfaces:**
- Produces: `ObjectKind::{Column, Index, Synonym, Trigger}` with `as_str()` → `"Column"`, `"Index"`, `"Synonym"`, `"Trigger"`
- Produces: `Catalog::search` treats Synonym/Trigger as leaves (same as Table/View/Proc/Function); Column/Index also matchable when present
- Produces: demo has ≥1 Table with child Folders `Columns`/`Indexes` each containing ≥1 Column and ≥1 Index; optionally ≥1 Synonym and ≥1 Trigger under DB folders

- [ ] **Step 1: Write failing tests** in `src/catalog.rs` tests module:

```rust
#[test]
fn object_kind_new_variants_as_str() {
    assert_eq!(ObjectKind::Column.as_str(), "Column");
    assert_eq!(ObjectKind::Index.as_str(), "Index");
    assert_eq!(ObjectKind::Synonym.as_str(), "Synonym");
    assert_eq!(ObjectKind::Trigger.as_str(), "Trigger");
}

#[test]
fn search_includes_synonym_and_trigger() {
    let cat = Catalog::demo();
    let sc = SearchConditions::default();
    let hits = cat.search(&sc);
    assert!(hits.iter().any(|o| o.kind == ObjectKind::Synonym));
    assert!(hits.iter().any(|o| o.kind == ObjectKind::Trigger));
}

#[test]
fn demo_table_has_column_and_index_children() {
    let cat = Catalog::demo();
    let table = cat.objects.iter().find(|o| o.kind == ObjectKind::Table && o.name == "Employee").expect("Employee");
    let folders: Vec<_> = cat.children(Some(table.id));
    assert!(folders.iter().any(|o| o.name == "Columns"));
    assert!(folders.iter().any(|o| o.name == "Indexes"));
    let cols_folder = folders.iter().find(|o| o.name == "Columns").unwrap();
    let idx_folder = folders.iter().find(|o| o.name == "Indexes").unwrap();
    assert!(cat.children(Some(cols_folder.id)).iter().any(|o| o.kind == ObjectKind::Column));
    assert!(cat.children(Some(idx_folder.id)).iter().any(|o| o.kind == ObjectKind::Index));
}
```

- [ ] **Step 2: Run tests — expect FAIL** (variants missing)

```powershell
cargo test --lib object_kind_new_variants_as_str search_includes_synonym demo_table_has_column -- --nocapture
```

Expected: compile error or FAIL on missing variants / assertions.

- [ ] **Step 3: Implement** — add enum variants + `as_str` arms; update `Catalog::search` filter to exclude only Server/Database/Folder (Column/Index/Synonym/Trigger included); extend `Catalog::demo()`:
  - Under DB: folders Synonyms + Triggers with one sample each (URN shapes per spec)
  - Under Employee table: folders Columns + Indexes with sample leaves (`BusinessEntityID` column, `PK_Employee` index); URN under table path
  - Keep existing tables without children (lazy-empty pattern for live is separate)

- [ ] **Step 4: Run tests — expect PASS**

```powershell
cargo test --lib -- --nocapture
```

Expected: all lib tests pass. Fix any exhaustive-match breaks in other modules only enough to compile (temporary `_ =>` only if Task 3 not yet done — prefer fixing `kind_glyph`/`short_type` in Task 3; if compile fails on gui matches, add arms with placeholder glyphs in this commit).

- [ ] **Step 5: Commit**

```powershell
git add src/catalog.rs src/gui.rs
git commit -m "Add Column/Index/Synonym/Trigger kinds and demo tree children."
```

---

### Task 2: load_catalog eager Synonyms/Triggers + empty Columns/Indexes folders

**Files:**
- Modify: `src/functions.rs` (`load_catalog`)
- Test: unit tests for URN helpers if extracted; otherwise structural tests via a small pure helper

**Interfaces:**
- Consumes: existing `load_catalog(client, server, database) -> Result<Catalog>`
- Produces: same function also creates Synonyms/Triggers folders+leaves; each Table gets empty `Columns` and `Indexes` Folder children
- Produces (optional helper): `fn table_child_folder_urn(table_urn: &str, folder: &str) -> String` e.g. `format!("{table_urn}/{folder}")`

- [ ] **Step 1: Write failing test** for URN helper (pure):

```rust
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
```

- [ ] **Step 2: Run — expect FAIL** (helper missing)

```powershell
cargo test --lib table_child_folder_urn_appends_segment -- --nocapture
```

- [ ] **Step 3: Implement helper + extend `load_catalog`**

After creating each Table object, also `add` two Folder children:

```rust
let table_id = add(/* ... Table ... */);
let _columns_id = add(
    Some(table_id),
    ObjectKind::Folder,
    "Columns",
    "",
    "",
    format!("{table_urn}/Columns"),
);
let _indexes_id = add(
    Some(table_id),
    ObjectKind::Folder,
    "Indexes",
    "",
    "",
    format!("{table_urn}/Indexes"),
);
```

Where `table_urn` is the URN string just built for that table.

Add Synonyms folder under `db_id`:

```sql
SELECT s.name AS schema_name, syn.name AS object_name,
       ISNULL(USER_NAME(syn.principal_id), s.name) AS owner_name
FROM sys.synonyms AS syn
INNER JOIN sys.schemas AS s ON syn.schema_id = s.schema_id
ORDER BY s.name, syn.name
```

URN: `…/Synonym[@Name='…' and @Schema='…']`

Add Triggers folder under `db_id`:

```sql
SELECT sch.name AS schema_name, t.name AS object_name,
       ISNULL(USER_NAME(o.principal_id), sch.name) AS owner_name
FROM sys.triggers AS t
INNER JOIN sys.objects AS o ON t.object_id = o.object_id
INNER JOIN sys.schemas AS sch ON o.schema_id = sch.schema_id
WHERE t.parent_class = 1
ORDER BY sch.name, t.name
```

URN: `…/Trigger[@Name='…' and @Schema='…']`

Keep existing object query unchanged except Table branch adds folders.

- [ ] **Step 4: Run**

```powershell
cargo test --lib -- --nocapture
```

Expected: PASS (no live SQL in CI).

- [ ] **Step 5: Commit**

```powershell
git add src/functions.rs
git commit -m "Load synonyms, triggers, and empty Columns/Indexes folders in catalog."
```

---

### Task 3: load_table_children + GUI lazy expand

**Files:**
- Modify: `src/functions.rs` (add `load_table_children`)
- Modify: `src/lib.rs` (re-export if used from gui via `functions::`)
- Modify: `src/gui.rs` (`loaded_folders`, expandable Table, glyphs, expand → load)

**Interfaces:**
- Produces:

```rust
pub async fn load_table_children(
    client: &mut SqlClient,
    database: &str,
    server: &str,
    schema: &str,
    table_name: &str,
    columns_folder_id: u64,
    indexes_folder_id: u64,
) -> Result<Vec<CatalogObject>>
```

- Returns Column objects parented to `columns_folder_id` and Index objects parented to `indexes_folder_id`
- Column URN: `{table_urn}/Column[@Name='{c}']` where `table_urn` rebuilt like load_catalog
- Index URN: `{table_urn}/Index[@Name='{i}']`
- Indexes query: `sys.indexes` where `object_id = OBJECT_ID(@schema.@table)` and `index_id > 0`
- Columns: same join as `describe_object` but build `CatalogObject`s

GUI:
- `loaded_folders: HashSet<u64>` on `SsmsApp`; clear on connect/demo reload
- Expandable: add `ObjectKind::Table` to expandable match (so Columns/Indexes folders visible)
- On twisty open for Folder named `Columns` or `Indexes`: if `!is_demo && connected && !loaded_folders.contains(&id) && children empty`:
  1. Find parent Table (walk `parent_id`)
  2. Find sibling Columns and Indexes folder ids under that table
  3. `sql_job` → `load_table_children` for **both** folders in one call
  4. On Ok: `catalog.objects.extend`; insert both folder ids into `loaded_folders`
  5. On Err: status_error + message; do **not** insert into `loaded_folders`

Glyphs: Column `·`, Index `⌁`, Synonym `⇄`, Trigger `⚡` (or similar ASCII-safe). Update `short_type` arms.

- [ ] **Step 1: Write failing unit test** for building column/index URN (pure helper in functions.rs):

```rust
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
}
```

- [ ] **Step 2: Run — expect FAIL**

```powershell
cargo test --lib column_and_index_urn_shapes -- --nocapture
```

- [ ] **Step 3: Implement** `child_leaf_urn`, `load_table_children`, GUI wiring, glyphs.

- [ ] **Step 4: Run**

```powershell
cargo test --lib -- --nocapture
```

Expected: PASS. Also `cargo check` for gui compile.

- [ ] **Step 5: Commit**

```powershell
git add src/functions.rs src/lib.rs src/gui.rs
git commit -m "Lazy-load table Columns and Indexes on Object Explorer expand."
```

---

### Task 4: Search/docs sanity + final verification

**Files:**
- Modify: `src/catalog.rs` tests if needed
- Modify: `docs/superpowers/specs/2026-10-14-oe-catalog-children-design.md` status line → implemented
- Modify: `README.md` only if a one-line GUI note is warranted (optional; skip if nothing user-facing to document beyond OE)

- [ ] **Step 1: Add test** that search by Type Equal Synonym works when demo has synonym:

```rust
#[test]
fn search_type_synonym() {
    let cat = Catalog::demo();
    let sc = SearchConditions {
        items: vec![FilterItem::new("Type", FilterOperator::Equal, "Synonym")],
    };
    let hits = cat.search(&sc);
    assert!(!hits.is_empty());
    assert!(hits.iter().all(|o| o.kind == ObjectKind::Synonym));
}
```

- [ ] **Step 2–4: Run full lib tests + clippy quiet**

```powershell
cargo test --lib
cargo clippy --lib -- -D warnings
```

Expected: PASS / no warnings.

- [ ] **Step 5: Commit**

```powershell
git add src/catalog.rs docs/superpowers/specs/2026-10-14-oe-catalog-children-design.md README.md
git commit -m "Verify OE catalog children search coverage and mark spec done."
```

---

## Spec coverage check

| Spec requirement | Task |
|------------------|------|
| ObjectKind Column/Index/Synonym/Trigger | 1 |
| URN shapes | 1–3 |
| search Synonym/Trigger; Column/Index if loaded | 1, 3 |
| load_catalog Synonyms/Triggers + empty folders | 2 |
| load_table_children | 3 |
| GUI loaded_folders + lazy expand | 3 |
| glyphs/properties | 3 |
| demo Columns/Indexes | 1 |
| no View columns / no CLI / triggers DB-only | constraints |
| tests + cargo test --lib | 1–4 |
