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

**Python (Development)**
```bash
maturin develop
```

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

```bash
cl.exe /EHsc /MD main.cpp wintoastlib.cpp /link ..\target\release\droptea_core.lib Kernel32.lib User32.lib Gdi32.lib WinSpool.lib Shell32.lib Ole32.lib OleAut32.lib Shlwapi.lib Propsys.lib Ws2_32.lib Advapi32.lib Bcrypt.lib Userenv.lib Iphlpapi.lib Secur32.lib Crypt32.lib Ntdll.lib /out:DropTea.exe
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

# 🧱 Firewall Configuration

This utility script automates Windows Firewall configuration to ensure **DropTea** can discover devices and transfer files smoothly, especially when using a **Mobile Hotspot** or connecting across different subnets.

## 🧐 Why is this script needed?
By default, Windows Firewall blocks incoming connections on networks classified as **"Public"** (which includes most Mobile Hotspots). This causes common issues such as:
* ❌ Devices cannot find each other (mDNS Discovery fails).
* ❌ File transfers fail or time out.

This script creates a specific **allow rule** for `droptea_core.exe`, permitting it to accept connections from any network profile (Public/Private) without exposing your entire system.

---

## 🚀 Quick Start

**1. Build the Project**
Ensure you have compiled your Rust project so that the `.exe` file exists.

```PowerShell
cargo build
cargo build --release
```

**2. Run the Script (Administrator)**
Right-click setup_firewall.ps1 and select "Run with PowerShell".
Alternatively, open a Terminal as Administrator and run:
```PowerShell
powershell -ExecutionPolicy Bypass -File .\setup\setup_firewall.ps1
```

**3. Verification**
To confirm that the firewall rule has been applied correctly, run this command in PowerShell:
```PowerShell
Get-NetFirewallRule -DisplayName "DropTea Allow All" | Select-Object Enabled, Profile, Action, Direction
```

**Expected Output** :
- Enabled: True
- Profile: Any (or a numeric value representing all profiles)
- Action: Allow
- Direction: Inbound



# 💳 Credits
[1] M. Boujemaoui, "WinToast," GitHub repository. [Online]. Available: https://github.com/mohabouje/WinToast. [Accessed: Dec. 12, 2025].