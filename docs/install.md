# Installing FluutCAM

Download the file for your platform from the
[releases page](https://github.com/martinlong1978/fluutcam/releases). Check it first if you
can ([Code signing policy](CODE_SIGNING.md#checking-a-download)).

## Windows

1. Unzip `fluutcam-<version>-windows-x86_64.zip` to a folder of your choice.
2. Run `fluutcam.exe` (the application) or `fluut.exe` (the command line).

The executables are signed; Windows shows SignPath Foundation as the publisher.

**Uninstall:** delete the folder. To remove settings and crash reports too, delete
`%LOCALAPPDATA%\FluutCAM`.

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

1. Unpack `fluutcam-<version>-linux-x86_64.tar.gz` (needs glibc 2.35 or later).
2. Run `bin/fluutcam` or `bin/fluut`. For a menu entry, copy `share/applications` and
   `share/icons` from the archive into `~/.local/share`.

**Uninstall:** delete the unpacked folder and any copied `.desktop` file and icons. To remove
settings and crash reports too, delete `~/.local/share/fluutcam` (or
`$XDG_DATA_HOME/fluutcam`).

FluutCAM changes no system settings and installs nothing outside the folders above.
