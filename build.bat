@echo off
setlocal

REM ==========================================
REM 🔍 STEP 0: Auto-Detect C++ Directory & VS
REM ==========================================

REM 1. หาตำแหน่งไฟล์ main.cpp อัตโนมัติ
if exist "app\main.cpp" (
    set CPP_DIR=app
    echo [INFO] Found source code in 'app' directory.
) else if exist "cpp\main.cpp" (
    set CPP_DIR=cpp
    echo [INFO] Found source code in 'cpp' directory.
) else if exist "main.cpp" (
    set CPP_DIR=.
    echo [INFO] Found source code in current directory.
) else (
    echo [ERROR] Could not find 'main.cpp'.
    echo Please ensure you have a folder named 'app' or 'cpp' containing main.cpp.
    pause
    exit /b 1
)

set OUT_NAME=DropTea.exe
set RUST_LIB=target\release\droptea_core.lib

REM 2. หา Visual Studio อัตโนมัติ
where cl.exe >nul 2>nul
if %errorlevel% equ 0 goto :START_BUILD

echo [INFO] cl.exe not found in PATH. Searching for Visual Studio...
set "VSWHERE=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe"

if not exist "%VSWHERE%" (
    echo [ERROR] 'vswhere.exe' not found. Please install Visual Studio with C++ workload.
    pause
    exit /b 1
)

for /f "usebackq tokens=*" %%i in (`"%VSWHERE%" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath`) do (
    set "VS_DIR=%%i"
)

if not defined VS_DIR (
    echo [ERROR] Visual Studio C++ installation not found!
    pause
    exit /b 1
)

echo [INFO] Found VS at: %VS_DIR%
call "%VS_DIR%\Common7\Tools\VsDevCmd.bat" -arch=x64 -host_arch=x64 -no_logo

:START_BUILD
REM ==========================================
REM 🚀 STEP 1: Build Rust Core
REM ==========================================
echo.
echo [1/2] Building Rust Core (Static Lib)...
cargo build --release --no-default-features --features ffi

if %errorlevel% neq 0 (
    echo [ERROR] Rust build failed!
    pause
    exit /b %errorlevel%
)

REM ==========================================
REM 🔨 STEP 2: Compile C++ & Link
REM ==========================================
echo.
echo [2/2] Compiling C++ and Linking...

cl.exe /nologo /EHsc /MD ^
    %CPP_DIR%\main.cpp ^
    %CPP_DIR%\wintoastlib.cpp ^
    /link %RUST_LIB% ^
    Kernel32.lib User32.lib Gdi32.lib WinSpool.lib Shell32.lib ^
    Ole32.lib OleAut32.lib Shlwapi.lib Propsys.lib Ws2_32.lib ^
    Advapi32.lib Bcrypt.lib Userenv.lib Iphlpapi.lib Secur32.lib ^
    Crypt32.lib Ntdll.lib ^
    /out:%OUT_NAME%

if %errorlevel% neq 0 (
    echo [ERROR] C++ build failed!
    pause
    exit /b %errorlevel%
)

echo.
echo ==========================================
echo BUILD SUCCESS!
echo Executable created: %CD%\%OUT_NAME%
echo ==========================================
pause