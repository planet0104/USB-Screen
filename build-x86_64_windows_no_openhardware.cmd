@echo off

set "TARGET_DIR=target\windows-lhm-only"
set "DIST_DIR=dist\x86_64-pc-windows-msvc"
set "OUTPUT_EXE=%TARGET_DIR%\x86_64-pc-windows-msvc\release\USB-Screen.exe"
set "DIST_EXE=%DIST_DIR%\USB-Screen-lhm-only.exe"

echo Installing nightly toolchain and target...
rustup install nightly
rustup target add x86_64-pc-windows-msvc --toolchain nightly
if errorlevel 1 exit /b %errorlevel%

echo Publishing NativeAOT LibreHardwareMonitor wrapper...
dotnet publish LibreHardwareMonitorNativeAot\LhmNativeAotWrapper.csproj -r win-x64 -c Release -o LibreHardwareMonitorNativeAot\publish
if errorlevel 1 exit /b %errorlevel%

echo Building Windows version with editor + tray + nokhwa-webcam + usb-serial no openhardware...
rustup run nightly cargo zbuild --target-dir "%TARGET_DIR%" --target x86_64-pc-windows-msvc --no-default-features --features "editor,tray,nokhwa-webcam,usb-serial"
if errorlevel 1 exit /b %errorlevel%

if not exist "%DIST_DIR%" mkdir "%DIST_DIR%"
if errorlevel 1 exit /b %errorlevel%

copy /Y "%OUTPUT_EXE%" "%DIST_EXE%" >nul
if errorlevel 1 exit /b %errorlevel%

echo.
echo ============================================
echo Build completed!
echo Output: %DIST_EXE%
echo ============================================
echo.
