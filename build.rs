fn main() {
    if std::env::var_os("CARGO_CFG_WINDOWS").is_none() {
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

    embed_resource::compile("icon-resource.rc", embed_resource::NONE);
}
