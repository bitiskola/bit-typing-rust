@echo off
setlocal EnableExtensions
cd /d "%~dp0"
rem The downloadable outputs folder also contains a copy of this script.
rem Prefer its parent source project when the resource folders are not here.
if not exist "assets\" (
    if exist "..\Cargo.toml" if exist "..\assets\" cd /d ".."
)

echo ============================================================
echo          bit-typing Windows build (Rust + NSIS)
echo ============================================================
echo.

set "PACKAGE_VERSION=%~1"
if "%PACKAGE_VERSION%"=="" set "PACKAGE_VERSION=2.1.0"
findstr /C:"version = \"%PACKAGE_VERSION%\"" Cargo.toml >nul 2>nul
if errorlevel 1 (
    echo ERROR: version '%PACKAGE_VERSION%' does not match Cargo.toml.
    echo Usage: build_win.bat [version]
    echo Pass the Cargo.toml version explicitly.
    goto :failed
)

where cargo >nul 2>nul
if errorlevel 1 (
    echo ERROR: Cargo was not found.
    echo Install the Rust toolchain from https://rustup.rs/ and retry.
    goto :failed
)

where makensis >nul 2>nul
if errorlevel 1 (
    echo ERROR: makensis was not found.
    echo Install NSIS 3 from https://nsis.sourceforge.io/ and retry
    echo ^(the installer directory must be on PATH^).
    goto :failed
)

if not exist "assets\favico.png" (
    echo ERROR: assets\favico.png is missing.
    echo Add the canonical bit-typing PNG icon before building.
    goto :failed
)
if not exist "assets\favico.ico" (
    echo ERROR: assets\favico.ico is missing.
    echo Regenerate it from assets\favico.png, e.g.:
    echo   python -c "from PIL import Image; Image.open('assets\favico.png').convert('RGBA').save('assets\favico.ico', sizes=[(16,16),(24,24),(32,32),(48,48),(64,64),(128,128),(256,256)])"
    goto :failed
)

echo Running unit tests...
cargo test --release
if errorlevel 1 goto :failed

if not exist "dist" mkdir "dist"
if errorlevel 1 goto :failed

echo.
echo [1/3] Building bit-typing.exe (Rust, icon embedded from assets\favico.ico)...
set "BIT_TYPING_FAVICO=%CD%\assets\favico.ico"
cargo build --release --bin bit-typing
if errorlevel 1 goto :failed
if not exist "target\release\bit-typing.exe" (
    echo ERROR: target\release\bit-typing.exe was not created.
    goto :failed
)
target\release\bit-typing.exe --verify-resources
if errorlevel 1 goto :failed
copy /Y "target\release\bit-typing.exe" "dist\bit-typing.exe" >nul
if errorlevel 1 goto :failed

echo.
echo [2/3] Staging portable files...
xcopy /E /I /Y "assets" "dist\assets\" >nul
if errorlevel 1 goto :failed
xcopy /E /I /Y "keyboards" "dist\keyboards\" >nul
if errorlevel 1 goto :failed
rem Lessons ship via git course packs (see details.json); the folder is still
rem staged so portable custom lessons keep working.
if not exist "courses\" mkdir "courses" >nul 2>nul
xcopy /E /I /Y "courses" "dist\courses\" >nul
if errorlevel 1 goto :failed
if exist "dist\courses\.gitkeep" del "dist\courses\.gitkeep" >nul 2>nul
xcopy /E /I /Y "lang" "dist\lang\" >nul
if errorlevel 1 goto :failed

echo.
echo [3/3] Building installer with NSIS...
if not exist "outputs" mkdir "outputs"
if errorlevel 1 goto :failed
makensis /DVERSION=%PACKAGE_VERSION% "/DAPP_ICON=%CD%\assets\favico.ico" installer.nsi
if errorlevel 1 goto :failed
if not exist "outputs\bit-typing-setup.exe" (
    echo ERROR: outputs\bit-typing-setup.exe was not created.
    goto :failed
)

echo.
echo ============================================================
echo Build completed successfully.
echo Installer: %CD%\outputs\bit-typing-setup.exe
echo Portable:  %CD%\dist\bit-typing.exe
echo.
echo Run the installer normally; it requests elevation itself
echo for per-machine installs and installs per-user otherwise.
echo ============================================================
echo.
pause
exit /b 0

:failed
echo.
echo ============================================================
echo BUILD FAILED. Review the error messages above.
echo ============================================================
echo.
pause
exit /b 1
