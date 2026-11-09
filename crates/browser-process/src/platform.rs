//! OS-backed entropy and monotonic timing. Constructed inside each renderer,
//! with no new IPC, filesystem, network, or page-controlled capabilities.
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use web_renderer::{PlatformError, PlatformServices};

#[derive(Debug)]
pub(super) struct SystemServices {
    monotonic_origin: Instant,
    time_origin_millis: f64,
}

impl SystemServices {
    pub(super) fn new() -> Result<Self, String> {
        let monotonic_origin = Instant::now();
        let duration = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "Document clock predates the Unix epoch".to_owned())?;
        Ok(Self {
            monotonic_origin,
            // Reduce timer precision to integral milliseconds. Both values have
            // this document's origin; neither consults the logical JS timer clock.
            time_origin_millis: duration.as_millis() as f64,
        })
    }
}

impl PlatformServices for SystemServices {
    fn random_u64(&self) -> Result<u64, PlatformError> {
        getrandom::u64().map_err(|_| PlatformError)
    }

    fn time_origin_millis(&self) -> Result<f64, PlatformError> {
        Ok(self.time_origin_millis)
    }

    fn monotonic_millis(&self) -> Result<f64, PlatformError> {
        Ok(self.monotonic_origin.elapsed().as_millis() as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_services_provide_entropy_and_an_independent_monotonic_document_clock() {
        let source = SystemServices::new().unwrap();
        let origin = source.time_origin_millis().unwrap();
        assert!(origin.is_finite() && origin > 0.0 && origin.fract() == 0.0);
        let before = source.monotonic_millis().unwrap();
        for _ in 0..16 {
            let _ = source.random_u64().unwrap();
        }
        let after = source.monotonic_millis().unwrap();
        assert!(after >= before && after.fract() == 0.0);
        assert_eq!(source.time_origin_millis().unwrap(), origin);
    }
}
