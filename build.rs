fn main() {
    let style = "fluent-dark";
    slint_build::compile_with_config(
        "view/main.slint",
        slint_build::CompilerConfiguration::new().with_style(style.into()),
    )
    .unwrap();

    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("images/monitor.ico");
        res.compile().unwrap();
    }
}
