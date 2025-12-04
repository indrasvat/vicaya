/// Compile-time build metadata shared across vicaya binaries.
#[derive(Debug, Clone, Copy)]
pub struct BuildInfo {
    pub version: &'static str,
    pub git_sha: &'static str,
    pub timestamp: &'static str,
    pub target: &'static str,
}

const fn env_or<'a>(value: Option<&'a str>, default: &'a str) -> &'a str {
    match value {
        Some(v) => v,
        None => default,
    }
}

pub const BUILD_INFO: BuildInfo = BuildInfo {
    version: env!("CARGO_PKG_VERSION"),
    git_sha: env_or(option_env!("VICAYA_BUILD_GIT_SHA"), "unknown"),
    timestamp: env_or(option_env!("VICAYA_BUILD_TIMESTAMP"), "unknown"),
    target: env_or(option_env!("VICAYA_BUILD_TARGET"), "unknown"),
};

impl BuildInfo {
    pub fn version_line(self, binary_name: &str) -> String {
        format!(
            "{binary_name} {} (rev {}, built {}, target {})",
            self.version, self.git_sha, self.timestamp, self.target
        )
    }
}

pub fn version_string(binary_name: &str) -> String {
    BUILD_INFO.version_line(binary_name)
}
