# BIT Typing — Rust

Public source code for **BIT Typing**, a Rust-based version of the application.

## 🛠️ Building

### Windows

#### Requirements

* [Rust & Cargo](https://www.rust-lang.org/tools/install)
* [NSIS 3](https://nsis.sourceforge.io/) (`makensis` must be on `PATH`)

#### 1. Run the Windows build script

```bat
./build_win.bat [version]
```

The optional version defaults to the `Cargo.toml` version and must match it.
The script runs the test suite, builds `bit-typing.exe` (with the embedded
`assets/favico.ico` icon), stages a portable `dist/` folder, and compiles
`installer.nsi` with NSIS. Run the installer normally: it installs per-machine
when elevated and per-user otherwise.

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
