//! Core of the application: block data model and file format readers/writers.
//!
//! This crate has no GPU or UI dependency so that everything in it can be unit tested headlessly.

/// Version of the application, shared by every crate of the workspace.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_semver() {
        let parts: Vec<&str> = VERSION.split('.').collect();
        assert_eq!(parts.len(), 3);
        assert!(parts.iter().all(|p| p.parse::<u32>().is_ok()));
    }
}
