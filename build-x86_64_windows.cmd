@echo off
::: ============================================
::: Windows x86_64 编译脚本
::: 以管理员身份运行 需要读取硬件信息
::: ============================================
:::
::: 自动启用 features: editor tray nokhwa-webcam usb-serial
::: 无需手动修改 Cargo.toml
:::
::: ============================================

::: 安装 nightly 工具链和目标
echo Installing nightly toolchain and target...
rustup install nightly
rustup target add x86_64-pc-windows-msvc --toolchain nightly
if errorlevel 1 exit /b %errorlevel%

::: 发布 NativeAOT 硬件包装器
echo Publishing NativeAOT LibreHardwareMonitor wrapper...
dotnet publish LibreHardwareMonitorNativeAot\LhmNativeAotWrapper.csproj -r win-x64 -c Release -o LibreHardwareMonitorNativeAot\publish
if errorlevel 1 exit /b %errorlevel%

::: 使用 nightly + cargo zbuild 编译，指定 features
echo Building Windows version with editor + tray + nokhwa-webcam + usb-serial...
rustup run nightly cargo zbuild --target x86_64-pc-windows-msvc --no-default-features --features "editor,tray,nokhwa-webcam,usb-serial"
if errorlevel 1 exit /b %errorlevel%

echo.
echo ============================================
echo Build completed!
echo Output: target/x86_64-pc-windows-msvc/release/USB-Screen.exe
echo ============================================
