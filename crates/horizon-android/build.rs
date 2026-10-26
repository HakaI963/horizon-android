use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=HORIZON_ANDROID_SEED");
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let seed = env::var_os("HORIZON_ANDROID_SEED").filter(|path| !path.is_empty());
    let bytes = match seed {
        Some(path) => {
            let path = PathBuf::from(path);
            println!("cargo:rerun-if-changed={}", path.display());
            fs::read(&path).unwrap_or_else(|error| {
                panic!("HORIZON_ANDROID_SEED {}: {error}", path.display())
            })
        }
        None => Vec::new(),
    };
    fs::write(out.join("seed.bin"), bytes).expect("write the embedded seed");
}
