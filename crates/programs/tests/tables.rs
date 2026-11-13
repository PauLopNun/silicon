//! `programs/maze3d/tables.inc` is generated; this keeps it that way.

use silicon_programs::tables;

/// Run with `SILICON_BLESS=1` to rewrite the file after changing the
/// generator.
#[test]
fn maze3d_tables_are_up_to_date() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../programs/maze3d/tables.inc"
    );
    let generated = tables::maze3d();
    let on_disk = std::fs::read_to_string(path)
        .unwrap_or_default()
        .replace("\r\n", "\n");
    if on_disk != generated {
        if std::env::var_os("SILICON_BLESS").is_some() {
            std::fs::write(path, generated).expect("the tables file can be written");
        } else {
            panic!("programs/maze3d/tables.inc is stale; rerun with SILICON_BLESS=1");
        }
    }
}
