# UAT: Install (Flatpak)

Tests of the Linux Flatpak bundle. Part of the [user acceptance tests](UAT). You need a Linux computer with [Flatpak](https://flatpak.org/setup/) and the Flathub remote, and the file `fluutcam-<version>-linux-x86_64.flatpak` of the build you are testing. See [Install](Install).

---

### UAT-INS-01 Install the Flatpak

- **Steps:**
  1. Run `flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo`.
  2. Run `flatpak install --user fluutcam-<version>-linux-x86_64.flatpak` and answer Yes.
  3. Run `flatpak info --user io.github.martinlong1978.FluutCAM`.
- **Expected:** The install fetches the runtime (freedesktop 26.08) if you do not have it and ends without an error. `flatpak info` shows FluutCAM with the version you are testing. FluutCAM appears in the application menu with its icon (log out and in if the menu does not show it).
- [ ] Pass

### UAT-INS-02 Run it and check the permissions

- **Steps:**
  1. Start FluutCAM from the application menu (or `flatpak run io.github.martinlong1978.FluutCAM`).
  2. Open **About FluutCAM** and note the version.
  3. In a terminal run `flatpak run --command=fluut io.github.martinlong1978.FluutCAM --version`.
  4. Run `flatpak info --show-permissions io.github.martinlong1978.FluutCAM`.
- **Expected:** The window opens and the viewport draws. The version in About and in the terminal is the one you installed. The permissions list the sockets `wayland` and `fallback-x11`, the device `dri` and the shared `ipc`, and nothing else: no `network`, no `filesystems`.
- [ ] Pass

### UAT-INS-03 Files go through the file dialogs

- **Preconditions:** UAT-INS-02. A copy of the `holes` project in a work folder under your home folder.
- **Steps:**
  1. In FluutCAM click **Open** and pick the `.fluut` file in the work folder, in the dialog that appears (a system dialog, not a GTK one drawn by FluutCAM).
  2. Click **Save as** and save a copy in the work folder.
  3. If the project does not open because its model file is beside it, note the message, run `flatpak override --user --filesystem=<work folder> io.github.martinlong1978.FluutCAM`, restart FluutCAM and open the project again.
- **Expected:** The dialog opens and the file you pick is opened. The copy is saved where you chose. Write on the record sheet whether step 3 was needed. After `flatpak override --user --reset io.github.martinlong1978.FluutCAM` the override is gone.
- [ ] Pass

### UAT-INS-04 Uninstall the Flatpak

- **Preconditions:** UAT-INS-02, with FluutCAM closed.
- **Steps:**
  1. Run `flatpak uninstall --user --delete-data io.github.martinlong1978.FluutCAM`.
  2. Run `flatpak info --user io.github.martinlong1978.FluutCAM`.
  3. Look for the folder `~/.var/app/io.github.martinlong1978.FluutCAM`.
- **Expected:** The uninstall ends without an error. `flatpak info` says the application is not installed. FluutCAM is gone from the application menu. The folder in step 3 does not exist. Files you saved in your work folder are still there.
- [ ] Pass
