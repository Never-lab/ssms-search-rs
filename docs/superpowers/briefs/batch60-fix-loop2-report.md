# Batch 60% — fix loop 2 report

Addresses remaining **Important** items from `batch60-audit-loop2.md` (I5 tail, I6, I8 tail).

## Changes

| ID | Fix |
|---|---|
| I5 tail | `ensure_database_loaded`: after Database kind check, require `database` equals node `name` (ASCII case-insensitive); clear bail on mismatch. |
| I6 | Object Explorer Details grid: **Database** column via `Catalog::database_name_of`. Status from `refresh_object_hits` when connected and `database_count() > 1`: `{hits} object(s)[ matching filter] · {loaded}/{total} databases loaded` (`loaded = loaded_databases.len()`). |
| I8 tail | `Catalog::database_name_of`, `database_count`; GUI uses catalog helper (removed private `database_of`). Tests: `database_name_of_walks_to_database`, `serde_deserializes_without_create_date`. |

## Verification

| Check | Result |
|---|---|
| `cargo test --lib` (D:\mingw64 linker) | **62 passed**, 0 failed |
| `.\build.ps1` | **62 passed** + release OK; `release/README.txt` updated |

## Commit

`d2a51b5` on `feat/oe-catalog-children` (not pushed).

## Audit expectation

Important **I5/I6/I8** closed in code; live SQL smoke still out of scope. Re-audit should reach **≥ 90/100** if static review agrees.
