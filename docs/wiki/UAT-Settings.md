# UAT: Settings and updates

Tests of the Settings window's release channel. Part of the [user acceptance tests](UAT). See [Settings and updates](Settings-and-Updates).

---

### UAT-STG-01 The channel is Stable by default

- **Preconditions:** FluutCAM has never been started on this account, or its data folder is empty (set `FLUUT_DATA_DIR` to an empty folder to be sure).
- **Steps:**
  1. Start FluutCAM. If it asks "Check for updates automatically?", answer **No**.
  2. Click **Settings** in the title bar.
- **Expected:** Under **Updates** the line **Releases:** shows two options, **Stable** and **Beta**, and **Stable** is selected. No note about test versions is shown. The settings file (see Settings and updates) has `channel = "stable"` in `[update]`, or no `channel` line.
- [ ] Pass

### UAT-STG-02 Choosing Beta shows a note and is kept

- **Preconditions:** UAT-STG-01 done, Settings window open.
- **Steps:**
  1. Select **Beta**.
  2. Read the text under the options.
  3. Close FluutCAM and start it again.
  4. Open **Settings**.
  5. Open `settings.toml` in the data folder.
- **Expected:** After step 1 a small note says Beta also offers test versions and that a stable version is offered when it is newer than the beta you run. After step 4 **Beta** is still selected. The file has `channel = "beta"` in `[update]`.
- [ ] Pass

### UAT-STG-03 Changing the channel clears the last result

- **Preconditions:** A build with an update key and a network (otherwise the result is a message about the key, which is also cleared). Settings open with **Stable** selected.
- **Steps:**
  1. Click **Check now**. Wait for the result line (for example "up to date").
  2. Select **Beta**.
- **Expected:** After step 2 the result line is empty, and no new check starts by itself (the **Check now** button stays enabled). Clicking **Check now** again gives a result for the beta list (or a failure message if no beta release exists yet).
- [ ] Pass

### UAT-STG-04 An unknown channel in the file means Stable

- **Preconditions:** FluutCAM closed.
- **Steps:**
  1. In `settings.toml`, set `channel = "nightly"` in `[update]` and keep the other lines.
  2. Start FluutCAM and open **Settings**.
- **Expected:** **Stable** is selected. The automatic-check setting is what it was before the edit.
- [ ] Pass
