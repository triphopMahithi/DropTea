# DropTea !
## hackathon.gemini3.devpost
 [Hackathon ](https://gemini3.devpost.com/)

---

# 📦 DropTea - Windows Native Build Guide
This guide details the build process for the Windows Native implementation of DropTea. The architecture consists of a high-performance Rust Core (compiled as a Dynamic Link Library) and a native C++ Wrapper for OS integration (handling Notifications/Toast).

# 🛠 Prerequisites
- Before building, ensure you have the following installed:
- Rust Toolchain: (Stable channel) via rustup
- Visual Studio 2022: With "Desktop development with C++" workload (specifically MSVC v143+ build tools).
- Git: For cloning dependencies.

📂 Project Structure
```bash
DropTea/
├── core/                   # Rust Source Code (droptea_core)
│   ├── Cargo.toml
│   └── src/
├── windows/                # C++ Native Implementation
│   ├── main.cpp
│   ├── droptea_api.h
│   └── vendor/             # External libraries
│       └── wintoast/       # WinToast library
└── build.bat               # Automated build script
```

# 🚀 Build Instructions

**Method 1**: Automated Build (Recommended)
We recommend using the provided build script to automate the Rust compilation, linking, and assembly process.

1. Open x64 Native Tools Command Prompt for VS 2022.
2. Navigate to the project root.
3. Run the build script:

```bash
build.bat
```

**Method 2: Manual Build** 
If you need to debug specific steps, follow these manual instructions.

**1. Build the Rust Core (DLL)**
Navigate to the core directory and build the FFI feature set.

```bash
cd core
cargo build --release --no-default-features --features ffi
```

This will generate:

- `target/release/droptea_core.dll` (Runtime binary)
- `target/release/droptea_core.dll.lib` (Linker library)

**2. Prepare the C++ Workspace**
Create a distribution folder and copy the necessary artifacts.
```bash
cd ..
mkdir dist
copy core\target\release\droptea_core.dll dist\
copy core\target\release\droptea_core.dll.lib dist\
```

**3. Compile C++ Native App**
Compile the C++ application and link it against the Rust core.
```bash
cd windows
cl.exe /EHsc /std:c++17 /O2 main.cpp vendor/wintoast/wintoastlib.cpp ^
  /I vendor/wintoast ^
  /link ../dist/droptea_core.dll.lib user32.lib ole32.lib shlwapi.lib shell32.lib ^
  /out:../dist/DropTea.exe
```

```example
```bash
cl.exe /EHsc main.cpp wintoastlib.cpp /link droptea_core.dll.lib user32.lib ole32.lib shlwapi.lib shell32.lib /out:DropTea.exe
```
```
# 📦 Runtime Artifacts
After a successful build, your dist/ folder will be ready for deployment:
```bash
dist/
├── DropTea.exe             # Main Executable
├── droptea_core.dll        # Core Logic (Rust)
└── downloads/              # Default download directory (Auto-created)
```

# 💳 Credits
[1] M. Boujemaoui, "WinToast," GitHub repository. [Online]. Available: https://github.com/mohabouje/WinToast. [Accessed: Dec. 12, 2025].