//! Bounded initial-document scheduling, not an incremental HTML event loop.
use super::*;
use diagnostics::ScriptOrigin;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum ReadyState {
    #[default]
    Loading,
    Interactive,
    Complete,
}
impl ReadyState {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Loading => "loading",
            Self::Interactive => "interactive",
            Self::Complete => "complete",
        }
    }
}
const MAX_INITIAL_SCRIPTS: usize = 64;
const MAX_INITIAL_SOURCE_BYTES: usize = 512 * 1024;

pub(super) fn charge_source(bytes: &mut usize, length: usize) -> Result<(), JsError> {
    if length > MAX_INITIAL_SOURCE_BYTES.saturating_sub(*bytes) {
        return Err(JsError::resource(
            "Initial script source budget exceeded 512 KiB",
        ));
    }
    *bytes += length;
    Ok(())
}
enum InitialScript {
    Classic(String),
    Module(String),
}
impl DocumentSession {
    /// One attempt per session, including failed attempts. The DOM is already
    /// parsed and assets are broker data. Only IDs are snapshotted initially:
    /// detached/inactive nodes are skipped when visited, and later mutations of
    /// an already queued script cannot alter its captured source/module URL.
    pub fn run_initial(&mut self, sources: &[ScriptAsset]) -> ScriptReport {
        let mut report = ScriptReport::default();
        if self.initial_started {
            report
                .warnings
                .push("JavaScript initial lifecycle already started; replay ignored".into());
            self.finish_initial_report(&mut report);
            return report;
        }
        // This guard is deliberately outside State and all VM transactions.
        self.initial_started = true;
        if self.scripts_disabled {
            report
                .warnings
                .push("JavaScript disabled after resource exhaustion".into());
            return report;
        }
        let ids = connected(&self.document, 0)
            .into_iter()
            .filter(|&id| {
                !self.document.is_inactive_subtree(id)
                    && self.document.element(id).is_some_and(|e| e.tag == "script")
            })
            .take(MAX_INITIAL_SCRIPTS + 1)
            .collect::<Vec<_>>();
        if ids.len() > MAX_INITIAL_SCRIPTS {
            report
                .warnings
                .push("JavaScript: maximum of 64 initial scripts exceeded".into());
        }
        let mut bytes = 0;
        let mut deferred = Vec::new();
        for (index, node) in ids.into_iter().take(MAX_INITIAL_SCRIPTS).enumerate() {
            if !connected(&self.document, 0).contains(&node)
                || self.document.is_inactive_subtree(node)
            {
                continue;
            }
            let Some(element) = self.document.element(node) else {
                continue;
            };
            let kind = element
                .attr("type")
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();
            let module = kind == "module";
            if !module
                && (element.attr("nomodule").is_some()
                    || !matches!(
                        kind.as_str(),
                        "" | "text/javascript" | "application/javascript" | "text/ecmascript"
                    ))
            {
                continue;
            }
            let external = element.attr("src").is_some();
            if (module || external) && element.attr("async").is_some() {
                Self::initial_warning(
                    &mut report,
                    "Async script scheduling is not implemented; script was not executed".into(),
                );
                continue;
            }
            let defer = module || external && element.attr("defer").is_some();
            let origin = ScriptOrigin::capture(
                index,
                node,
                module,
                defer,
                element.attr("src"),
                self.state.document_url.as_ref(),
            );
            let result = self.prepare_initial_script(node, module, sources, &mut bytes);
            match result {
                Ok(script) if defer => deferred.push((origin, script)),
                Ok(script) => {
                    if !self.run_initial_script(script, origin, &mut report) {
                        break;
                    }
                }
                Err(error) => {
                    Self::initial_warning(&mut report, origin.warning("preparation", &error, None));
                    if error.kind == ErrorKind::Resource {
                        self.disable_scripts();
                        break;
                    }
                }
            }
        }
        if !self.scripts_disabled && self.initial_transition(ReadyState::Interactive, &mut report) {
            for (origin, script) in deferred {
                if !self.run_initial_script(script, origin, &mut report) {
                    break;
                }
            }
            if !self.scripts_disabled
                && self.initial_event(DOC, "DOMContentLoaded", &mut report)
                && self.initial_transition(ReadyState::Complete, &mut report)
                && self.initial_event(WINDOW, "load", &mut report)
                && let Err(error) = self.run_due_timers(0)
            {
                Self::initial_warning(
                    &mut report,
                    diagnostics::callback_warning("timer", None, &error),
                );
            }
        }
        self.finish_initial_report(&mut report);
        report
    }
    fn finish_initial_report(&self, report: &mut ScriptReport) {
        report.pending_event_handlers = self
            .state
            .listeners
            .iter()
            .filter(|l| !matches!(l.kind.as_str(), "DOMContentLoaded" | "load"))
            .count();
        report.pending_timers = self.state.timers.len();
    }
    fn initial_warning(report: &mut ScriptReport, message: String) {
        if report.warnings.len() < 32 {
            report.warnings.push(message);
        }
    }
    fn initial_event(&mut self, target: u64, kind: &str, report: &mut ScriptReport) -> bool {
        if let Err(error) = self.dispatch_target(target, kind) {
            let fatal = error.kind == ErrorKind::Resource;
            Self::initial_warning(
                report,
                diagnostics::callback_warning("callback", Some(kind), &error),
            );
            if fatal {
                self.disable_scripts();
                return false;
            }
        }
        !self.scripts_disabled
    }
    fn initial_transition(&mut self, state: ReadyState, report: &mut ScriptReport) -> bool {
        let previous = self.state.ready_state;
        self.state.ready_state = state;
        if !self.initial_event(DOC, "readystatechange", report) {
            // A failed transition must not advertise a completed lifecycle.
            self.state.ready_state = previous;
            return false;
        }
        true
    }
    fn run_initial_script(
        &mut self,
        script: InitialScript,
        origin: ScriptOrigin,
        report: &mut ScriptReport,
    ) -> bool {
        let result = match &script {
            InitialScript::Classic(source) if source.trim().is_empty() => return true,
            InitialScript::Classic(source) => self.execute(source),
            InitialScript::Module(url) => self.execute_registered_module(url),
        };
        match result {
            Ok(_) => report.executed += 1,
            Err(error) => {
                let fatal = error.kind == ErrorKind::Resource;
                Self::initial_warning(
                    report,
                    origin.warning(
                        "execute",
                        &error,
                        match &script {
                            InitialScript::Classic(source) => Some(source),
                            InitialScript::Module(_) => None,
                        },
                    ),
                );
                if fatal {
                    self.disable_scripts();
                    return false;
                }
            }
        }
        true
    }
    fn initial_inline_source(&self, node: NodeId, bytes: &mut usize) -> Result<String, JsError> {
        // Count borrowed text before text_content allocates the combined source.
        let mut pending = vec![node];
        while let Some(id) = pending.pop() {
            if let NodeKind::Text(text) = &self.document.nodes[id].kind {
                charge_source(bytes, text.len())?;
            }
            pending.extend(self.document.nodes[id].children.iter().rev().copied());
        }
        Ok(self.document.text_content(node))
    }
    fn prepare_initial_script(
        &mut self,
        node: NodeId,
        module: bool,
        sources: &[ScriptAsset],
        bytes: &mut usize,
    ) -> Result<InitialScript, JsError> {
        let element = self.document.element(node).expect("visited script element");
        if !module {
            let source = if let Some(src) = element.attr("src") {
                let source = sources
                    .iter()
                    .find(|asset| !asset.src.starts_with("noir-module:") && asset.src == src)
                    .ok_or_else(|| {
                        JsError::runtime(format!(
                            "JavaScript resource not supplied by broker: {}",
                            diagnostics::source_label(src, self.state.document_url.as_ref())
                        ))
                    })?;
                charge_source(bytes, source.source.len())?;
                source.source.clone()
            } else {
                self.initial_inline_source(node, bytes)?
            };
            return Ok(InitialScript::Classic(source));
        }
        let base = self.state.document_url.clone().ok_or_else(|| {
            JsError::runtime("Modules require the broker-authorized document URL")
        })?;
        if !matches!(base.scheme(), "http" | "https" | "file") {
            return Err(JsError::runtime("Module document scheme is not supported"));
        }
        let (url, source_url, source) = if let Some(src) = element.attr("src") {
            let url = base
                .join(src)
                .map_err(|_| JsError::runtime("Invalid module script URL"))?;
            self.require_module_origin(&url, &base)?;
            let source = self
                .module_asset(&url, sources, &base)
                .ok_or_else(|| JsError::runtime("Module source was not supplied by the broker"))?;
            charge_source(bytes, source.len())?;
            (url.clone(), url, source.to_owned())
        } else {
            let mut id = base.clone();
            id.set_fragment(Some(&format!("noir-inline-module-{node}")));
            (id, base.clone(), self.initial_inline_source(node, bytes)?)
        };
        // Preparation mutates only the module registry. Roll it back even for
        // ordinary link errors, preserving VM budget consumption via rollback_to.
        let previous = self.runtime.clone();
        if let Err(error) = self.register_module_graph(
            url.as_str(),
            source_url.as_str(),
            source,
            sources,
            &base,
            bytes,
        ) {
            self.runtime.rollback_to(previous);
            if error.kind == ErrorKind::Resource {
                self.disable_scripts();
            }
            return Err(error);
        }
        Ok(InitialScript::Module(url.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn session(html: &str) -> DocumentSession {
        let mut session = DocumentSession::new(web_html::parse(html).unwrap());
        session
            .set_document_url("https://example.test/page.html")
            .unwrap();
        session
    }
    fn asset(src: &str, source: &str) -> ScriptAsset {
        ScriptAsset {
            src: src.into(),
            source: source.into(),
        }
    }
    #[test]
    fn initial_order_interleaves_defer_modules_events_microtasks_and_timer() {
        let mut s = session(
            r#"
        <script>
        var order=document.readyState;
        document.onreadystatechange=function(e){order+='|r:'+document.readyState+':'+e.bubbles+':'+e.cancelable;queueMicrotask(()=>{order+='|rq';});};
        window.addEventListener('readystatechange',()=>{order+='|BAD';});
        document.addEventListener('DOMContentLoaded',()=>{order+='|d:'+document.readyState;queueMicrotask(()=>{order+='|dq';});});
        window.addEventListener('DOMContentLoaded',()=>{order+='|w';});
        window.onload=()=>{order+='|l:'+document.readyState;};
        setTimeout(()=>{order+='|t';},0);
        queueMicrotask(()=>{order+='|q';});
        </script>
        <script defer src='a.js'></script>
        <script type='module'>order+='|m:'+document.readyState;queueMicrotask(()=>{order+='|mq';});</script>
        <script defer src='b.js'></script>
        <script>order+='|c:'+document.readyState;</script>
        "#,
        );
        let report = s.run_initial(&[
            asset(
                "a.js",
                "order+='|a:'+document.readyState;queueMicrotask(()=>{order+='|aq';});",
            ),
            asset("b.js", "order+='|b:'+document.readyState;"),
        ]);
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert_eq!(report.executed, 5);
        assert_eq!(
            s.runtime.global_value("order"),
            Some(Value::string(
                "loading|q|c:loading|r:interactive:false:false|rq|a:interactive|aq|m:interactive|mq|b:interactive|d:interactive|dq|w|r:complete:false:false|rq|l:complete|t"
            ))
        );
    }
    #[test]
    fn inline_classic_flags_are_ignored_and_external_async_is_refused() {
        let mut s = session(
            "<script async defer>var order=document.readyState;</script><script async src='a.js'></script><script async defer src='b.js'></script><script type=module async>order+='bad';</script><script defer>order+='|'+document.readyState;</script>",
        );
        let report = s.run_initial(&[
            asset("a.js", "order+='bad';"),
            asset("b.js", "order+='bad';"),
        ]);
        assert_eq!(report.executed, 2);
        assert_eq!(report.warnings.len(), 3);
        assert_eq!(
            s.runtime.global_value("order"),
            Some(Value::string("loading|loading"))
        );
        assert_eq!(s.state.ready_state, ReadyState::Complete);
    }
    #[test]
    fn ordinary_errors_continue_initial_lifecycle_and_replay_is_ignored() {
        let mut s = session(
            "<script>var count=1;throw 'ordinary';</script><script>count++;document.addEventListener('DOMContentLoaded',()=>{count++;throw 'event';});window.onload=()=>{count++;};</script>",
        );
        let report = s.run_initial(&[]);
        assert_eq!(report.warnings.len(), 2);
        assert_eq!(s.state.ready_state, ReadyState::Complete);
        assert_eq!(s.runtime.global_value("count"), Some(Value::Number(4.)));
        let replay = s.run_initial(&[]);
        assert_eq!(replay.executed, 0);
        assert_eq!(s.runtime.global_value("count"), Some(Value::Number(4.)));
    }
    #[test]
    fn classic_resource_rolls_back_and_stops_lifecycle_and_replay() {
        let mut s = session(
            "<p id=x>safe</p><script>document.getElementById('x').textContent='bad';while(true){}</script><script>document.title='bad';</script>",
        );
        let report = s.run_initial(&[]);
        assert_eq!(report.executed, 0);
        assert!(s.scripts_disabled);
        assert_eq!(s.state.ready_state, ReadyState::Loading);
        assert_eq!(
            s.document
                .text_content(s.query_selector("#x").unwrap().unwrap()),
            "safe"
        );
        assert!(s.state.events.is_empty());
        assert_eq!(s.run_initial(&[]).executed, 0);
    }
    #[test]
    fn preparation_resource_discards_partial_module_registry() {
        let mut s =
            session("<script type=module src='./root.mjs'></script><script>var bad=1;</script>");
        let report = s.run_initial(&[
            asset(
                "noir-module:https://example.test/root.mjs",
                "import './huge.mjs';",
            ),
            asset(
                "noir-module:https://example.test/huge.mjs",
                &" ".repeat(MAX_INITIAL_SOURCE_BYTES),
            ),
        ]);
        assert_eq!(report.executed, 0);
        assert!(s.scripts_disabled);
        assert_eq!(s.state.ready_state, ReadyState::Loading);
        assert!(s.state.events.is_empty());
        // The previously registered root must be absent after graph rollback.
        let error = s
            .runtime
            .execute_module(
                "https://example.test/root.mjs",
                &mut DomHost {
                    document: &mut s.document,
                    state: &mut s.state,
                },
            )
            .unwrap_err();
        assert_ne!(error.kind, ErrorKind::Resource);
        assert!(error.message.contains("not registered"), "{error}");
    }
    #[test]
    fn total_classic_source_limit_is_fatal_before_source_clone() {
        let mut s = session("<script src='large.js'></script><script>var bad=1;</script>");
        let report = s.run_initial(&[asset("large.js", &" ".repeat(MAX_INITIAL_SOURCE_BYTES + 1))]);
        assert_eq!(report.executed, 0);
        assert!(report.warnings.iter().any(|w| w.contains("512 KiB")));
        assert!(s.scripts_disabled);
        assert_eq!(s.state.ready_state, ReadyState::Loading);
    }
    #[test]
    fn lifecycle_event_resource_stops_following_events_and_timers() {
        for kind in ["readystatechange", "DOMContentLoaded"] {
            let mut s = session(&format!(
                "<p id=x>safe</p><script>document.addEventListener('{kind}',()=>{{document.getElementById('x').textContent='bad';while(true){{}}}});window.onload=()=>{{document.title='bad';}};setTimeout(()=>{{document.title='bad';}},0);</script>"
            ));
            let report = s.run_initial(&[]);
            assert_eq!(report.executed, 1);
            assert!(s.scripts_disabled);
            assert_ne!(s.state.ready_state, ReadyState::Complete);
            assert_eq!(
                s.document
                    .text_content(s.query_selector("#x").unwrap().unwrap()),
                "safe"
            );
            assert!(s.state.timers.is_empty());
            assert!(!s.state.events.iter().any(|event| event.kind == "load"));
        }
    }
    #[test]
    fn deferred_sources_are_frozen_and_future_detached_scripts_are_skipped() {
        let mut s = session(
            "<script>var order='';</script><script id=queued defer src='a.js'></script><script>document.getElementById('queued').remove();document.getElementById('later').remove();</script><script id=later>order+='bad';</script>",
        );
        let report = s.run_initial(&[asset("a.js", "order+='a';")]);
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert_eq!(report.executed, 3);
        assert_eq!(s.runtime.global_value("order"), Some(Value::string("a")));
    }
    #[test]
    fn initialization_guard_is_independent_of_readiness_and_transactions() {
        let mut s = session("<script>var bad=1;</script>");
        s.initial_started = true; // Models a nested entry while the first call is active.
        assert_eq!(s.run_initial(&[]).executed, 0);
        assert_eq!(s.runtime.global_value("bad"), None);
        assert_eq!(s.state.ready_state, ReadyState::Loading);
        assert!(s.state.events.is_empty());
    }
    #[test]
    fn inline_budget_counts_text_below_template_added_by_dom_mutation() {
        let mut s = session("<script id=future></script>");
        let script = s.query_selector("#future").unwrap().unwrap();
        let template = s
            .document
            .append(
                script,
                NodeKind::Element(Element {
                    tag: "template".into(),
                    attributes: BTreeMap::new(),
                }),
            )
            .unwrap();
        s.document
            .append(
                template,
                NodeKind::Text(" ".repeat(MAX_INITIAL_SOURCE_BYTES + 1)),
            )
            .unwrap();
        let mut bytes = 0;
        assert_eq!(
            s.initial_inline_source(script, &mut bytes)
                .unwrap_err()
                .kind,
            ErrorKind::Resource
        );
        let report = s.run_initial(&[]);
        assert_eq!(report.executed, 0);
        assert!(s.scripts_disabled);
        assert_eq!(s.state.ready_state, ReadyState::Loading);
    }
    #[test]
    fn ordinary_module_preparation_failure_removes_partial_records_and_continues() {
        let mut s = session(
            "<script type=module src='./broken.mjs'></script><script type=module src='./good.mjs'></script>",
        );
        let report = s.run_initial(&[
            asset(
                "noir-module:https://example.test/broken.mjs",
                "import './first.mjs';",
            ),
            asset(
                "noir-module:https://example.test/first.mjs",
                "import './missing.mjs';",
            ),
            asset(
                "noir-module:https://example.test/good.mjs",
                "document.title='good';",
            ),
        ]);
        assert_eq!(report.executed, 1);
        assert_eq!(report.warnings.len(), 1);
        assert_eq!(s.state.ready_state, ReadyState::Complete);
        assert_eq!(s.document.document_title(), "good");
        for url in [
            "https://example.test/broken.mjs",
            "https://example.test/first.mjs",
        ] {
            let error = s
                .runtime
                .execute_module(
                    url,
                    &mut DomHost {
                        document: &mut s.document,
                        state: &mut s.state,
                    },
                )
                .unwrap_err();
            assert!(error.message.contains("not registered"), "{error}");
        }
    }
}
