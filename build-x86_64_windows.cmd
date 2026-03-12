@echo off

set "TARGET_DIR=target\windows-openhardware"
set "DIST_DIR=dist\x86_64-pc-windows-msvc"
set "OUTPUT_EXE=%TARGET_DIR%\x86_64-pc-windows-msvc\release\USB-Screen.exe"
set "DIST_EXE=%DIST_DIR%\USB-Screen-openhardware.exe"

echo Installing nightly toolchain and target...
rustup install nightly
rustup target add x86_64-pc-windows-msvc --toolchain nightly
if errorlevel 1 exit /b %errorlevel%

echo Publishing NativeAOT LibreHardwareMonitor wrapper...
dotnet publish LibreHardwareMonitorNativeAot\LhmNativeAotWrapper.csproj -r win-x64 -c Release -o LibreHardwareMonitorNativeAot\publish
if errorlevel 1 exit /b %errorlevel%

echo Building OpenHardwareMonitor service source...
if not exist OpenHardwareMonitorService\publish mkdir OpenHardwareMonitorService\publish

set "MSBUILD_EXE="
for %%I in (MSBuild.exe) do set "MSBUILD_EXE=%%~$PATH:I"
if not defined MSBUILD_EXE if exist "%ProgramFiles%\Microsoft Visual Studio\2022\BuildTools\MSBuild\Current\Bin\MSBuild.exe" set "MSBUILD_EXE=%ProgramFiles%\Microsoft Visual Studio\2022\BuildTools\MSBuild\Current\Bin\MSBuild.exe"
if not defined MSBUILD_EXE if exist "%ProgramFiles%\Microsoft Visual Studio\2022\Community\MSBuild\Current\Bin\MSBuild.exe" set "MSBUILD_EXE=%ProgramFiles%\Microsoft Visual Studio\2022\Community\MSBuild\Current\Bin\MSBuild.exe"
if not defined MSBUILD_EXE if exist "%ProgramFiles%\Microsoft Visual Studio\2022\Professional\MSBuild\Current\Bin\MSBuild.exe" set "MSBUILD_EXE=%ProgramFiles%\Microsoft Visual Studio\2022\Professional\MSBuild\Current\Bin\MSBuild.exe"
if not defined MSBUILD_EXE if exist "%ProgramFiles%\Microsoft Visual Studio\2022\Enterprise\MSBuild\Current\Bin\MSBuild.exe" set "MSBUILD_EXE=%ProgramFiles%\Microsoft Visual Studio\2022\Enterprise\MSBuild\Current\Bin\MSBuild.exe"
if not defined MSBUILD_EXE if exist "%ProgramFiles(x86)%\Microsoft Visual Studio\2022\BuildTools\MSBuild\Current\Bin\MSBuild.exe" set "MSBUILD_EXE=%ProgramFiles(x86)%\Microsoft Visual Studio\2022\BuildTools\MSBuild\Current\Bin\MSBuild.exe"
if not defined MSBUILD_EXE if exist "%ProgramFiles(x86)%\Microsoft Visual Studio\2022\Community\MSBuild\Current\Bin\MSBuild.exe" set "MSBUILD_EXE=%ProgramFiles(x86)%\Microsoft Visual Studio\2022\Community\MSBuild\Current\Bin\MSBuild.exe"
if not defined MSBUILD_EXE if exist "%ProgramFiles(x86)%\Microsoft Visual Studio\2022\Professional\MSBuild\Current\Bin\MSBuild.exe" set "MSBUILD_EXE=%ProgramFiles(x86)%\Microsoft Visual Studio\2022\Professional\MSBuild\Current\Bin\MSBuild.exe"
if not defined MSBUILD_EXE if exist "%ProgramFiles(x86)%\Microsoft Visual Studio\2022\Enterprise\MSBuild\Current\Bin\MSBuild.exe" set "MSBUILD_EXE=%ProgramFiles(x86)%\Microsoft Visual Studio\2022\Enterprise\MSBuild\Current\Bin\MSBuild.exe"

if defined MSBUILD_EXE (
  echo Using MSBuild: %MSBUILD_EXE%
  "%MSBUILD_EXE%" OpenHardwareMonitorService\OpenHardwareMonitorService.csproj /p:Configuration=Release
) else (
  echo MSBuild not found, trying dotnet build...
  dotnet build OpenHardwareMonitorService\OpenHardwareMonitorService.csproj -c Release
)
if errorlevel 1 exit /b %errorlevel%

copy /Y OpenHardwareMonitorService\bin\Release\OpenHardwareMonitorService.exe OpenHardwareMonitorService\publish\OpenHardwareMonitorService.exe >nul
if errorlevel 1 exit /b %errorlevel%
copy /Y OpenHardwareMonitorService\bin\Release\OpenHardwareMonitorService.exe.config OpenHardwareMonitorService\publish\OpenHardwareMonitorService.exe.config >nul
if errorlevel 1 exit /b %errorlevel%

echo Building Windows version with editor + tray + nokhwa-webcam + usb-serial + openhardware...
rustup run nightly cargo zbuild --target-dir "%TARGET_DIR%" --target x86_64-pc-windows-msvc --no-default-features --features "editor,tray,nokhwa-webcam,usb-serial,openhardware"
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