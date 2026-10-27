fn main() {
    let config = slint_build::CompilerConfiguration::new().with_style("fluent".into());
    slint_build::compile_with_config("ui/app.slint", config)
        .expect("Failed to compile the Slint UI");
    println!("cargo:rerun-if-env-changed=CHECK_UI_PREVIEWS");
    if std::env::var_os("CHECK_UI_PREVIEWS").is_some() {
        let root = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
        let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
        for name in [
            "viewer",
            "video",
            "library",
            "image-row",
            "thumbnail",
            "filmstrip",
            "info-panel",
            "viewer-header",
            "controls",
            "segmented-control",
            "key-hint",
            "interval-control",
            "toast",
            "date-rail",
            "scan-progress",
            "settings",
            "app-icon",
        ] {
            let path = root.join(format!("ui/previews/{name}-preview.slint"));
            println!("cargo:rerun-if-changed={}", path.display());
            slint_build::compile_with_output_path(
                path,
                out.join(format!("{name}-preview.rs")),
                slint_build::CompilerConfiguration::new().with_style("fluent".into()),
            )
            .expect("Failed to compile UI preview");
        }
    }
    // libmpv2-sys supplies bindings; pkg-config supplies nonstandard library paths
    // (notably Homebrew on macOS) as well as a clear missing-dependency error.
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc") {
        pkg_config::Config::new()
            .atleast_version("2.2")
            .probe("mpv")
            .expect("Install libmpv development files (mpv / libmpv-dev)");
    }
    println!("cargo:rerun-if-changed=native/raw.cpp");
    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("c++11")
        .file("native/raw.cpp")
        .file("native/jpeg.cpp");
    println!("cargo:rerun-if-changed=native/jpeg.cpp");
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        let lib = vcpkg::find_package("libraw").expect("Install LibRaw 0.22+ using vcpkg");
        for path in lib.include_paths {
            build.include(path);
        }
    } else {
        let lib = pkg_config::Config::new()
            .atleast_version("0.22")
            .probe("libraw")
            .expect("Install LibRaw 0.22+ development files (libraw / libraw-dev)");
        for path in lib.include_paths {
            build.include(path);
        }
    }
    let jpeg_paths = if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        vcpkg::find_package("libjpeg-turbo")
            .expect("Install libjpeg-turbo using vcpkg")
            .include_paths
    } else {
        pkg_config::Config::new()
            .probe("libturbojpeg")
            .expect("Install libjpeg-turbo development files")
            .include_paths
    };
    for path in jpeg_paths {
        build.include(path);
    }
    build.compile("photo_decoders");
}
