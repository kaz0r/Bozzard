//! Shared native build setup. Cargo stages the same SDK used by steamworks-sys.
use std::{env, fs, path::PathBuf, process::Command};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-env-changed=STEAM_SDK_LOCATION");
    if env::var_os("CARGO_FEATURE_STEAM").is_none() {
        return Ok(());
    }
    let os = env::var("CARGO_CFG_TARGET_OS")?;
    let arch = env::var("CARGO_CFG_TARGET_ARCH")?;
    let (folder, library) = match (os.as_str(), arch.as_str()) {
        ("windows", "x86_64") => ("win64", "steam_api64.dll"),
        ("linux", "x86_64") => ("linux64", "libsteam_api.so"),
        ("linux", "aarch64") => ("linuxarm64", "libsteam_api.so"),
        ("macos", "x86_64" | "aarch64") => ("osx", "libsteam_api.dylib"),
        _ => return Err(format!("Unsupported Steam target: {os}/{arch}").into()),
    };
    let sdk = if let Some(path) = env::var_os("STEAM_SDK_LOCATION") {
        PathBuf::from(path)
    } else {
        // Metadata resolves dependency source paths, including custom CARGO_HOME/vendor setups.
        // It does not build anything or download dependencies.
        let output = Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
            .args(["metadata", "--offline", "--locked", "--format-version", "1"])
            .arg("--filter-platform")
            .arg(env::var("TARGET")?)
            .output()?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
        }
        let metadata: serde_json::Value = serde_json::from_slice(&output.stdout)?;
        let package = metadata["packages"].as_array().ok_or("missing packages")?
            .iter().find(|p| p["name"] == "steamworks-sys" && p["version"] == "0.13.0")
            .ok_or("expected locked steamworks-sys 0.13.0; update the native SDK resolver with the dependency")?;
        PathBuf::from(
            package["manifest_path"]
                .as_str()
                .ok_or("missing SDK manifest")?,
        )
        .parent()
        .ok_or("missing SDK directory")?
        .join("lib/steam")
    };
    let source = sdk
        .join("redistributable_bin")
        .join(folder)
        .join(library)
        .canonicalize()?;
    println!("cargo:rerun-if-changed={}", source.display());
    println!("cargo:rustc-env=BOZZARD_STEAM_LIBRARY={library}");
    println!(
        "cargo:rustc-env=BOZZARD_STEAM_LIBRARY_PATH={}",
        source.display()
    );
    let out = PathBuf::from(env::var_os("OUT_DIR").ok_or("missing OUT_DIR")?);
    let profile = out
        .ancestors()
        .nth(3)
        .ok_or("unexpected Cargo output layout")?;
    let bytes = fs::read(&source)?;
    for directory in [profile.to_owned(), profile.join("deps")] {
        fs::create_dir_all(&directory)?;
        let target = directory.join(library);
        // Avoid rewriting a loaded DLL on Windows when another app is running.
        if fs::read(&target).ok().as_deref() != Some(&bytes) {
            fs::write(target, &bytes)?;
        }
    }
    // Executables and exported games load their adjacent redistributable without a launcher.
    match os.as_str() {
        "linux" => println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN"),
        "macos" => println!("cargo:rustc-link-arg=-Wl,-rpath,@loader_path"),
        _ => {}
    }
    Ok(())
}
