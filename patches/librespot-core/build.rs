fn main() {
    // Build info omitido para evitar conflito de versão do vergen-lib.
    println!("cargo:rustc-env=VERGEN_BUILD_DATE=unknown");
    println!("cargo:rustc-env=VERGEN_GIT_SHA=unknown");
    println!("cargo:rustc-env=VERGEN_GIT_COMMIT_DATE=unknown");
    println!("cargo:rustc-env=LIBRESPOT_BUILD_ID=isi-music");
}
