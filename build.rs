fn main() {
    generate_linux_bindings();

    embed_resource::compile("icon-resource.rc", embed_resource::NONE);
}

#[cfg(target_os = "linux")]
fn generate_linux_bindings() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("linux") {
        return;
    }

    let bindings = bindgen::builder()
        .header_contents(
            "bindings.h",
            "
        #include <linux/input.h>
        #include <linux/input-event-codes.h>
    ",
        )
        .generate()
        .expect("failed to generate bindings for linux/input.h");

    bindings
        .write_to_file(format!(
            "{}/input_bindings.rs",
            std::env::var("OUT_DIR").expect("OUT_DIR not set")
        ))
        .expect("Failed to write binds to outdir");
}

#[cfg(not(target_os = "linux"))]
fn generate_linux_bindings() {}
