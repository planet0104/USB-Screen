@echo off

echo Building OpenHardwareMonitorService...

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

echo.
echo ============================================
echo Build completed!
echo EXE: OpenHardwareMonitorService\publish\OpenHardwareMonitorService.exe
echo CONFIG: OpenHardwareMonitorService\publish\OpenHardwareMonitorService.exe.config
echo ============================================
echo.