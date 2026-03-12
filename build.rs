use std::{
    env, fs,
    path::{Path, PathBuf},
};

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
        fs::write(
            output,
            "pub const EMBEDDED_LHM_WRAPPER: &[u8] = &[];\npub const EMBEDDED_OHM_SERVICE_EXE: &[u8] = &[];\npub const EMBEDDED_OHM_SERVICE_CONFIG: &[u8] = &[];\npub const EMBEDDED_OHM_OPENHARDWAREMONITOR_LIB: &[u8] = &[];\npub const EMBEDDED_OHM_NEWTONSOFT_JSON: &[u8] = &[];\n",
        )
            .expect("failed to write non-windows wrapper source");
        return;
    }

    let wrappers = [EmbeddedBinary {
            constant_name: "EMBEDDED_LHM_WRAPPER",
            error_message: "缺少 LibreHardwareMonitor NativeAOT 包装器 DLL 请先执行 dotnet publish LibreHardwareMonitorNativeAot/LhmNativeAotWrapper.csproj -r win-x64 -c Release -o LibreHardwareMonitorNativeAot/publish",
            tracked_paths: &[
                "LibreHardwareMonitorNativeAot/LhmNativeAotWrapper.csproj",
                "LibreHardwareMonitorNativeAot/HardwareWrapper.cs",
                "LibreHardwareMonitorNativeAot/hardware_wrapper.h",
                "LibreHardwareMonitor.NET.10/LibreHardwareMonitorLib.dll",
            ],
            candidates: &[
                "LibreHardwareMonitorNativeAot/publish/LhmNativeAotWrapper.dll",
                "LibreHardwareMonitorNativeAot/bin/Release/net10.0/win-x64/publish/LhmNativeAotWrapper.dll",
            ],
        },
        EmbeddedBinary {
            constant_name: "EMBEDDED_OHM_SERVICE_EXE",
            error_message: "缺少 OpenHardwareMonitor 补充服务 EXE 文件 请先构建 OpenHardwareMonitorService 或补齐 OpenHardwareMonitorService/publish/OpenHardwareMonitorService.exe",
            tracked_paths: &[
                "OpenHardwareMonitorService/OpenHardwareMonitorService.csproj",
                "OpenHardwareMonitorService/Program.cs",
                "OpenHardwareMonitorService/App.config",
                "OpenHardwareMonitorService/app.manifest",
                "OpenHardwareMonitorService/libs/OpenHardwareMonitorLib.dll",
                "OpenHardwareMonitorService/libs/Newtonsoft.Json.dll",
                "OpenHardwareMonitorService/publish/OpenHardwareMonitorService.exe",
            ],
            candidates: &[
                "OpenHardwareMonitorService/publish/OpenHardwareMonitorService.exe",
                "OpenHardwareMonitorService/bin/Release/OpenHardwareMonitorService.exe",
            ],
        },
        EmbeddedBinary {
            constant_name: "EMBEDDED_OHM_SERVICE_CONFIG",
            error_message: "缺少 OpenHardwareMonitor 补充服务配置文件 请先构建 OpenHardwareMonitorService 或补齐 OpenHardwareMonitorService/publish/OpenHardwareMonitorService.exe.config",
            tracked_paths: &[
                "OpenHardwareMonitorService/OpenHardwareMonitorService.csproj",
                "OpenHardwareMonitorService/App.config",
                "OpenHardwareMonitorService/publish/OpenHardwareMonitorService.exe.config",
            ],
            candidates: &[
                "OpenHardwareMonitorService/publish/OpenHardwareMonitorService.exe.config",
                "OpenHardwareMonitorService/bin/Release/OpenHardwareMonitorService.exe.config",
            ],
        },
        EmbeddedBinary {
            constant_name: "EMBEDDED_OHM_OPENHARDWAREMONITOR_LIB",
            error_message: "缺少 OpenHardwareMonitorLib.dll 请补齐 OpenHardwareMonitorService/libs/OpenHardwareMonitorLib.dll",
            tracked_paths: &[
                "OpenHardwareMonitorService/libs/OpenHardwareMonitorLib.dll",
            ],
            candidates: &[
                "OpenHardwareMonitorService/libs/OpenHardwareMonitorLib.dll",
            ],
        },
        EmbeddedBinary {
            constant_name: "EMBEDDED_OHM_NEWTONSOFT_JSON",
            error_message: "缺少 Newtonsoft.Json.dll 请补齐 OpenHardwareMonitorService/libs/Newtonsoft.Json.dll",
            tracked_paths: &[
                "OpenHardwareMonitorService/libs/Newtonsoft.Json.dll",
            ],
            candidates: &[
                "OpenHardwareMonitorService/libs/Newtonsoft.Json.dll",
            ],
        },
    ];

    let mut content = String::new();
    for wrapper in wrappers {
        content.push_str(&build_embedded_constant(wrapper));
    }

    fs::write(output, content).expect("failed to write embedded wrapper source");
}

struct EmbeddedBinary<'a> {
    constant_name: &'a str,
    error_message: &'a str,
    tracked_paths: &'a [&'a str],
    candidates: &'a [&'a str],
}

fn build_embedded_constant(wrapper: EmbeddedBinary<'_>) -> String {
    for tracked_path in wrapper.tracked_paths {
        println!("cargo:rerun-if-changed={tracked_path}");
    }

    for candidate in wrapper.candidates {
        println!("cargo:rerun-if-changed={candidate}");
    }

    wrapper
        .candidates
        .iter()
        .map(Path::new)
        .find(|path| path.exists())
        .map(|path| {
            let absolute = path
                .canonicalize()
                .expect("failed to canonicalize hardware wrapper dll path");
            format!(
                "pub const {}: &[u8] = include_bytes!(r#\"{}\"#);\n",
                wrapper.constant_name,
                absolute.display()
            )
        })
        .unwrap_or_else(|| format!("compile_error!(\"{}\");\n", wrapper.error_message))
}
