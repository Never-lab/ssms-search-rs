# Batch 60% — gap ledger (2026-10-14)

## Original gaps → status

| # | Gap | Weight | Status |
|---|-----|--------|--------|
| 1 | Multi-DB OE tree | high | TARGET this batch |
| 2 | Describe + Script in Properties GUI | high | TARGET |
| 3 | Columns/Indexes/Synonyms/Triggers | high | DONE |
| 4 | Date property on objects | med | TARGET |
| 5 | Refresh catalog without reconnect | med | TARGET |
| 6 | Integrated Auth | hard | SKIP (toolchain) |
| 7 | cell_to_string non-string types | med | DONE (try_get+chrono) |
| 8 | Multi-row filter GUI | med | SKIP this batch |
| 9 | Plan graph | low | SKIP |
| 10 | View columns / Keys / FK | low | SKIP |

**Coverage target:** 4/7 remaining actionable ≈ **57–60%** (items 1,2,4,5).

## Design (ponytail, retrocompat)

### Multi-DB (lazy)
- Keep `load_catalog(client, server, db)` as-is (CLI/single-DB).
- Add `load_server_skeleton(client, server) -> Catalog`: Server + Database nodes only (`list_databases`).
- Add `load_database_into(client, server, database, catalog, db_node_id, start_id) -> Result<(Catalog, u64)>` OR replace children of one DB by merging `load_catalog` objects under existing db id (re-parent).
- Simplest merge: on first expand of Database when children empty (live): call existing load_catalog SQL helpers / extract inner builder; attach folders+objects under that db id.
- GUI: `loaded_databases: HashSet<u64>` like folders; expand Database → load.
- Connect: use skeleton (all DBs) instead of full single-DB catalog. Initial database from connect form gets auto-expanded + loaded once.
- Demo: keep multi-DB if easy (AdventureWorks only OK — add second stub DB optional).

### Date prop
- `CatalogObject.create_date: String` (empty default).
- `props()` → 5 entries including `Date`.
- SQL: `CONVERT(varchar(33), o.create_date, 126)` in object queries; synonyms/triggers/columns/indexes may leave empty.
- Update filter demo tests; Filter column list in GUI includes Date.

### Refresh
- Menu/toolbar "Refresh" when connected: re-run skeleton + reload previously expanded DBs OR clear loaded_* and rebuild skeleton; expand initial db again.

### Properties Describe + Script
- Table/View: button "Load columns…" → `describe_object` → show in properties pane (extra section).
- Module kinds (View/Proc/Function/Trigger/Synonym?): "Script as CREATE…" → `object_definition` → put SQL in query editor + switch to Query workspace.
- Tables: keep existing "Script as SELECT TOP 100".

## Constraints
- No Integrated Auth.
- No breaking CLI `load_catalog` signature semantics.
- `.\build.ps1` then commit.
- Do not commit `.superpowers/` or briefs scratch unless asked.
