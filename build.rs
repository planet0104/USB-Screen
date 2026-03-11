use std::{env, fs, path::{Path, PathBuf}};

fn main() {
    let style = "fluent-dark";
    slint_build::compile_with_config(
        "view/main.slint",
        slint_build::CompilerConfiguration::new().with_style(style.into()),
    )
    .unwrap();

    generate_hardware_wrapper_bytes();

    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("images/monitor.ico");
        res.compile().unwrap();
    }
}

fn generate_hardware_wrapper_bytes() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR not found"));
    let output = out_dir.join("lhm_wrapper_bytes.rs");

    println!("cargo:rerun-if-env-changed=CARGO_CFG_WINDOWS");

    if env::var_os("CARGO_CFG_WINDOWS").is_none() {
        fs::write(output, "pub const EMBEDDED_LHM_WRAPPER: &[u8] = &[];\n")
            .expect("failed to write non-windows wrapper source");
        return;
    }

    let candidates = [
        Path::new("LibreHardwareMonitorNativeAot/publish/LhmNativeAotWrapper.dll"),
        Path::new("LibreHardwareMonitorNativeAot/bin/Release/net10.0/win-x64/publish/LhmNativeAotWrapper.dll"),
    ];

    println!("cargo:rerun-if-changed=LibreHardwareMonitorNativeAot/LhmNativeAotWrapper.csproj");
    println!("cargo:rerun-if-changed=LibreHardwareMonitorNativeAot/HardwareWrapper.cs");
    println!("cargo:rerun-if-changed=LibreHardwareMonitorNativeAot/hardware_wrapper.h");
    println!("cargo:rerun-if-changed=LibreHardwareMonitor.NET.10/LibreHardwareMonitorLib.dll");

    for candidate in &candidates {
        println!("cargo:rerun-if-changed={}", candidate.display());
    }

    let content = candidates
        .iter()
        .find(|path| path.exists())
        .map(|path| {
            let absolute = path
                .canonicalize()
                .expect("failed to canonicalize hardware wrapper dll path");
            format!(
                "pub const EMBEDDED_LHM_WRAPPER: &[u8] = include_bytes!(r#\"{}\"#);\n",
                absolute.display()
            )
        })
        .unwrap_or_else(|| {
            "compile_error!(\"缺少 NativeAOT 包装器 DLL 请先执行 dotnet publish LibreHardwareMonitorNativeAot/LhmNativeAotWrapper.csproj -r win-x64 -c Release -o LibreHardwareMonitorNativeAot/publish\");\n".to_string()
        });

    fs::write(output, content).expect("failed to write embedded wrapper source");
}
