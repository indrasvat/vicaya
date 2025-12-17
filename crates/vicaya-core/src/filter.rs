//! Path filtering helpers (exclusions, indexing decisions).

use std::path::Path;

/// Return `true` if a path should be indexed given the configured exclusions.
///
/// Exclusions are matched against individual path components. Supports:
/// - Exact component matches (`node_modules`)
/// - Simple globs:
///   - `*.ext` (extension match)
///   - `prefix*` (prefix match)
pub fn should_index_path(path: &Path, exclusions: &[String]) -> bool {
    for exclusion in exclusions {
        for component in path.components() {
            if matches!(component, std::path::Component::RootDir) {
                continue;
            }

            let component_str = component.as_os_str().to_string_lossy();

            if exclusion.contains('*') {
                if let Some(ext) = exclusion.strip_prefix("*.") {
                    if component_str.ends_with(&format!(".{}", ext)) {
                        return false;
                    }
                } else if let Some(prefix) = exclusion.strip_suffix('*') {
                    if !prefix.is_empty() && component_str.starts_with(prefix) {
                        return false;
                    }
                }
            } else if component_str == exclusion.as_str() {
                return false;
            }
        }
    }

    true
}
