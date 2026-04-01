fn main() {
    slint_build::compile("src/app.slint").unwrap();

    #[cfg(windows)]
    embed_resource::compile("embed_resources.rc");
}
