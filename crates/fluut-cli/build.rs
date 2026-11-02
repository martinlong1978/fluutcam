// SPDX-License-Identifier: MIT OR Apache-2.0

//! Gives `fluut.exe` its Windows version resource (D14 §5.2). Code signing
//! checks the product name and version in it (docs/CODE_SIGNING.md).

fn main() {
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set("ProductName", "FluutCAM")
            .set("FileDescription", "FluutCAM command line")
            .set("OriginalFilename", "fluut.exe")
            .set("LegalCopyright", "MIT OR Apache-2.0");
        res.compile().expect("compile the Windows version resource");
    }
}
