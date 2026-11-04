# Install

Download the file for your system from the [releases page](https://github.com/martinlong1978/fluutcam/releases). If you can, check it before you run it: see [how to check a download](https://github.com/martinlong1978/fluutcam/blob/main/docs/CODE_SIGNING.md#checking-a-download).

You get two programs:

- **FluutCAM** (`fluutcam`), the window.
- **`fluut`**, the command-line program for calculating and posting without the window. See [Command line](Command-Line).

## Windows

**Installer (recommended).** Run `fluutcam-<version>-windows-x86_64.msi`.

- It installs for your user only. You do not need administrator rights.
- It installs to `%LOCALAPPDATA%\Programs\FluutCAM` and adds FluutCAM to the Start menu.
- To use `fluut` from a terminal, install from a command prompt with `msiexec /i fluutcam-<version>-windows-x86_64.msi ADDTOPATH=1`. That adds the folder to your user `PATH`.
- Installing a newer version replaces the older one.

**Portable zip.** Unzip `fluutcam-<version>-windows-x86_64.zip` to any folder. Run `fluutcam.exe` (the window) or `fluut.exe` (the command line).

The installer and the programs are signed. Windows shows SignPath Foundation as the publisher.

**Uninstall.** For the installer, open Settings, Apps, Installed apps, FluutCAM, Uninstall. This removes the program, the Start menu entry and the `PATH` entry. It keeps your settings and crash reports, which are in a different folder (`%LOCALAPPDATA%\FluutCAM`); delete that folder yourself to remove them. For the zip, delete the folder.

## macOS

1. Open `fluutcam-<version>-macos-universal.dmg` and drag FluutCAM to Applications.
2. The first time, right-click (or Control-click) FluutCAM in Applications and choose **Open**, then **Open** again in the dialog. On macOS 15 and later, if there is no Open button, open **System Settings, Privacy & Security** and choose **Open Anyway** for FluutCAM.

Apple has not notarised FluutCAM, so macOS asks once ([why](https://github.com/martinlong1978/fluutcam/blob/main/docs/CODE_SIGNING.md#macos)). The `fluut` program is in `FluutCAM.app/Contents/MacOS/`, and in `fluutcam-<version>-macos-universal-cli.tar.gz`.

**Uninstall.** Move FluutCAM from Applications to the Bin. To remove settings and crash reports too, delete `~/Library/Application Support/FluutCAM`.

## Linux

**AppImage.** Download `fluutcam-<version>-linux-x86_64.AppImage`, make it executable (`chmod +x`) and run it.

- It needs glibc 2.35 or later (Ubuntu 22.04 or newer) and FUSE 2 (`libfuse2`). Without FUSE, start it with `--appimage-extract-and-run` as the first argument.
- For the command-line program, start the AppImage through a link named `fluut`: `ln -s fluutcam-<version>-linux-x86_64.AppImage fluut`.
- The AppImage does not update itself. Download a newer one.

**Tarball.** Unpack `fluutcam-<version>-linux-x86_64.tar.gz` (glibc 2.35 or later) and run `bin/fluutcam` or `bin/fluut`. For a menu entry, copy `share/applications` and `share/icons` from the archive into `~/.local/share`.

**Uninstall.** Delete the AppImage (and the menu entry, if a tool added one). For the tarball, delete the unpacked folder and any `.desktop` file and icons you copied. To remove settings and crash reports too, delete `~/.local/share/fluutcam` (or `$XDG_DATA_HOME/fluutcam`).

FluutCAM changes no system settings and installs nothing outside the folders above.

## Updates

FluutCAM never updates itself. It can tell you that a newer version exists if you turn that on. See [Settings and updates](Settings-and-Updates).

## Next

[Interface tour](Interface-Tour)
