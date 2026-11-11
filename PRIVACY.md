# Privacy policy

This program will not transfer any information to other networked systems unless
specifically requested by the user or the person installing or operating it.

FluutCAM is a desktop program. It has no account, no telemetry, no analytics and no
advertising. Your projects, models, tools and settings stay on your computer.

## What can use the network

| Feature | When | What is sent | Where |
|---|---|---|---|
| Update check | Only if you say Yes when FluutCAM asks on first start, or turn it on in Settings; at most once a day | One HTTPS request for the release list (and one for its signature). GitHub sees your IP address and the fixed user agent `FluutCAM-update-check`, which carries no version or system details; nothing else is sent. If you choose the **Beta** release channel in Settings, the list asked for is the beta one, so GitHub can tell that you chose it | `github.com` (GitHub's [privacy statement](https://docs.github.com/en/site-policy/privacy-policies/github-general-privacy-statement)) |
| Check now | Only when you click **Check now**, or run `fluut update-check` | The same single request | `github.com` |

The Linux Flatpak is built without the network permission, so neither of these can run
there: the Flatpak sandbox blocks the request. That is by decision (a Flatpak is updated by
Flatpak, and the least permission that works is the right one); the update check still
reports a failure if you turn it on.

The update check is off until you turn it on. You can turn it off at any time in
Settings. FluutCAM never downloads or installs an update itself: it shows the new version
and a link to the release page, and you download the file yourself.

## What stays local

- **Crash reports** are written to files in your settings folder. They are never sent.
  You can attach one to an issue yourself if you want to.
- **Settings and recent files** are kept in your settings folder
  (`%LOCALAPPDATA%\FluutCAM` on Windows, `~/Library/Application Support/FluutCAM` on
  macOS, `~/.local/share/fluutcam` on Linux, `~/.var/app/io.github.martinlong1978.FluutCAM/data/fluutcam`
in the Flatpak).

## Installing and uninstalling

The Windows installer adds the program, a Start menu entry and a `PATH` entry, and
removes all three when you uninstall. The Linux Flatpak asks for a display and the GPU only:
no network and no file access beyond the files you choose in the file dialogs. It does not change any other system setting. See
[Installing FluutCAM](docs/install.md) for every platform, including how to uninstall.

## Changes

A change to this policy is a change to this file, reviewed like any other change in the
repository. Its history is on GitHub.

## Contact

Questions about privacy: open an issue on
[GitHub](https://github.com/martinlong1978/fluutcam/issues).
