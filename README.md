# BIT Typing — Rust

Public source code for **BIT Typing**, a Rust-based version of the application.

## 🛠️ Building

### Windows

#### Requirements

* [Rust & Cargo](https://www.rust-lang.org/tools/install)
* [Python 3.11+](https://www.python.org/downloads/)

#### 1. Install the Python dependencies

```bash
pip install customtkinter pillow pyinstaller pyinstaller-hooks-contrib
```

#### 2. Run the Windows build script

```bat
./build_win.bat
```

Once the build finishes, the compiled output can be found in:

```text
outputs/
```

---

### Linux

#### Requirements

* Rust & Cargo

#### 1. Run the Linux build script

```bash
./build_linux.sh
```

Once the build finishes, the compiled output can be found in:

```text
outputs/
```

## 📁 Output

Build artifacts for both platforms are placed in the `outputs/` directory.

```text
outputs/
├── ...
└── ...
```

## 📜 License

See the repository's license file for details.
