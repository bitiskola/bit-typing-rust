@echo off
setlocal EnableExtensions
cd /d "%~dp0"
rem The downloadable outputs folder also contains a copy of this script.
rem Prefer its parent source project when the resource folders are not here.
if not exist "courses\" (
    if exist "..\Cargo.toml" if exist "..\courses\" cd /d ".."
)

echo ============================================================
echo              bit-typing Windows build (Rust)
echo ============================================================
echo.

where cargo >nul 2>nul
if errorlevel 1 (
    echo ERROR: Cargo was not found.
    echo Install the Rust toolchain from https://rustup.rs/ and retry.
    goto :failed
)

if not exist "assets\favico.png" (
    echo ERROR: assets\favico.png is missing.
    echo Add the canonical bit-typing PNG icon before building.
    goto :failed
)

if not exist "assets\favico.ico" (
    echo NOTE: assets\favico.ico is missing, the EXE icon will be embedded
    echo from assets\favico.png at packaging time if Pillow is available.
)

echo Checking default resources...
if exist "build_resources.py" (
    python build_resources.py check-source
    if errorlevel 1 goto :failed
)

echo Running unit tests...
cargo test --release
if errorlevel 1 goto :failed

echo.
echo [1/3] Building bit-typing.exe...
cargo build --release --bin bit-typing
if errorlevel 1 goto :failed
if not exist "target\release\bit-typing.exe" (
    echo ERROR: target\release\bit-typing.exe was not created.
    goto :failed
)
target\release\bit-typing.exe --verify-resources
if errorlevel 1 goto :failed

echo.
echo [2/3] Building uninstall.exe...
cargo build --release --bin bit-typing-uninstall
if errorlevel 1 goto :failed
if not exist "target\release\bit-typing-uninstall.exe" (
    echo ERROR: target\release\bit-typing-uninstall.exe was not created.
    goto :failed
)

echo.
echo [3/3] Building the self-contained installer...
cargo build --release --bin bit-typing-setup
if errorlevel 1 goto :failed
if not exist "target\release\bit-typing-setup.exe" (
    echo ERROR: target\release\bit-typing-setup.exe was not created.
    goto :failed
)

if not exist "dist" mkdir "dist"
copy /Y "target\release\bit-typing.exe" "dist\bit-typing.exe" >nul
if errorlevel 1 goto :failed
copy /Y "target\release\bit-typing-uninstall.exe" "dist\uninstall.exe" >nul
if errorlevel 1 goto :failed
copy /Y "target\release\bit-typing-setup.exe" "dist\bit-typing-setup.exe" >nul
if errorlevel 1 goto :failed
rem Runtime payload layout, mirroring PyInstaller's --add-binary payload step:
rem the setup binary looks for payload\bit-typing.exe next to itself first.
if not exist "dist\payload" mkdir "dist\payload"
copy /Y "dist\bit-typing.exe" "dist\payload\bit-typing.exe" >nul
copy /Y "dist\uninstall.exe" "dist\payload\uninstall.exe" >nul

if not exist "outputs" mkdir "outputs"
copy /Y "dist\bit-typing-setup.exe" "outputs\bit-typing-setup.exe" >nul
if errorlevel 1 goto :failed

echo.
echo ============================================================
echo Build completed successfully.
echo Installer: %CD%\outputs\bit-typing-setup.exe
echo Portable:  %CD%\dist\bit-typing.exe
echo.
echo Run normally for a per-user installation, or right-click the
echo installer and choose "Run as administrator" for all users.
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
