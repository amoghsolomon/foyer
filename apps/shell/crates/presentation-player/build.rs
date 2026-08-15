#[cfg(target_os = "linux")]
fn main() {
    use std::{env, fs, os::unix::fs::symlink, path::Path};

    // Some Fedora installations have the runtime library but omit the unversioned linker name
    // supplied by libxkbcommon-x11-devel. Keep the workaround local to Cargo's output directory.
    let runtime_library = [
        "/usr/lib64/libxkbcommon-x11.so.0",
        "/usr/lib/x86_64-linux-gnu/libxkbcommon-x11.so.0",
    ]
    .into_iter()
    .map(Path::new)
    .find(|path| path.exists());

    let Some(runtime_library) = runtime_library else {
        return;
    };
    let out_dir = env::var_os("OUT_DIR").expect("Cargo must set OUT_DIR");
    let link_dir = Path::new(&out_dir).join("native-libs");
    fs::create_dir_all(&link_dir).expect("failed to create native library directory");
    let linker_name = link_dir.join("libxkbcommon-x11.so");
    if !linker_name.exists() {
        symlink(runtime_library, &linker_name).expect("failed to link xkbcommon runtime library");
    }
    println!("cargo:rustc-link-search=native={}", link_dir.display());
}

#[cfg(not(target_os = "linux"))]
fn main() {}
