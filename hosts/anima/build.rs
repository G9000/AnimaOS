//! On macOS, embed an Info.plist in the `anima` binary. `codesign` takes a bare
//! binary's identifier from its embedded `CFBundleIdentifier`, so every release signs
//! as `dev.animaos.anima` instead of a name derived from the file. The keychain trusts
//! a signature by that identifier, so it has to stay the same across updates.

use std::env;
use std::fs;
use std::path::PathBuf;

const BUNDLE_IDENTIFIER: &str = "dev.animaos.anima";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }

    let version = env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION");
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleIdentifier</key>
	<string>{BUNDLE_IDENTIFIER}</string>
	<key>CFBundleName</key>
	<string>Anima</string>
	<key>CFBundleInfoDictionaryVersion</key>
	<string>6.0</string>
	<key>CFBundleShortVersionString</key>
	<string>{version}</string>
	<key>CFBundleVersion</key>
	<string>{version}</string>
</dict>
</plist>
"#
    );
    let path = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR")).join("Info.plist");
    fs::write(&path, plist).expect("write Info.plist");

    // One linker argument per line, so a path with commas or spaces stays whole.
    let path = path.to_str().expect("OUT_DIR is UTF-8");
    for arg in ["-sectcreate", "__TEXT", "__info_plist", path] {
        println!("cargo:rustc-link-arg-bin=anima=-Xlinker");
        println!("cargo:rustc-link-arg-bin=anima={arg}");
    }
}
