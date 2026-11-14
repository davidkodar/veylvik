fn main() {
    println!("cargo:rerun-if-changed=src/ocio.cpp");
    let ocio = pkg_config::Config::new()
        .atleast_version("2.5")
        .probe("OpenColorIO")
        .expect("OpenColorIO 2.5 development files are required");
    let mut build = cc::Build::new();
    build.cpp(true).std("c++17").file("src/ocio.cpp");
    for path in ocio.include_paths {
        build.include(path);
    }
    build.compile("veylvik_ocio");
}
