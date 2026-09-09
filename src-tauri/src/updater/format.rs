//! Do not let the plugin's byte sniffing select a different Windows installer
//! than the installed package. Metadata, signature and these checks all agree.
use super::PackageMode;

pub(super) fn verify(mode: &PackageMode, bytes: &[u8]) -> Result<(), String> {
    let valid = match mode {
        PackageMode::Automatic {
            platform: "windows-x86_64",
            msi: true,
        } => bytes.starts_with(b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1"),
        PackageMode::Automatic {
            platform: "windows-x86_64",
            msi: false,
        } => {
            let pe = bytes
                .get(0x3c..0x40)
                .map(|raw| u32::from_le_bytes(raw.try_into().unwrap()) as usize);
            bytes.starts_with(b"MZ")
                && pe.and_then(|at| at.checked_add(4).and_then(|end| bytes.get(at..end)))
                    == Some(b"PE\0\0".as_slice())
        }
        PackageMode::Automatic {
            platform: "linux-x86_64",
            msi: false,
        } => {
            bytes.starts_with(b"\x7fELF\x02\x01") // ELF64, little-endian
                && bytes.get(8..11) == Some(b"AI\x02".as_slice()) // AppImage Type2 magic
                && bytes.get(18..20) == Some(b"\x3e\x00".as_slice())
        } // EM_X86_64
        PackageMode::Automatic {
            platform: "darwin-aarch64" | "darwin-x86_64",
            msi: false,
        } => bytes.starts_with(b"\x1f\x8b\x08"), // gzip archive; actual .app architecture is signed in metadata
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err("Формат подписанного пакета не соответствует установленному лаунчеру. Автоматическая смена типа установки запрещена.".into())
    }
}

pub(super) fn installed_label() -> &'static str {
    use tauri::utils::{config::BundleType, platform::bundle_type};
    if cfg!(debug_assertions) {
        return "development";
    }
    match bundle_type() {
        Some(BundleType::AppImage) => "AppImage",
        Some(BundleType::Deb) => "deb",
        Some(BundleType::Rpm) => "rpm",
        Some(BundleType::Msi) => "MSI",
        Some(BundleType::Nsis) => "NSIS",
        Some(BundleType::App | BundleType::Dmg) => "app",
        None => "unpackaged",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn mode(platform: &'static str, msi: bool) -> PackageMode {
        PackageMode::Automatic { platform, msi }
    }
    #[test]
    fn windows_installers_cannot_silently_switch_formats() {
        let msi = b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1";
        let mut exe = vec![0; 96];
        exe[..2].copy_from_slice(b"MZ");
        exe[0x3c] = 64;
        exe[64..68].copy_from_slice(b"PE\0\0");
        assert!(verify(&mode("windows-x86_64", true), msi).is_ok());
        assert!(verify(&mode("windows-x86_64", false), &exe).is_ok());
        assert!(verify(&mode("windows-x86_64", true), &exe).is_err());
        assert!(verify(&mode("windows-x86_64", false), msi).is_err());
        exe[0x3c..0x40].fill(255); // malformed offset cannot panic/wrap
        assert!(verify(&mode("windows-x86_64", false), &exe).is_err());
    }
    #[test]
    fn linux_requires_the_expected_appimage_arch_and_mac_requires_archive() {
        let mut image = vec![0; 32];
        image[..6].copy_from_slice(b"\x7fELF\x02\x01");
        image[8..11].copy_from_slice(b"AI\x02");
        image[18..20].copy_from_slice(b"\x3e\x00");
        assert!(verify(&mode("linux-x86_64", false), &image).is_ok());
        image[18] = 183; // ARM64 ELF is never an x86_64 update.
        assert!(verify(&mode("linux-x86_64", false), &image).is_err());
        for platform in ["darwin-aarch64", "darwin-x86_64"] {
            assert!(verify(&mode(platform, false), b"\x1f\x8b\x08").is_ok());
            assert!(verify(&mode(platform, false), b"MSI").is_err());
        }
        assert!(verify(&PackageMode::Manual, b"\x1f\x8b\x08").is_err());
    }
}
