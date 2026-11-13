//! With the `render` feature, compiles the libmgba shim (src/render/mgba.c)
//! and links libmgba. MGBA_PREFIX names the prefix that holds include/mgba
//! and lib/libmgba; otherwise the usual prefixes are tried.
//!
//! Always, it compiles in each table of `evidence/` (H8), so the program
//! carries the evidence of its own revision wherever it runs; a game's table
//! that does not exist yet is simply absent.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    evidence();
    #[cfg(feature = "render")]
    render();
}

fn evidence() {
    use std::fmt::Write;
    use std::path::{Path, PathBuf};

    let folder = Path::new(&std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR")).join("evidence");
    println!("cargo:rerun-if-changed={}", folder.display());
    let mut tables: Vec<PathBuf> = std::fs::read_dir(&folder)
        .map(|entries| entries.filter_map(|entry| entry.ok().map(|entry| entry.path())).filter(|path| path.extension().is_some_and(|e| e == "tsv")).collect())
        .unwrap_or_default();
    tables.sort();
    let mut code = String::from("/// Each table of `evidence/`, by file name, as this program was built with it.\npub const TABLES: &[(&str, &str)] = &[\n");
    for table in &tables {
        let name = table.file_name().expect("a table's name").to_string_lossy().into_owned();
        let _ = writeln!(code, "    ({name:?}, include_str!({:?})),", table.display().to_string());
    }
    code.push_str("];\n");
    let out = Path::new(&std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR")).join("evidence.rs");
    std::fs::write(out, code).expect("write the evidence tables");
}

#[cfg(feature = "render")]
fn render() {
    use std::path::PathBuf;

    println!("cargo:rerun-if-changed=src/render/mgba.c");
    println!("cargo:rerun-if-env-changed=MGBA_PREFIX");
    let prefix = std::env::var_os("MGBA_PREFIX")
        .map(PathBuf::from)
        .or_else(|| {
            ["/opt/homebrew", "/usr/local", "/usr"]
                .iter()
                .map(PathBuf::from)
                .find(|p| p.join("include/mgba/core/core.h").is_file())
        })
        .unwrap_or_else(|| panic!("the render feature needs libmgba and its headers: install mGBA's library or set MGBA_PREFIX"));
    cc::Build::new().file("src/render/mgba.c").include(prefix.join("include")).compile("alchemy_mgba");
    println!("cargo:rustc-link-search=native={}", prefix.join("lib").display());
    println!("cargo:rustc-link-lib=dylib=mgba");
}
