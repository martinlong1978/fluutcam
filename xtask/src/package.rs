// SPDX-License-Identifier: MIT OR Apache-2.0

//! `cargo xtask package`: lays out the release files of one platform from
//! built binaries and generated icons (D14 §5.2), and `checksums`.
//!
//! | Platform | Output in `--out` |
//! |---|---|
//! | `windows-x86_64` | `fluutcam-<v>-windows-x86_64.zip` (portable) |
//! | `linux-x86_64` | `fluutcam-<v>-linux-x86_64.tar.gz` |
//! | `macos-universal` | `FluutCAM.app/`, to sign and wrap in a DMG, and `fluutcam-<v>-macos-universal-cli.tar.gz` |
//!
//! Signing, notarisation, the DMG and the MSI need platform tools and
//! secrets; the release workflow does those (D14 §5.2).

use crate::archive::{self, Entry};
use crate::util::{self, Flags};
use std::path::{Path, PathBuf};

/// Identifier of the macOS bundle. Reverse-DNS; the product owner may
/// replace it with one under a domain they own (plan M8, decisions).
pub const BUNDLE_ID: &str = "io.github.fluutcam.FluutCAM";
/// Oldest macOS the bundle claims to run on.
pub const MACOS_MIN: &str = "11.0";

/// The platforms a release is made for.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Platform {
    WindowsX64,
    LinuxX64,
    MacosUniversal,
}

impl Platform {
    pub fn parse(s: &str) -> Result<Platform, String> {
        match s {
            "windows-x86_64" => Ok(Platform::WindowsX64),
            "linux-x86_64" => Ok(Platform::LinuxX64),
            "macos-universal" => Ok(Platform::MacosUniversal),
            _ => Err(format!("unknown platform {s}")),
        }
    }

    fn exe(self, name: &str) -> String {
        match self {
            Platform::WindowsX64 => format!("{name}.exe"),
            _ => name.to_owned(),
        }
    }
}

/// Where the inputs of a package are.
pub struct Inputs {
    pub version: String,
    /// Holds `fluut` and `fluutcam` (with `.exe` on Windows).
    pub bin_dir: PathBuf,
    /// Output of `cargo xtask icons`.
    pub icons: PathBuf,
    /// Holds `LICENSE-*` and `README.md`.
    pub root: PathBuf,
}

fn read(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// The freedesktop launcher.
pub fn desktop_file() -> String {
    "[Desktop Entry]\n\
     Type=Application\n\
     Name=FluutCAM\n\
     Comment=Open-source CAM for CNC milling\n\
     Exec=fluutcam %f\n\
     Icon=fluutcam\n\
     Terminal=false\n\
     Categories=Graphics;Engineering;\n\
     StartupWMClass=fluutcam\n"
        .to_owned()
}

/// The macOS bundle's `Info.plist`.
pub fn info_plist(version: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
<plist version=\"1.0\">\n\
<dict>\n\
\t<key>CFBundleName</key><string>FluutCAM</string>\n\
\t<key>CFBundleDisplayName</key><string>FluutCAM</string>\n\
\t<key>CFBundleIdentifier</key><string>{BUNDLE_ID}</string>\n\
\t<key>CFBundleExecutable</key><string>fluutcam</string>\n\
\t<key>CFBundleIconFile</key><string>fluutcam</string>\n\
\t<key>CFBundlePackageType</key><string>APPL</string>\n\
\t<key>CFBundleShortVersionString</key><string>{version}</string>\n\
\t<key>CFBundleVersion</key><string>{version}</string>\n\
\t<key>LSMinimumSystemVersion</key><string>{MACOS_MIN}</string>\n\
\t<key>LSApplicationCategoryType</key><string>public.app-category.graphics-design</string>\n\
\t<key>NSHighResolutionCapable</key><true/>\n\
</dict>\n\
</plist>\n"
    )
}

fn licence_entries(prefix: &str, root: &Path) -> Result<Vec<Entry>, String> {
    ["LICENSE-APACHE", "LICENSE-MIT", "README.md"]
        .iter()
        .map(|f| {
            Ok(Entry::new(
                format!("{prefix}{f}"),
                read(&root.join(f))?,
                false,
            ))
        })
        .collect()
}

/// Every file of the package of `platform`. For macOS this is the
/// `FluutCAM.app` tree only.
pub fn entries(platform: Platform, i: &Inputs) -> Result<Vec<Entry>, String> {
    let bin = |name: &str| read(&i.bin_dir.join(platform.exe(name)));
    let mut out = Vec::new();
    match platform {
        Platform::WindowsX64 => {
            let top = format!("fluutcam-{}/", i.version);
            for n in ["fluutcam", "fluut"] {
                out.push(Entry::new(
                    format!("{top}{}", platform.exe(n)),
                    bin(n)?,
                    true,
                ));
            }
            out.push(Entry::new(
                format!("{top}fluutcam.ico"),
                read(&i.icons.join("windows/fluutcam.ico"))?,
                false,
            ));
            out.extend(licence_entries(&top, &i.root)?);
        }
        Platform::LinuxX64 => {
            let top = format!("fluutcam-{}/", i.version);
            for n in ["fluutcam", "fluut"] {
                out.push(Entry::new(format!("{top}bin/{n}"), bin(n)?, true));
            }
            out.push(Entry::new(
                format!("{top}share/applications/fluutcam.desktop"),
                desktop_file().into_bytes(),
                false,
            ));
            for &s in crate::icons::LINUX_SIZES {
                let rel = format!("hicolor/{s}x{s}/apps/fluutcam.png");
                out.push(Entry::new(
                    format!("{top}share/icons/{rel}"),
                    read(&i.icons.join("linux").join(&rel))?,
                    false,
                ));
            }
            out.extend(licence_entries(
                &format!("{top}share/doc/fluutcam/"),
                &i.root,
            )?);
        }
        Platform::MacosUniversal => {
            let app = "FluutCAM.app/Contents/";
            for n in ["fluutcam", "fluut"] {
                out.push(Entry::new(format!("{app}MacOS/{n}"), bin(n)?, true));
            }
            out.push(Entry::new(
                format!("{app}Info.plist"),
                info_plist(&i.version).into_bytes(),
                false,
            ));
            out.push(Entry::new(
                format!("{app}Resources/fluutcam.icns"),
                read(&i.icons.join("macos/fluutcam.icns"))?,
                false,
            ));
            out.extend(licence_entries(&format!("{app}Resources/"), &i.root)?);
        }
    }
    Ok(out)
}

/// Writes the entries below `dir`, keeping the executable bit on Unix.
pub fn write_tree(dir: &Path, entries: &[Entry]) -> Result<(), String> {
    for e in entries {
        let path = dir.join(&e.path);
        std::fs::create_dir_all(path.parent().expect("file path")).map_err(|e| e.to_string())?;
        std::fs::write(&path, &e.data).map_err(|err| format!("{}: {err}", path.display()))?;
        #[cfg(unix)]
        if e.executable {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                .map_err(|err| format!("{}: {err}", path.display()))?;
        }
    }
    Ok(())
}

/// Builds the package; returns the files and directories made in `out`.
pub fn package(
    platform: Platform,
    i: &Inputs,
    out: &Path,
    mtime: u64,
) -> Result<Vec<PathBuf>, String> {
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let all = entries(platform, i)?;
    let v = &i.version;
    let mut made = Vec::new();
    let mut put = |name: String, bytes: Vec<u8>| -> Result<(), String> {
        let p = out.join(name);
        std::fs::write(&p, bytes).map_err(|e| format!("{}: {e}", p.display()))?;
        made.push(p);
        Ok(())
    };
    match platform {
        Platform::WindowsX64 => put(
            format!("fluutcam-{v}-windows-x86_64.zip"),
            archive::zip(&all, mtime)?,
        )?,
        Platform::LinuxX64 => put(
            format!("fluutcam-{v}-linux-x86_64.tar.gz"),
            archive::tar_gz(&all, mtime)?,
        )?,
        Platform::MacosUniversal => {
            let top = format!("fluut-{v}/");
            let mut cli = vec![Entry::new(
                format!("{top}fluut"),
                read(&i.bin_dir.join("fluut"))?,
                true,
            )];
            cli.extend(licence_entries(&top, &i.root)?);
            put(
                format!("fluutcam-{v}-macos-universal-cli.tar.gz"),
                archive::tar_gz(&cli, mtime)?,
            )?;
            write_tree(out, &all)?;
            made.push(out.join("FluutCAM.app"));
        }
    }
    Ok(made)
}

/// `SHA256SUMS` text for `files`, in `sha256sum` format, sorted by name.
pub fn checksums_text(files: &[(String, Vec<u8>)]) -> String {
    let mut v: Vec<_> = files.iter().collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v.iter()
        .map(|(name, data)| format!("{}  {name}\n", util::sha256_hex(data)))
        .collect()
}

/// Writes `SHA256SUMS` for the regular files directly in `dir`.
pub fn write_checksums(dir: &Path) -> Result<PathBuf, String> {
    let mut files = Vec::new();
    for e in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let e = e.map_err(|e| e.to_string())?;
        let name = e.file_name().to_string_lossy().into_owned();
        if e.path().is_file() && name != "SHA256SUMS" {
            files.push((name, read(&e.path())?));
        }
    }
    let path = dir.join("SHA256SUMS");
    std::fs::write(&path, checksums_text(&files)).map_err(|e| e.to_string())?;
    Ok(path)
}

/// `cargo xtask package --platform P --bin-dir D --icons I --out O [--version V]`.
pub fn run(args: Vec<String>) -> Result<(), String> {
    let f = Flags::parse(args)?;
    let root = util::workspace_root();
    let version = match f.opt("version") {
        Some(v) => v.to_owned(),
        None => util::workspace_version(&read_string(&root.join("Cargo.toml"))?)?,
    };
    let platform = Platform::parse(f.req("platform")?)?;
    let inputs = Inputs {
        version,
        bin_dir: f.req("bin-dir")?.into(),
        icons: f.req("icons")?.into(),
        root: root.into(),
    };
    let mtime = util::source_date_epoch().unwrap_or(0);
    for p in package(platform, &inputs, Path::new(f.req("out")?), mtime)? {
        println!("package: {}", p.display());
    }
    Ok(())
}

fn read_string(p: &Path) -> Result<String, String> {
    std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A temporary directory with fake binaries, icons and licences.
    struct Fixture(PathBuf);

    impl Fixture {
        fn new(tag: &str, exe: &str) -> Fixture {
            let d = std::env::temp_dir().join(format!("fluut-pkg-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&d);
            for (p, data) in [
                (format!("bin/fluut{exe}"), "CLI"),
                (format!("bin/fluutcam{exe}"), "APP"),
                ("icons/windows/fluutcam.ico".to_owned(), "ICO"),
                ("icons/macos/fluutcam.icns".to_owned(), "ICNS"),
                ("root/LICENSE-APACHE".to_owned(), "A"),
                ("root/LICENSE-MIT".to_owned(), "M"),
                ("root/README.md".to_owned(), "R"),
            ]
            .into_iter()
            .chain(crate::icons::LINUX_SIZES.iter().map(|s| {
                (
                    format!("icons/linux/hicolor/{s}x{s}/apps/fluutcam.png"),
                    "PNG",
                )
            })) {
                let path = d.join(p);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, data).unwrap();
            }
            Fixture(d)
        }

        fn inputs(&self) -> Inputs {
            Inputs {
                version: "1.2.3".into(),
                bin_dir: self.0.join("bin"),
                icons: self.0.join("icons"),
                root: self.0.join("root"),
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn paths(e: &[Entry]) -> Vec<&str> {
        e.iter().map(|e| e.path.as_str()).collect()
    }

    #[test]
    fn windows_zip_holds_both_exes_icon_and_licences() {
        let fx = Fixture::new("win", ".exe");
        let e = entries(Platform::WindowsX64, &fx.inputs()).unwrap();
        let p = paths(&e);
        for want in [
            "fluutcam-1.2.3/fluutcam.exe",
            "fluutcam-1.2.3/fluut.exe",
            "fluutcam-1.2.3/fluutcam.ico",
            "fluutcam-1.2.3/LICENSE-MIT",
            "fluutcam-1.2.3/LICENSE-APACHE",
            "fluutcam-1.2.3/README.md",
        ] {
            assert!(p.contains(&want), "{want} in {p:?}");
        }
    }

    #[test]
    fn linux_tarball_follows_the_prefix_layout() {
        let fx = Fixture::new("lin", "");
        let e = entries(Platform::LinuxX64, &fx.inputs()).unwrap();
        let p = paths(&e);
        assert!(p.contains(&"fluutcam-1.2.3/bin/fluutcam"));
        assert!(p.contains(&"fluutcam-1.2.3/share/applications/fluutcam.desktop"));
        assert!(p.contains(&"fluutcam-1.2.3/share/icons/hicolor/256x256/apps/fluutcam.png"));
        assert!(p.contains(&"fluutcam-1.2.3/share/doc/fluutcam/LICENSE-MIT"));
        assert!(
            e.iter()
                .filter(|e| e.path.contains("/bin/"))
                .all(|e| e.executable)
        );
    }

    #[test]
    fn macos_bundle_has_plist_executable_and_icon() {
        let fx = Fixture::new("mac", "");
        let e = entries(Platform::MacosUniversal, &fx.inputs()).unwrap();
        let p = paths(&e);
        assert!(p.contains(&"FluutCAM.app/Contents/MacOS/fluutcam"));
        assert!(p.contains(&"FluutCAM.app/Contents/Resources/fluutcam.icns"));
        let plist = e.iter().find(|e| e.path.ends_with("Info.plist")).unwrap();
        let text = String::from_utf8(plist.data.clone()).unwrap();
        assert!(text.contains("<key>CFBundleShortVersionString</key><string>1.2.3</string>"));
        assert!(text.contains("<key>CFBundleExecutable</key><string>fluutcam</string>"));
        assert!(text.contains(BUNDLE_ID));
    }

    #[test]
    fn missing_input_names_the_file() {
        let fx = Fixture::new("miss", "");
        std::fs::remove_file(fx.0.join("bin/fluut")).unwrap();
        let err = entries(Platform::LinuxX64, &fx.inputs()).unwrap_err();
        assert!(err.contains("fluut"), "{err}");
    }

    #[test]
    fn package_writes_the_archive_and_the_app_tree() {
        let fx = Fixture::new("out", "");
        let out = fx.0.join("dist");
        let made = package(Platform::LinuxX64, &fx.inputs(), &out, 0).unwrap();
        assert_eq!(made, [out.join("fluutcam-1.2.3-linux-x86_64.tar.gz")]);
        assert!(made[0].is_file());
        package(Platform::MacosUniversal, &fx.inputs(), &out, 0).unwrap();
        assert!(out.join("FluutCAM.app/Contents/Info.plist").is_file());
        assert!(
            out.join("fluutcam-1.2.3-macos-universal-cli.tar.gz")
                .is_file()
        );
    }

    #[test]
    fn checksums_use_sha256sum_format_sorted_by_name() {
        let t = checksums_text(&[("b".into(), b"abc".to_vec()), ("a".into(), b"".to_vec())]);
        assert_eq!(
            t,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  a\n\
             ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  b\n"
        );
    }

    #[test]
    fn write_checksums_skips_itself() {
        let fx = Fixture::new("sum", "");
        let dir = fx.0.join("root");
        write_checksums(&dir).unwrap();
        let again = write_checksums(&dir).unwrap();
        let t = std::fs::read_to_string(again).unwrap();
        assert_eq!(t.lines().count(), 3);
        assert!(!t.contains("SHA256SUMS"));
    }
}
