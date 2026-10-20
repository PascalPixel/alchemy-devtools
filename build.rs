//! With the `render` feature, compiles the libmgba shim (src/render/mgba.c)
//! and links libmgba. MGBA_PREFIX names the prefix that holds include/mgba
//! and lib/libmgba; otherwise the usual prefixes are tried.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    #[cfg(feature = "render")]
    render();
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
