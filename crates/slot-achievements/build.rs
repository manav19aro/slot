use std::path::Path;

fn main() {
    let root = Path::new("native/rcheevos");
    let mut build = cc::Build::new();
    build
        .include(root.join("include"))
        .include(root.join("src/runtime"));
    for dir in ["src/runtime", "src/util"] {
        let mut sources: Vec<_> = std::fs::read_dir(root.join(dir))
            .expect("vendored rcheevos sources")
            .map(|entry| entry.expect("source entry").path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "c"))
            .collect();
        sources.sort();
        build.files(sources);
    }
    build
        .file(root.join("src/hash/rc_hash.c"))
        .define("RC_HASH_NO_DISC", None)
        .define("RC_HASH_NO_ROM", None)
        .define("RC_HASH_NO_ENCRYPTED", None)
        .define("RC_HASH_NO_ZIP", None);
    build.file("native/bridge.c").compile("slot_rcheevos");
    println!("cargo:rerun-if-changed=native");
}
