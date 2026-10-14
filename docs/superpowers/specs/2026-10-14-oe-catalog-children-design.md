# Object Explorer catalog children — design

Date: 2026-10-14  
Status: implemented  
Repo: `ssms-search-rs` v0.5.1

## Goal

Make the Object Explorer tree closer to SSMS for one database by adding:

- **Synonyms** and **Triggers** as Database-level folders (eager at connect)
- **Columns** and **Indexes** as folders under each **Table** (lazy on first expand)

Approach: extend existing `Catalog` / `ObjectKind` / `load_catalog` plus one new `load_table_children` helper. No new abstraction layers.

## Decisions (from brainstorming)

| Topic | Choice |
|-------|--------|
| Scope | Columns + Indexes + Synonyms + Triggers |
| Load strategy | Hybrid: Synonyms/Triggers eager; Columns/Indexes lazy |
| Triggers placement | Database folder only (not under Table) |
| Views | No Columns under View in this tranche |
| CLI | No new subcommands |

## Data model

New `ObjectKind` variants:

- `Column`
- `Index`
- `Synonym`
- `Trigger`

`as_str()` / SMO-style tokens:

| Kind | Token |
|------|--------|
| Column | `Column` |
| Index | `Index` |
| Synonym | `Synonym` |
| Trigger | `Trigger` |

### URN shapes

Consistent with existing templates:

```
Server[@Name='{server}']/Database[@Name='{db}']/Synonym[@Name='{name}' and @Schema='{schema}']
Server[@Name='{server}']/Database[@Name='{db}']/Trigger[@Name='{name}' and @Schema='{schema}']
Server[@Name='{server}']/Database[@Name='{db}']/Table[@Name='{t}' and @Schema='{s}']/Column[@Name='{c}']
Server[@Name='{server}']/Database[@Name='{db}']/Table[@Name='{t}' and @Schema='{s}']/Index[@Name='{i}']
```

Trigger leaf: name from `sys.triggers.name`; schema from `sys.objects`/`sys.schemas` for the trigger object_id (same pattern as tables). Parent table name is not required in URN for this tranche.

### Search / filter

`Catalog::search` leaf kinds include existing leaves plus `Synonym` and `Trigger`.

`Column` and `Index` are searchable **only if already loaded** into the catalog (lazy). No fetch-on-filter in this tranche.

Props remain Name / Schema / Owner / Type. Column/Index may use empty Owner; Schema inherits from parent table when useful for filter.

## API and SQL

### `load_catalog` (eager)

Unchanged core: Tables, Views, Stored Procedures, Functions.

Add:

1. Folder `Synonyms` + rows from `sys.synonyms` (user synonyms).
2. Folder `Triggers` + rows from `sys.triggers` where `parent_class = 1` (object/DML triggers on tables/views), joining names/schemas for display.
3. For each **Table** leaf: create empty child folders `Columns` and `Indexes` with URN prefixes under that table.

Track lazy load with a `HashSet<u64>` of folder (or table) ids whose children were loaded — owned by GUI state or a small field on a catalog wrapper. Prefer GUI `loaded_folders: HashSet<u64>` to avoid changing every `Catalog` clone path unless needed.

### `load_table_children` (lazy)

```text
load_table_children(client, database, schema, table_name, columns_folder_id, indexes_folder_id, server, db)
  -> Vec<CatalogObject>
```

- `USE [database]` (existing helper)
- Columns: `sys.columns` + `sys.types` (reuse describe-style join)
- Indexes: `sys.indexes` where `index_id > 0` for that `object_id`

Caller appends returned objects to `catalog.objects`.

### GUI

In `draw_tree_node` (or expand handler):

- When expanding a `Columns` or `Indexes` folder under a live (non-demo) connection, if children empty and folder id not in `loaded_folders`:
  - resolve parent Table schema/name from catalog
  - `sql_job` → `load_table_children`
  - `extend` catalog, mark loaded, keep expanded
- On failure: set error status; leave folder empty; do not panic

Demo catalog: preload Columns/Indexes under 1–2 sample tables (no SQL).

### Glyphs / properties

Extend `kind_glyph` / `short_type` for the four new kinds. Properties pane shows existing props for selected node.

## Errors

- Connect/catalog failures: existing path
- Lazy load failure: status bar message only; folder stays empty; retry allowed on next expand if not marked loaded (prefer **not** marking loaded on failure so retry works)

## Testing

- `ObjectKind::as_str` for new variants
- Demo tree contains Synonym and/or Trigger stubs if easy; at least one Table with Column + Index children
- `search` includes Synonym/Trigger; excludes unloaded Column/Index naturally when absent
- URN string shapes for Column/Index helpers (unit, no SQL)
- `cargo test --lib` green

## Out of scope

- Columns under Views
- Keys, Constraints, FK, Statistics
- Triggers also nested under Table
- Integrated Auth
- Multi-row filter GUI
- Graphical execution plan
- New CLI subcommands
- Fetch-on-filter for unloaded columns/indexes

## Success criteria

1. After connect, tree shows Synonyms and Triggers folders populated; each Table has Columns and Indexes folders.
2. First expand of Columns/Indexes on a live Table loads real children from SQL.
3. Filter finds Synonym and Trigger; Column/Index only after load.
4. Demo has at least one Table with static Columns/Indexes.
5. Lib tests pass; new kinds render in tree/properties.
