# UI B+C + graphical plan report

- Status: done. `src/gui.rs` polished (OE ASCII glyphs, full-width tree rows, header + connection label, plain toolbar, denser grids, framed editor, underline bottom tabs) and `draw_plan_graph` added (L->R tree via `plan_children`, collapsible Operator list, Raw XML kept).
- Verified: `cargo test` 63 passed (lib); `cargo build --release` OK, no warnings.
- Not verified: visual check of the GUI (no interactive run).
- Concerns: roots are `parent.is_none()`; ops with an out-of-range parent are not drawn in the graph (still in list). `build.ps1` failed once because target\release\ssms-search-rs.exe was running; renamed the locked exe and finished build + copy manually.
