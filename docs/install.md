# Installing FluutCAM

Download the file for your platform from the
[releases page](https://github.com/martinlong1978/fluutcam/releases). Check it first if you
can ([Code signing policy](CODE_SIGNING.md#checking-a-download)).

## Windows

**Installer (recommended).** Run `fluutcam-<version>-windows-x86_64.msi`. It installs for
your user only (no administrator rights needed) to `%LOCALAPPDATA%\Programs\FluutCAM` and adds
a FluutCAM entry to the Start menu. To use `fluut` from a terminal, install from a command
prompt with `msiexec /i fluutcam-<version>-windows-x86_64.msi ADDTOPATH=1`, which adds the
folder to your user `PATH`. Installing a newer version replaces the older one.

**Portable zip.** Unzip `fluutcam-<version>-windows-x86_64.zip` to a folder of your choice and
run `fluutcam.exe` (the application) or `fluut.exe` (the command line).

The installer and the executables are signed; Windows shows SignPath Foundation as the
publisher.

**Uninstall:** for the installer, use **Settings, Apps, Installed apps, FluutCAM, Uninstall**
(or `msiexec /x` with the same MSI). This removes the program, the Start menu entry and the
`PATH` entry, and **keeps your settings and crash reports** in `%LOCALAPPDATA%\FluutCAM` (a
different folder); delete that folder yourself to remove them. For the zip, delete the folder.

## macOS

1. Open `fluutcam-<version>-macos-universal.dmg` and drag FluutCAM to Applications.
2. The first time, right-click (or Control-click) FluutCAM in Applications and choose
   **Open**, then **Open** again in the dialog. On macOS 15 and later, if there is no Open
   button, open **System Settings → Privacy & Security** and choose **Open Anyway** for
   FluutCAM.

FluutCAM is not notarised by Apple, so Gatekeeper asks once
([why](CODE_SIGNING.md#macos)). The command line `fluut` is in
`FluutCAM.app/Contents/MacOS/`, and in `fluutcam-<version>-macos-universal-cli.tar.gz`.

**Uninstall:** move FluutCAM from Applications to the Bin. To remove settings and crash
reports too, delete `~/Library/Application Support/FluutCAM`.

## Linux

**AppImage.** Download `fluutcam-<version>-linux-x86_64.AppImage`, make it executable
(`chmod +x`) and run it. It needs glibc 2.35 or later (Ubuntu 22.04 or newer) and, as any
AppImage, FUSE 2 (`libfuse2`); without it, run it with
`--appimage-extract-and-run` as the first argument. For the command line program, start the
AppImage through a link named `fluut` (`ln -s fluutcam-<version>-linux-x86_64.AppImage fluut`).
The AppImage does not update itself; download a newer one.

**Flatpak.** Download `fluutcam-<version>-linux-x86_64.flatpak` and install it for your user
(this needs [Flatpak](https://flatpak.org/setup/) and the Flathub remote, which supplies the
freedesktop 24.08 runtime):

```
flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
flatpak install --user fluutcam-<version>-linux-x86_64.flatpak
flatpak run io.github.martinlong1978.FluutCAM
```

FluutCAM then also appears in the application menu. For the command line program use
`flatpak run --command=fluut io.github.martinlong1978.FluutCAM --version`. The Flatpak runs in
a sandbox with a GPU, a display and nothing else: no network (so the optional update check
cannot run; install a newer `.flatpak` to update) and no access to your files except those you
pick in the file dialogs. A project that refers to a model file beside it needs that folder
opened too: either grant it once with
`flatpak override --user --filesystem=~/path/to/folder io.github.martinlong1978.FluutCAM`, or
use the AppImage or tarball. The Flatpak is not yet on Flathub.

**Tarball.**

1. Unpack `fluutcam-<version>-linux-x86_64.tar.gz` (needs glibc 2.35 or later).
2. Run `bin/fluutcam` or `bin/fluut`. For a menu entry, copy `share/applications` and
   `share/icons` from the archive into `~/.local/share`.

**Uninstall:** for the Flatpak, run `flatpak uninstall --user io.github.martinlong1978.FluutCAM`;
add `--delete-data` to remove its settings and crash reports too (they are in
`~/.var/app/io.github.martinlong1978.FluutCAM/`, the settings under `data/fluutcam`). For the
AppImage, delete the file (and the menu entry, if a tool such as AppImageLauncher added one); for
the tarball, delete the unpacked folder and any copied `.desktop` file and icons. To remove
settings and crash reports of the AppImage or tarball too, delete `~/.local/share/fluutcam` (or
`$XDG_DATA_HOME/fluutcam`).

FluutCAM changes no system settings and installs nothing outside the folders above.
