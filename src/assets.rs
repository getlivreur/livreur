use crate::ResolvedConfig;

#[must_use]
pub fn is_windows(target: &str) -> bool {
    target.contains("windows")
}

#[must_use]
pub fn archive_ext(target: &str) -> &'static str {
    if is_windows(target) { "zip" } else { "tar.gz" }
}

#[must_use]
pub fn asset_name(name: &str, tag: &str, target: &str) -> String {
    format!("{name}-{tag}-{target}.{}", archive_ext(target))
}

#[must_use]
pub fn bin_file_name(binary: &str, target: &str) -> String {
    if is_windows(target) {
        format!("{binary}.exe")
    } else {
        binary.to_owned()
    }
}

impl ResolvedConfig {
    #[must_use]
    pub fn expected_assets(&self, tag: &str) -> Vec<String> {
        self.targets
            .iter()
            .map(|target| asset_name(&self.package.name, tag, target))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_assets_by_platform() {
        assert!(!is_windows("x86_64-unknown-linux-gnu"));
        assert!(is_windows("x86_64-pc-windows-msvc"));
        assert_eq!(archive_ext("aarch64-apple-darwin"), "tar.gz");
        assert_eq!(archive_ext("x86_64-pc-windows-msvc"), "zip");
        assert_eq!(
            asset_name("livreur", "v0.1.0", "x86_64-apple-darwin"),
            "livreur-v0.1.0-x86_64-apple-darwin.tar.gz"
        );
        assert_eq!(
            bin_file_name("livreur", "x86_64-unknown-linux-gnu"),
            "livreur"
        );
        assert_eq!(
            bin_file_name("livreur", "x86_64-pc-windows-msvc"),
            "livreur.exe"
        );
    }
}
