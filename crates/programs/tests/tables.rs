//! The tables of maze3d are generated; this keeps the checked-in files that
//! way.

use silicon_programs::tables;

/// Compare a generated file with the one in `programs/`. Run with
/// `SILICON_BLESS=1` to rewrite it after changing the generator.
fn up_to_date(file: &str, generated: &str) {
    let path = format!("{}/../../programs/{file}", env!("CARGO_MANIFEST_DIR"));
    let on_disk = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .replace("\r\n", "\n");
    if on_disk != generated {
        if std::env::var_os("SILICON_BLESS").is_some() {
            std::fs::write(&path, generated).expect("the file can be written");
        } else {
            panic!("programs/{file} is stale; rerun with SILICON_BLESS=1");
        }
    }
}

#[test]
fn maze3d_tables_are_up_to_date() {
    up_to_date("maze3d/tables.inc", &tables::maze3d());
}

#[test]
fn maze3d_steps_are_up_to_date() {
    up_to_date("maze3d/walk.h", &tables::maze3d_header());
}
