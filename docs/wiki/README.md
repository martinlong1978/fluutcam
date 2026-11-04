# The FluutCAM wiki (source)

These files are the pages of the [FluutCAM wiki](https://github.com/martinlong1978/fluutcam/wiki). **The wiki is a copy.** The workflow `.github/workflows/wiki.yml` copies this folder to the wiki when a change that touches `docs/wiki/` reaches `main`, and deletes wiki pages that are no longer here. Edit pages here, in a pull request, never in the wiki's own editor.

## Layout

- `Home.md`, `_Sidebar.md`, `_Footer.md`: the wiki's front page, side menu and footer.
- `*.md`: pages. The file name is the page name (`Add-a-Setup.md` is the page "Add a Setup").
- `images/`: screenshots, made by `cargo xtask screenshots`. Do not edit them by hand.
- `reference/help.toml`, `reference/advice.toml`: text that the generator uses (not copied to the wiki).
- `README.md`: this note (not copied to the wiki).

## Generated pages

`Toolpath-Types.md`, one `Toolpath-<type>.md` per toolpath type, and `Diagnostics.md` are generated and start with a comment saying so. Do not edit them. Run

```
cargo xtask wiki-reference           # write them
cargo xtask wiki-reference --check   # fail if they are out of date (CI runs this)
```

To change the text of a setting, edit `reference/help.toml`. To change the advice for a diagnostic, edit `reference/advice.toml`. Everything else comes from the code and the design documents.

## Screenshots

```
cargo xtask screenshots
```

renders the app headless, from the reference projects in `crates/fluut-cli/tests/projects/`, into `images/`. The list of pictures is one table at the top of `crates/fluut-app/examples/screenshots.rs`. Run it again, and commit the pictures, whenever the window changes.

## Writing

- Plain English for a hobbyist or small-shop machinist: short sentences, active voice. Explain a CAM term the first time it appears on a page.
- Say only what the program does. Check it against the code or by running `fluut` or the app. Say "not yet" plainly for planned features, and add them to `Known-Limits.md`.
- The ribbon is being wired up. Until it is complete, the pages describe the temporary File menu and say the ribbon will replace it.
- Do not copy text, names, screenshots or workflows from other CAM products.
- A test in the `UAT-*` pages changes together with the behaviour it tests.
