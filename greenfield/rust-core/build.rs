fn main() {
    use std::env;
    use std::path::PathBuf;

    if let (Ok(folder), Ok(crate_name)) = (
        env::var("NAPI_TYPE_DEF_TMP_FOLDER"),
        env::var("CARGO_PKG_NAME"),
    ) {
        let mut path = PathBuf::from(folder);
        path.push(format!("{}.d.ts", crate_name));
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        println!("cargo:rustc-env=TYPE_DEF_TMP_PATH={}", path.display());
    }
    napi_build::setup();
}
