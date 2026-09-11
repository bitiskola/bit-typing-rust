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

where python >nul 2>nul
if errorlevel 1 (
    echo ERROR: Python was not found.
    echo Install Python 3.11+ from https://www.python.org/downloads/ and retry.
    goto :failed
)

python -c "import customtkinter, PIL.Image, PyInstaller" >nul 2>nul
if errorlevel 1 (
    echo ERROR: Python packaging requirements are missing.
    echo Run: pip install customtkinter pillow pyinstaller pyinstaller-hooks-contrib
    echo and retry.
    goto :failed
)

if not exist "assets\favico.png" (
    echo ERROR: assets\favico.png is missing.
    echo Add the canonical bit-typing PNG icon before building.
    goto :failed
)

echo Checking default resources...
if exist "build_resources.py" (
    python build_resources.py check-source
    if errorlevel 1 goto :failed
)

echo Running unit tests...
cargo test --release
if errorlevel 1 goto :failed

if not exist "dist" mkdir "dist"

echo.
echo Generating EXE icon from assets\favico.png...
set "ICONFLAG="
python -c "from PIL import Image; im = Image.open('assets\\favico.png').convert('RGBA'); im.save('dist\\favico.ico', sizes=[(16,16),(24,24),(32,32),(48,48),(64,64),(128,128),(256,256)])" >nul 2>nul
if errorlevel 1 (
    echo WARNING: Could not generate favico.ico; executables will use the default icon.
) else (
    set "ICONFLAG=--icon dist\favico.ico"
)

echo.
echo [1/4] Building bit-typing.exe (Rust, icon embedded when favico.ico exists)...
if exist "dist\favico.ico" (
    set "BIT_TYPING_FAVICO=%CD%\dist\favico.ico"
) else (
    set "BIT_TYPING_FAVICO="
)
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
echo [2/4] Building uninstall.exe (Python uninstaller, PyInstaller)...
if not exist "dist\payload" mkdir "dist\payload"
copy /Y "dist\bit-typing.exe" "dist\payload\bit-typing.exe" >nul
python -m PyInstaller --noconfirm --clean --onefile --windowed --name uninstall %ICONFLAG% --collect-all customtkinter --add-data "assets;assets" --distpath dist --workpath build\tmp --specpath build uninstall.py
if errorlevel 1 goto :failed
if not exist "dist\uninstall.exe" (
    echo ERROR: dist\uninstall.exe was not created.
    goto :failed
)
copy /Y "dist\uninstall.exe" "dist\payload\uninstall.exe" >nul
if errorlevel 1 goto :failed

echo.
echo [3/4] Building bit-typing-setup.exe (Python installer, PyInstaller)...
echo       Payload: executables plus assets, keyboards, courses, and lang.
python -m PyInstaller --noconfirm --clean --onefile --windowed --name bit-typing-setup %ICONFLAG% --collect-all customtkinter --add-data "dist\payload\bit-typing.exe;payload" --add-data "dist\payload\uninstall.exe;payload" --add-data "assets;assets" --add-data "keyboards;keyboards" --add-data "courses;courses" --add-data "lang;lang" --distpath dist --workpath build\tmp --specpath build installer.py
if errorlevel 1 goto :failed
if not exist "dist\bit-typing-setup.exe" (
    echo ERROR: dist\bit-typing-setup.exe was not created.
    goto :failed
)

echo.
echo [4/4] Staging portable files and outputs...
copy /Y "dist\bit-typing.exe" "dist\payload\bit-typing.exe" >nul
xcopy /E /I /Y "assets" "dist\assets\" >nul
if errorlevel 1 goto :failed
xcopy /E /I /Y "keyboards" "dist\keyboards\" >nul
if errorlevel 1 goto :failed
xcopy /E /I /Y "courses" "dist\courses\" >nul
if errorlevel 1 goto :failed
xcopy /E /I /Y "lang" "dist\lang\" >nul
if errorlevel 1 goto :failed

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
