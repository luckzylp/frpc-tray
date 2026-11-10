//! Describes the frp release artifact that matches the OS / CPU this binary
//! was built for. Windows ships a `.zip` with `frpc.exe`; Linux and macOS ship
//! a `.tar.gz` with an extension-less `frpc` binary.

/// `frpc.exe` on Windows, `frpc` everywhere else.
#[cfg(windows)]
pub const BINARY_NAME: &str = "frpc.exe";
#[cfg(not(windows))]
pub const BINARY_NAME: &str = "frpc";

/// frp publishes Windows builds as `.zip`, all other platforms as `.tar.gz`.
#[cfg(windows)]
pub const ARCHIVE_EXT: &str = "zip";
#[cfg(not(windows))]
pub const ARCHIVE_EXT: &str = "tar.gz";

/// The `<os>_<arch>` suffix used by fatedier/frp release assets.
fn asset_target() -> Result<&'static str, String> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let target = match (os, arch) {
        ("windows", "x86_64") => "windows_amd64",
        ("windows", "aarch64") => "windows_arm64",
        ("linux", "x86_64") => "linux_amd64",
        ("linux", "aarch64") => "linux_arm64",
        ("linux", "arm") => "linux_arm",
        ("linux", "loongarch64") => "linux_loong64",
        ("linux", "riscv64") => "linux_riscv64",
        ("macos", "x86_64") => "darwin_amd64",
        ("macos", "aarch64") => "darwin_arm64",
        _ => {
            return Err(format!(
                "平台 {}-{} 暂无对应的 frp 预编译包，请手动把 {} 放到 frp/ 目录",
                os, arch, BINARY_NAME
            ));
        }
    };
    Ok(target)
}

/// `frp_<version>_<os>_<arch>`
pub fn archive_stem(version: &str) -> Result<String, String> {
    Ok(format!("frp_{}_{}", version, asset_target()?))
}

/// `frp_<version>_<os>_<arch>.zip` / `.tar.gz`
pub fn archive_file_name(version: &str) -> Result<String, String> {
    Ok(format!("{}.{}", archive_stem(version)?, ARCHIVE_EXT))
}

pub fn download_url(version: &str) -> Result<String, String> {
    Ok(format!(
        "https://github.com/fatedier/frp/releases/download/v{}/{}",
        version,
        archive_file_name(version)?
    ))
}

/// Expected path of the frpc binary inside the release archive.
pub fn archive_entry(version: &str) -> Result<String, String> {
    Ok(format!("{}/{}", archive_stem(version)?, BINARY_NAME))
}

/// Accept the exact expected entry, plus a tolerant `*/frpc[.exe]` fallback in
/// case frp changes its top-level directory name in a future release.
pub fn is_frpc_entry(name: &str, expected: &str) -> bool {
    name == expected || name.ends_with(&format!("/{}", BINARY_NAME))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_names_are_platform_specific() {
        let stem = archive_stem("0.71.0").unwrap();
        assert!(stem.starts_with("frp_0.71.0_"));
        assert_eq!(
            download_url("0.71.0").unwrap(),
            format!(
                "https://github.com/fatedier/frp/releases/download/v0.71.0/{}.{}",
                stem, ARCHIVE_EXT
            )
        );
        assert_eq!(
            archive_entry("0.71.0").unwrap(),
            format!("{}/{}", stem, BINARY_NAME)
        );
    }

    #[test]
    fn entry_matching_is_tolerant() {
        assert!(is_frpc_entry(
            &format!("a/b/{}", BINARY_NAME),
            "does/not/match"
        ));
        assert!(!is_frpc_entry("a/b/frps", "does/not/match"));
    }
}
