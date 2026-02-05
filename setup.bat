@echo off
setlocal

echo ==========================================
echo      DROPTEA DEVELOPMENT SETUP
echo ==========================================
echo.

REM 1. เช็ค Python
where python >nul 2>nul
if %errorlevel% neq 0 (
    echo [ERROR] Python not found! Please install Python 3.8+ first.
    pause
    exit /b 1
)

REM 2. อัปเดต pip
echo [1/3] Updating pip...
python -m pip install --upgrade pip

REM 3. ติดตั้ง Build Tool (Maturin)
echo.
echo [2/3] Installing Build Tools (Maturin)...
pip install maturin

REM 4. ติดตั้ง Library อื่นๆ จาก requirements.txt (ถ้ามี)
if exist requirements.txt (
    echo.
    echo [3/3] Installing Project Dependencies...
    pip install -r requirements.txt
) else (
    echo.
    echo [INFO] requirements.txt not found, skipping dependencies.
)

echo.
echo ==========================================
echo      SETUP COMPLETED SUCCESSFULLY!
echo ==========================================
echo You can now run 'build.bat' to compile the project.
pause