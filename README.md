# ReCircles for Windows

[ReCircles Website](recircles.rekosk.dev)

Native Windows recreation of **ReCircles (Linux)**. It isn't a 1:1 port because it has features which Linux port doesn't have and doesn't have some which Linux port has. That's why this port **is still in beta**, and there is a big possibility **it won't get updates from me as often as the Linux port**, because... I don't like windows lol...

I surely want to optimize this port even more tho, because thats my main focus - make something fast, but good looking too.

## Main differences between Windows and Linux versions

*Bold features are the ones which should get implemented into the other port (sooner or later)*

Linux:

- Has a Wifi/Bluetooth menu

Windows:

- **Has a better customization**

- Has an installer

- Has a daemon which listens for the keybinds 

- Has a native clipboard menu, even for pictures

*And probably more, but I genuinely don't remember everything and I'm not using AI for this file :D...*

## Featuring and technical parameters

It is featuring a hardware-accelerated Direct3D 11 / DirectComposition / Direct2D 1.1 / DirectWrite glass ring renderer, an ultra-lightweight hotkey & clipboard background daemon, custom ReCircles clipboard history manager, modular per-submenu DLLs, per-submenu custom global hotkeys, and a standalone interactive & silent Windows installer.

### Architecture

- **`recircles.exe`** (`crates/recircles-ui`): Pure floating circular glass ring & spring-morphing grid/list/input/color-picker launcher rendered with Direct3D 11 + DXGI (`FLIP_SEQUENTIAL`, premultiplied alpha) + DirectComposition (`WS_EX_NOREDIRECTIONBITMAP`, zero rectangular window behind the ring) + Direct2D 1.1 shape-clipped GPU Gaussian blur + `DwmFlush()` VSync frame pacing.
- **`recircles-daemon.exe`** (`crates/recircles-daemon`): Minimal Win32 `GetMessageW` event-driven daemon holding global `RegisterHotKey` shortcuts (Main Ring + per-submenu + custom menus), system tray controls, and `AddClipboardFormatListener` (when `Clipboard` is installed).
- **`submenus/recircles_*.dll`** (`crates/recircles-sub-*`): Optional built-in submenus (`files`, `apps`, `windows`, `clipboard`). If unselected in the installer, those DLLs are excluded (or deleted on modify) and never installed on disk. `Settings` is built directly into `recircles.exe` and cannot be unselected.
- **`recircles-setup.exe`** (`crates/recircles-installer`): Self-contained Windows installer embedding all binaries and submenu DLLs, letting users select installed submenus, theme (with live preview), and default or custom global/in-ring keybinds.

## Configuration

**Full configuration can be done trough a GUI interface.** If you really want to, you can do it yourself too:

Stored in `%LOCALAPPDATA%\recircles\recircles.json` (plus `themes/`, `menus/`, `usage`, `pins`, and `clipboard_history.json`).

### Submenu & Custom Keybinds Example

```json
{
  "keybinds": {
    "main_menu": "Ctrl+Space",
    "menu_keybinds": {
      "clipboard": "Ctrl+Alt+v",
      "apps": "Ctrl+Alt+a"
    }
  }
}
```

### Documentation

[You can find the documentation here.](docs.rekosk.dev/recircles) It is aiming mostly on the Linux configuration *(for now)*, but **everything should be crossplatform** - the only thing that really won't be right are the **directory paths**.

*Full documentation for Windows is planned in the near future.*

## Build (Native Visual Studio MSVC)

Requires **Rust (`x86_64-pc-windows-msvc`)** and **Visual Studio 2022 C++ Build Tools + Windows 10/11 SDK**. *If you have those installed, you can skip to [3. Build ReCircles & Standalone Installer](#3-build-recircles--standalone-installer).*

### 1. Install Visual Studio 2022 C++ Build Tools & Windows SDK

You can install the required C++ compiler (`link.exe` / MSVC) and Windows SDK via **winget**:

```powershell
winget install Microsoft.VisualStudio.2022.BuildTools --override "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
```

---

### 2. Install Rust (`x86_64-pc-windows-msvc`)

```powershell
winget install Rustlang.Rustup
```

#### Verify & Configure Toolchain

Restart your PowerShell terminal so `%USERPROFILE%\.cargo\bin` is on your `PATH`, then ensure the native MSVC toolchain is active:

```powershell
rustup toolchain install stable-x86_64-pc-windows-msvc
rustup default stable-x86_64-pc-windows-msvc
rustc -vV
```

*(Verify that the `host:` line outputs `x86_64-pc-windows-msvc`.)*

### 3. Build ReCircles & Standalone Installer

Run the build script from the `ReCircles_Windows` directory:

```powershell
.\build_windows.ps1
```

Once finished, the compiled binaries and installer are staged in `target\dist\`:

- `target\dist\recircles-setup.exe` — Standalone interactive & silent installer
- `target\dist\recircles.exe` — Main circular ring UI launcher
- `target\dist\recircles-daemon.exe` — Background hotkey & clipboard daemon
- `target\dist\submenus\recircles_*.dll` — Optional submenu plugins

## License

ReCircles is licensed under the [GNU General Public License, version 3.0 only](LICENSE).

Copyright © 2026 Andrej "Reko" Revák.
