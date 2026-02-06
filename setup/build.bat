@echo off
setlocal enabledelayedexpansion

REM ==========================================
REM 📋 MAIN MENU
REM ==========================================
cls
echo ==========================================
echo      DROPTEA BUILD SYSTEM (ALL-IN-ONE)
echo ==========================================
echo.
echo  1. Build C++ Host (DropTea.exe)
echo  2. Build Python Package (.whl)
echo  3. Build EVERYTHING (Recommended)
echo.
set /p choice="Select option (1-3): "

if "%choice%"=="1" goto :SETUP_CPP
if "%choice%"=="2" goto :BUILD_PYTHON
if "%choice%"=="3" goto :BUILD_PYTHON_THEN_CPP
echo Invalid choice. Exiting.
exit /b 1

:BUILD_PYTHON_THEN_CPP
call :DO_PYTHON_BUILD
if %errorlevel% neq 0 exit /b %errorlevel%
goto :SETUP_CPP

:BUILD_PYTHON
call :DO_PYTHON_BUILD
if %errorlevel% neq 0 exit /b %errorlevel%
goto :FINISH

REM ==========================================
REM 🐍 PYTHON BUILD ROUTINE
REM ==========================================
:DO_PYTHON_BUILD
echo.
echo ==========================================
echo [PYTHON] Building Python Package...
echo ==========================================

REM 1. Check for Maturin
where maturin >nul 2>nul
if %errorlevel% neq 0 (
    echo [ERROR] 'maturin' not found.
    echo.
    echo Please run 'setup.bat' first to install build tools!
    echo.
    pause
    exit /b 1
)

REM 2. Build Release Wheel
maturin build --release

if %errorlevel% neq 0 (
    echo [ERROR] Python build failed!
    exit /b %errorlevel%
)

echo [SUCCESS] Python wheels created in 'target/wheels'
exit /b 0

REM ==========================================
REM ⚙️ C++ SETUP & BUILD ROUTINE
REM ==========================================
:SETUP_CPP
echo.
echo ==========================================
echo [C++] Preparing to build Host Application...
echo ==========================================

REM 1. Auto-Detect C++ Source Directory
if exist "app\main.cpp" (
    set CPP_DIR=app
    echo [INFO] Found C++ source in 'app' directory.
) else if exist "cpp\main.cpp" (
    set CPP_DIR=cpp
    echo [INFO] Found C++ source in 'cpp' directory.
) else if exist "main.cpp" (
    set CPP_DIR=.
    echo [INFO] Found C++ source in current directory.
) else (
    echo [ERROR] Could not find 'main.cpp'.
    echo Please ensure 'main.cpp' exists in 'app', 'cpp', or root folder.
    pause
    exit /b 1
)

set OUT_NAME=DropTea.exe
set RUST_LIB=target\release\droptea_core.lib

REM 2. Auto-Detect Visual Studio
where cl.exe >nul 2>nul
if %errorlevel% equ 0 goto :START_CPP_BUILD

echo [INFO] cl.exe not found. Searching for Visual Studio...
set "VSWHERE=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe"

if not exist "%VSWHERE%" (
    echo [ERROR] 'vswhere.exe' not found. Please install Visual Studio (C++ Workload).
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

:START_CPP_BUILD
REM 3. Build Rust Core (Static Lib for C++)
echo.
echo [1/2] Building Rust Core (Static Lib for C++)...
cargo build --release --no-default-features --features ffi

if %errorlevel% neq 0 (
    echo [ERROR] Rust Static Lib build failed!
    pause
    exit /b %errorlevel%
)

REM 4. Compile C++ & Link
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
echo [SUCCESS] DropTea.exe created successfully.

:FINISH
echo.
echo ==========================================
echo  ALL TASKS COMPLETED!
echo ==========================================
pause