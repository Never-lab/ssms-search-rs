# ssms-search-rs

Desktop **Object Explorer** inspired by SSMS, built with **egui/eframe** (finestra nativa Windows) + toolkit SQL Server in Rust.

Evidence SSMS 22: [docs/EVIDENCE.md](docs/EVIDENCE.md).

## Perché egui

UI **esterna** (non più TUI in terminale): menu, toolbar, Object Explorer, Details/Properties, dialog Connect e Filter Settings. Stesso core (`connection` / `catalog` / `filter` / `functions`).

## Release

```powershell
.\build.ps1
# → release\ssms-search-rs.exe
```

Avvio GUI (default):

```powershell
.\release\ssms-search-rs.exe
# oppure
.\release\ssms-search-rs.exe gui
```

## GUI

| Area / controllo | Cosa fa |
|------------------|---------|
| **New Query** | Apre editor SQL (`SQLQuery1.sql`) |
| **▶ Execute** / `F5` | Esegue la query (con Actual Plan se spuntato) |
| **Estimated Plan** | `SET SHOWPLAN_XML` (non esegue) |
| **Include Actual Plan** | `SET STATISTICS XML ON` + execute |
| Results / Messages / **Execution Plan** | Pannelli sotto l’editor (stile SSMS) |
| Connect to Server… | Login SQL, catalog live |
| Object Explorer / Filter | Albero + Filter Settings |

## CLI (headless, opzionale)

```powershell
.\release\ssms-search-rs.exe connect -U sa -P '...' 
.\release\ssms-search-rs.exe search --demo -c Name -o Contains -v User
.\release\ssms-search-rs.exe query -U sa -P '...' -d MyDb "SELECT TOP 5 name FROM sys.tables"
```

## Build notes

- Toolchain: `stable-x86_64-pc-windows-gnu` + `D:\mingw64`
- SQL auth via `-U`/`-P` o dialog GUI; Integrated Auth non ancora
