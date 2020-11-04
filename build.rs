use cmake::Config;


fn main() {
    let rtaudio = Config::new("rtaudio")
        .generator("Unix Makefiles")
        .build();

    println!("cargo:rustc-link-search=native={}/build", rtaudio.display());
    println!("cargo:rustc-link-lib=dylib=crtaudio");
}
