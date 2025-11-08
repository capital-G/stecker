use std::fs;
use std::path::Path;

fn main() {
    let templates_path = Path::new("templates");
    let static_path = Path::new("static");

    let out_dir_env = std::env::var("OUT_DIR").unwrap();
    let out_dir = Path::new(&out_dir_env);
    let target_path = out_dir.ancestors().nth(3).unwrap();

    let templates_target_path = target_path.join("templates");
    let static_target_path = target_path.join("static");

    for (source_path, target_path) in vec![
        (templates_path, templates_target_path),
        (static_path, static_target_path),
    ] {
        // println!("cargo:warning=COPYING FILES TO {target_path:?}");
        if target_path.exists() {
            fs::remove_dir_all(&target_path).unwrap();
        };
        fs::create_dir_all(&target_path).unwrap();

        for file in fs::read_dir(source_path).unwrap() {
            let file = file.unwrap();
            let path = file.path();
            if path.extension().is_some() {
                let dest_file = target_path.join(file.file_name());
                fs::copy(&path, &dest_file).unwrap();
            }
        }
    }

    println!("cargo:rerun-if-changed=templates");

    // println!("cargo:warning=Copied server artifacts to {target_path:?}");
}
