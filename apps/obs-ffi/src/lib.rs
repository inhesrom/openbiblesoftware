//! The FFI surface of the Mobile app (ADR 0021, `docs/dev/mobile-core.md`): UniFFI exports over
//! `obs-mobile`. Rules stay in `obs-mobile`; this crate only converts and forwards.

uniffi::setup_scaffolding!();

/// The engine's version.
#[uniffi::export]
pub fn engine_version() -> String {
    obs_mobile::MobileApp::engine_version().to_string()
}

#[cfg(test)]
mod tests {
    #[test]
    fn engine_version_is_the_engines() {
        assert_eq!(
            super::engine_version(),
            obs_mobile::MobileApp::engine_version()
        );
    }
}
