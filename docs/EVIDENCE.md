# How search works in SSMS 22 — evidence ledger

Target install (this host, 2026-10-14):

`C:\Program Files\Microsoft SQL Server Management Studio 22\Release\Common7\IDE\`

Investigation used REA managed static analysis + UTF-16/ASCII string recovery.  
Static analysis does **not** observe execution. Conclusions are labeled below.

REA evidence bundle export: `Documents\Context\ssms-search-rea-evidence.json`

---

## Q1 — What is the search surface?

**Answered (observed).** Two layers:

| Layer | Role | Primary artifacts |
|-------|------|-------------------|
| Object Explorer filter | SQL object tree filter / criteria dialog | `ObjectExplorer.dll` |
| VS shell search | All-in-one search controller / providers | `CommonExtensions\Microsoft\Search\*.dll` |

### Evidence — Object Explorer

- **File:** `ObjectExplorer.dll`
- **SHA-256:** `e0c2a57219a9d176ca78b90914f0969995281b58d24073624d58a0356d639c54`
- **REA:** `inspect_managed_artifact` → `ev_cb47763a30642cafca3c204ae804748b53586c5234ac9017c39832f92de5b2c9`
- **REA:** `inspect_managed_members` → `ev_b2871b3c47539f9d012078416b6a1b7e7d22d3a5c97f37e8406dc497bad4bca2`  
  **Limitation:** CLI metadata exceeded 64 MiB representation budget; members unavailable via REA. Strings recovered separately from the same bytes.

Observed type / API names (ASCII heap):

- `FilterDialog`, `FilterItem`, `IFilterProvider`, `IFilterRow`, `IFilterStorage`
- `IUrnSearchable`, `GetFilterForUrn`, `MakeVsFilterString`, `SearchConditions`
- `NonContextFilterHierarchyNodeBuilder*`, `NonContextFilterNavigableItemBuilder*`
- `cmdidFilter`, `ClearFilter`, `lblFilterCriteria`, `filterGrid`

### Evidence — VS Search stack

- `Microsoft.VisualStudio.Search.dll` (contracts)
- `Microsoft.VisualStudio.Search.Implementation.dll`  
  Observed: `SearchController`, `PerProviderSearchPerformer`, `ResultAggregator`,  
  `ISearchItemsSourceProvider`, `PatternMatching`, `SearchFilterRepository`,  
  `IndexedSearchItemsSourceProvider`, `RankingManager`, `MostRecentQueryManager`
- `Microsoft.VisualStudio.Platform.SearchProviders.dll`

REA retained members Evidence IDs (transport-large; in exported bundle):

- `ev_c32c4f36c638209c15c2059ea1d6b06354be32e0e11b427ace3b4770307b419d` (Search.Implementation members)
- `ev_5cd66572430a4eb617b34ee18566ee5d93af462ee44587f82a2f039b3001d343` (SqlStudio.Explorer members)

---

## Q2 — What filter operators does Object Explorer expose?

**Answered (observed string sequence)** in `ObjectExplorer.dll` UTF-16:

```
Equal | NotEqual | StartsWith | Contains | NotContains |
LessThan | LessThanEqual | GreaterThan | GreaterThanEqual |
Between | NotBetween
```

Adjacent property labels (same region): `Name`, `Schema`, `Owner`, `Date`, `Category`, …

---

## Q3 — How are objects addressed?

**Answered (observed URN templates)** in `ObjectExplorer.dll`:

- `Server[@Name='{0}']/Database[@Name='{1}']/{2}{3}`
- `Server[@Name='{0}']/Information`
- `…/Table[@Name='…' and @Schema='…']`-style paths (URN count in ASCII heap ~740 `Urn*` tokens)

**Inference:** filters are applied against SMO/SFC URN-addressable children (`IUrnSearchable` / `GetFilterForUrn`), not a free-text full-text index inside ObjectExplorer itself.

---

## Q4 — How does “search without expanding the tree” work?

**Partially answered (inferred from names).**

`NonContextFilter*` builders suggest a parallel hierarchy walk that does not require the current context node — i.e. filter/search across objects outside the expanded tree selection. Exact CIL control flow was **not** recovered (metadata budget / no decompile of 13 MB assembly).

---

## Q5 — Relation to shell Search (Ctrl+T style)?

**Answered at architecture level (observed).**

VS Search is a MEF provider pipeline:

1. Query tokens (`ISearchQuery`)
2. Per-provider performers
3. Pattern match + ranking
4. Result aggregation → UI broker

This is the Visual Studio shell feature hosted by SSMS 22 (VS-based). SQL Object Explorer filtering is a separate WinForms dialog path (`FilterDialog`).

---

## What this Rust app reconstructs

Independent reimplementation of **Object Explorer filter semantics** only:

- Same operator set
- Column-oriented criteria (Name / Schema / Owner / Type)
- URN-shaped paths
- Non-context result list (scan catalog leaves)

**Not** a decompilation or binary reuse of Microsoft code.  
**Not** a clone of VS All-In-One Search indexing.
