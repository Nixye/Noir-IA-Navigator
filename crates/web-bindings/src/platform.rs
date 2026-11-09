//! Explicit data-only entropy and document-clock services, supplied by the
//! renderer supervisor. Cloned checkpoints share their source, never rewind it.
use super::*;
use std::sync::Arc;

/// A trusted, document-scoped source. Implementations belong outside web-*.
/// No page-selected path, URL, seed, or arbitrary request is accepted here.
pub trait PlatformServices: std::fmt::Debug + Send + Sync {
    fn random_u64(&self) -> Result<u64, PlatformError>;
    fn time_origin_millis(&self) -> Result<f64, PlatformError>;
    fn monotonic_millis(&self) -> Result<f64, PlatformError>;
}

/// Deliberately payload-free: OS diagnostics must not leak through page errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlatformError;

#[derive(Debug, Clone, Default)]
pub(super) struct PlatformState {
    source: Option<Arc<dyn PlatformServices>>,
}

impl PlatformState {
    fn source(&self) -> Result<&dyn PlatformServices, JsError> {
        self.source
            .as_deref()
            .ok_or_else(|| JsError::runtime("Document platform services are unavailable"))
    }

    pub(super) fn random_u64(&self) -> Result<u64, JsError> {
        self.source()?
            .random_u64()
            .map_err(|_| JsError::runtime("Document entropy source failed"))
    }

    pub(super) fn time_origin_millis(&self) -> Result<f64, JsError> {
        self.source()?
            .time_origin_millis()
            .map_err(|_| JsError::runtime("Document time origin is unavailable"))
    }

    pub(super) fn monotonic_millis(&self) -> Result<f64, JsError> {
        self.source()?
            .monotonic_millis()
            .map_err(|_| JsError::runtime("Document monotonic clock failed"))
    }
}

impl DocumentSession {
    /// Install once before any execution attempt, including a failed script.
    /// State rollback cannot replace a source or rewind its clock/entropy.
    pub fn set_platform_services(
        &mut self,
        source: Arc<dyn PlatformServices>,
    ) -> Result<(), JsError> {
        if self.document_url_frozen || self.initial_started || self.state.platform.source.is_some()
        {
            return Err(JsError::runtime("Document platform services are frozen"));
        }
        self.state.platform.source = Some(source);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[derive(Debug, Default)]
    struct TestServices {
        elapsed: AtomicU64,
        random_calls: AtomicU64,
        clock_reads: AtomicU64,
        fail: bool,
    }
    impl PlatformServices for TestServices {
        fn random_u64(&self) -> Result<u64, PlatformError> {
            if self.fail {
                return Err(PlatformError);
            }
            Ok(self.random_calls.fetch_add(1, Ordering::Relaxed) << 62)
        }
        fn time_origin_millis(&self) -> Result<f64, PlatformError> {
            Ok(1_700_000_000_000.0)
        }
        fn monotonic_millis(&self) -> Result<f64, PlatformError> {
            if self.fail {
                return Err(PlatformError);
            }
            self.clock_reads.fetch_add(1, Ordering::Relaxed);
            Ok(self.elapsed.load(Ordering::Relaxed) as f64)
        }
    }
    fn session(source: Arc<TestServices>) -> DocumentSession {
        let mut session = DocumentSession::new(web_html::parse("<p id=x>before</p>").unwrap());
        session.set_platform_services(source).unwrap();
        session
    }

    #[test]
    fn document_clock_and_random_keep_window_identity_and_var_bindings() {
        let mut session = session(Arc::new(TestServices::default()));
        assert_eq!(session.execute("var performance;var Math;performance===window.performance&&typeof performance==='object'&&performance.now()===0&&performance.timeOrigin===1700000000000&&('performance' in window)&&Math===window.Math&&Math.random()===0&&window.Math.random()===0.25;").unwrap(), Value::Bool(true));
        assert_eq!(session.execute("const now=performance.now;now.length===0&&now.name==='now'&&now.call(performance)===0;").unwrap(), Value::Bool(true));
    }

    #[test]
    fn document_clock_is_independent_of_logical_timers_and_has_a_stable_origin() {
        let source = Arc::new(TestServices::default());
        let mut session = session(source.clone());
        session.execute("var observed=-1;const origin=performance.timeOrigin;setTimeout(function(){observed=performance.now();},10);").unwrap();
        session.run_due_timers(86_400_000).unwrap();
        assert_eq!(
            session
                .execute("observed===0&&performance.now()===0&&performance.timeOrigin===origin;")
                .unwrap(),
            Value::Bool(true)
        );
        source.elapsed.store(37, Ordering::Relaxed);
        assert_eq!(
            session
                .execute("performance.now()===37&&performance.timeOrigin===origin;")
                .unwrap(),
            Value::Bool(true)
        );
    }

    #[test]
    fn performance_receiver_readonly_origin_and_unsupported_constructor() {
        let mut session = session(Arc::new(TestServices::default()));
        assert_eq!(session.execute("let invalid=false;let ctor=false;try{performance.now.call({});}catch(e){invalid=e.name==='TypeError';}try{new performance.now();}catch(e){ctor=e.name==='TypeError';}performance.timeOrigin=42;const d=Object.getOwnPropertyDescriptor(performance,'timeOrigin');invalid&&ctor&&performance.timeOrigin===1700000000000&&!d.writable&&!d.configurable&&performance.now({valueOf:function(){throw 9;}})===0;").unwrap(), Value::Bool(true));
    }

    #[test]
    fn platform_services_cannot_be_replaced_after_install_or_any_execution() {
        let source = Arc::new(TestServices::default());
        let mut installed = session(source.clone());
        assert!(installed.set_platform_services(source.clone()).is_err());
        for script in ["1;", "throw 3;", "const =;"] {
            let mut empty = DocumentSession::new(web_html::parse("").unwrap());
            let _ = empty.execute(script);
            assert!(
                empty.set_platform_services(source.clone()).is_err(),
                "{script}"
            );
        }
    }

    #[test]
    fn source_failures_are_explicit_and_checkpoint_clones_share_consumption() {
        let source = Arc::new(TestServices::default());
        let mut session = session(source.clone());
        let checkpoint = session.state.clone();
        assert_eq!(
            session.execute("Math.random();").unwrap(),
            Value::Number(0.0)
        );
        session.state = checkpoint;
        assert_eq!(
            session.execute("Math.random();").unwrap(),
            Value::Number(0.25)
        );
        assert_eq!(source.random_calls.load(Ordering::Relaxed), 2);
        let mut failed = self::session(Arc::new(TestServices {
            fail: true,
            ..TestServices::default()
        }));
        assert_eq!(
            failed.execute("Math.random();").unwrap_err().message,
            "Document entropy source failed"
        );
        assert_eq!(
            failed.execute("performance.now();").unwrap_err().message,
            "Document monotonic clock failed"
        );
        let mut empty = DocumentSession::new(web_html::parse("").unwrap());
        assert!(
            empty
                .execute("Math.random();")
                .unwrap_err()
                .message
                .contains("unavailable")
        );
    }

    #[test]
    fn performance_namespace_remains_a_gc_root_and_is_per_document() {
        let mut one = session(Arc::new(TestServices::default()));
        one.execute("performance.tag=123;").unwrap();
        one.runtime
            .collect_garbage(
                &[],
                &DomHost {
                    document: &mut one.document,
                    state: &mut one.state,
                },
            )
            .unwrap();
        assert_eq!(
            one.execute("performance.tag===123&&performance.now()===0;")
                .unwrap(),
            Value::Bool(true)
        );
        let mut two = session(Arc::new(TestServices::default()));
        assert_eq!(
            two.execute("performance.tag===undefined;").unwrap(),
            Value::Bool(true)
        );
    }

    #[test]
    fn real_resource_rollback_restores_dom_without_rewinding_platform_source() {
        let source = Arc::new(TestServices::default());
        source.elapsed.store(42, Ordering::Relaxed);
        let mut session = session(source.clone());
        let error = session.execute("document.getElementById('x').textContent='changed';Math.random();performance.now();while(true){}").unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
        assert!(session.scripts_disabled);
        let node = session
            .document
            .elements()
            .find_map(|(id, element)| (element.attr("id") == Some("x")).then_some(id))
            .unwrap();
        assert_eq!(session.document.text_content(node), "before");
        assert_eq!(source.random_calls.load(Ordering::Relaxed), 1);
        assert_eq!(source.clock_reads.load(Ordering::Relaxed), 1);
        assert_eq!(session.state.platform.random_u64().unwrap(), 1 << 62);
        assert_eq!(session.state.platform.monotonic_millis().unwrap(), 42.0);
        assert_eq!(source.clock_reads.load(Ordering::Relaxed), 2);
        assert!(
            session
                .set_platform_services(Arc::new(TestServices::default()))
                .is_err()
        );
    }

    #[derive(Debug)]
    struct InvalidClock {
        origin: Result<f64, PlatformError>,
        now: f64,
    }
    impl PlatformServices for InvalidClock {
        fn random_u64(&self) -> Result<u64, PlatformError> {
            Ok(0)
        }
        fn time_origin_millis(&self) -> Result<f64, PlatformError> {
            self.origin
        }
        fn monotonic_millis(&self) -> Result<f64, PlatformError> {
            Ok(self.now)
        }
    }

    #[test]
    fn invalid_clock_values_and_missing_origins_are_never_exposed_to_scripts() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0] {
            for invalid_origin in [false, true] {
                let mut session = DocumentSession::new(web_html::parse("").unwrap());
                session
                    .set_platform_services(Arc::new(InvalidClock {
                        origin: Ok(if invalid_origin { value } else { 1.0 }),
                        now: if invalid_origin { 0.0 } else { value },
                    }))
                    .unwrap();
                let error = session.execute("performance.now();").unwrap_err();
                assert!(error.message.contains(if invalid_origin {
                    "time origin"
                } else {
                    "monotonic time"
                }));
            }
        }
        let mut session = DocumentSession::new(web_html::parse("").unwrap());
        session
            .set_platform_services(Arc::new(InvalidClock {
                origin: Err(PlatformError),
                now: 0.0,
            }))
            .unwrap();
        assert_eq!(
            session
                .execute("performance.timeOrigin;")
                .unwrap_err()
                .message,
            "Document time origin is unavailable"
        );
    }
}
