@echo off
rem Build the PhotoCraft-Spektrafilm launcher with MSVC (VS2022 cl.exe + Windows SDK rc.exe).
rem Output lands in packaging\spektra\launcher\PhotoCraft-Spektrafilm.exe (copy next to photocraft.exe).
setlocal
set "HERE=%~dp0"
set "VS_DIR="
set "VSWHERE=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe"
if exist "%VSWHERE%" (
    for /f "usebackq tokens=*" %%I in (`"%VSWHERE%" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath`) do set "VS_DIR=%%I"
)
if not defined VS_DIR (
    echo [error] Visual Studio C++ x64 toolset not found. Run from a VS x64 Native Tools Command Prompt.
    exit /b 1
)
call "%VS_DIR%\VC\Auxiliary\Build\vcvars64.bat" || exit /b 1
pushd "%HERE%"
rc /nologo /fo launcher.res launcher.rc || exit /b 1
cl /nologo /O2 /W4 /Fe:"PhotoCraft-Spektrafilm.exe" launcher.c launcher.res /link /SUBSYSTEM:WINDOWS /ENTRY:mainCRTStartup user32.lib kernel32.lib
if errorlevel 1 exit /b 1
del /q launcher.obj launcher.res >nul 2>&1
popd
echo BUILD_OK
