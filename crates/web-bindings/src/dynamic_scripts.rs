//! External classic scripts are queued as data, never fetched by this host.
//! The broker binds requests to the retained document, supplies bounded bytes,
//! and delivers completions in insertion order at a later operation boundary.
use super::*;
use browser_ipc::{
    DynamicScriptRequest, DynamicScriptResponse, MAX_DYNAMIC_SCRIPT_BYTES,
    MAX_DYNAMIC_SCRIPT_SRC_BYTES, MAX_DYNAMIC_SCRIPT_TOTAL_BYTES, MAX_DYNAMIC_SCRIPTS,
};
use std::collections::BTreeSet;

#[derive(Debug, Clone)]
struct Pending {
    request: DynamicScriptRequest,
    node: NodeId,
    sent: bool,
}

#[derive(Debug, Clone)]
struct InlineScript {
    index: usize,
    node: NodeId,
    source: String,
}

#[derive(Debug, Clone, Default)]
pub(super) struct DynamicScripts {
    // Only script elements made by createElement (and their unstarted clones)
    // are candidates. Parser/innerHTML scripts cannot be resurrected by moving.
    pub candidates: BTreeSet<NodeId>,
    started: BTreeSet<NodeId>,
    pub ordered: BTreeSet<NodeId>,
    pending: Vec<Pending>,
    inline: Vec<InlineScript>,
    warnings: Vec<String>,
    attempts: usize,
    source_bytes: usize,
    pub current: Option<NodeId>,
}

impl DynamicScripts {
    pub(super) fn clear(&mut self) {
        self.pending.clear();
        self.inline.clear();
        self.current = None;
    }

    pub(super) fn clone_node(&mut self, original: NodeId, clone: NodeId) {
        if self.candidates.contains(&original) {
            self.candidates.insert(clone);
            if self.started.contains(&original) {
                self.started.insert(clone);
            }
            if self.ordered.contains(&original) {
                self.ordered.insert(clone);
            }
        }
    }
}

impl DomHost<'_> {
    pub(super) fn prepare_dynamic_subtree(&mut self, root: NodeId) -> Result<(), JsError> {
        if !self.connected_node(root)? || self.document.is_inactive_subtree(root) {
            return Ok(());
        }
        let mut pending = vec![root];
        while let Some(node) = pending.pop() {
            self.charge_tree_read(1)?;
            if self
                .document
                .element(node)
                .is_some_and(|e| matches!(e.tag.as_str(), "template" | "noscript"))
            {
                continue;
            }
            self.prepare_dynamic_script(node)?;
            self.charge_tree_read(self.document.nodes[node].children.len())?;
            pending.extend(self.document.nodes[node].children.iter().rev().copied());
        }
        Ok(())
    }

    pub(super) fn prepare_dynamic_script(&mut self, node: NodeId) -> Result<(), JsError> {
        if !self.state.dynamic_scripts.candidates.contains(&node)
            || self.state.dynamic_scripts.started.contains(&node)
            || !self.connected_node(node)?
            || self.document.is_inactive_subtree(node)
        {
            return Ok(());
        }
        let element = self.document.element(node).expect("script candidate");
        let kind = element
            .attr("type")
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if kind == "module" {
            return Err(JsError::runtime(
                "Dynamic module scripts are not implemented",
            ));
        }
        if element.attr("nomodule").is_some()
            || !matches!(
                kind.as_str(),
                "" | "text/javascript" | "application/javascript" | "text/ecmascript"
            )
        {
            return Ok(());
        }
        let Some(src) = element.attr("src") else {
            return self.prepare_inline_script(node);
        };
        if src.len() > MAX_DYNAMIC_SCRIPT_SRC_BYTES {
            return Err(JsError::resource("Dynamic script URL exceeds 8192 bytes"));
        }
        // Invalid URL data is still a real, bounded load attempt. The broker
        // receives a harmless nonempty sentinel and sends an error completion.
        let src = if src.is_empty() || src.chars().any(char::is_control) {
            "noir-invalid-script:".to_owned()
        } else {
            src.to_owned()
        };
        let scripts = &mut self.state.dynamic_scripts;
        if scripts.attempts >= MAX_DYNAMIC_SCRIPTS {
            return Err(JsError::resource("Dynamic script attempt limit exceeded"));
        }
        scripts.attempts += 1;
        scripts.started.insert(node);
        scripts.pending.push(Pending {
            request: DynamicScriptRequest {
                id: scripts.attempts as u64,
                src,
            },
            node,
            sent: false,
        });
        Ok(())
    }

    fn prepare_inline_script(&mut self, node: NodeId) -> Result<(), JsError> {
        // Count borrowed text before allocating a combined source. Empty
        // scripts remain unstarted so later text/src insertion can prepare them.
        let mut bytes = 0usize;
        let mut pending = vec![node];
        while let Some(id) = pending.pop() {
            self.charge_tree_read(1)?;
            if let NodeKind::Text(text) = &self.document.nodes[id].kind {
                bytes = bytes.saturating_add(text.len());
            }
            if bytes > MAX_DYNAMIC_SCRIPT_BYTES {
                return Err(JsError::resource(
                    "Dynamic inline script source exceeds 128 KiB",
                ));
            }
            self.charge_tree_read(self.document.nodes[id].children.len())?;
            pending.extend(self.document.nodes[id].children.iter().rev().copied());
        }
        if bytes == 0 {
            return Ok(());
        }
        self.charge_tree_read(bytes)?;
        let scripts = &mut self.state.dynamic_scripts;
        if scripts.attempts >= MAX_DYNAMIC_SCRIPTS
            || bytes > MAX_DYNAMIC_SCRIPT_TOTAL_BYTES.saturating_sub(scripts.source_bytes)
        {
            return Err(JsError::resource(
                "Dynamic script count or source budget exceeded",
            ));
        }
        scripts.attempts += 1;
        scripts.source_bytes += bytes;
        scripts.started.insert(node);
        scripts.inline.push(InlineScript {
            index: scripts.attempts - 1,
            node,
            source: self.document.text_content(node),
        });
        Ok(())
    }

    /// Flush only this mutation's captured batch. A nested insertion executes
    /// its own batch before its caller resumes, never an outer sibling's batch.
    pub(super) fn run_inline_scripts(&mut self, runtime: &mut Runtime) -> Result<(), JsError> {
        let ready = std::mem::take(&mut self.state.dynamic_scripts.inline);
        for script in ready {
            let previous = self.state.dynamic_scripts.current.replace(script.node);
            let result = runtime.execute_nested_script(&script.source, self);
            self.state.dynamic_scripts.current = previous;
            if let Err(error) = result {
                if error.kind == ErrorKind::Resource {
                    return Err(error);
                }
                // HTML script evaluation reports ordinary errors. They are not
                // thrown by appendChild/the setter into the inserting script.
                self.report_inline_error(
                    runtime,
                    script.index,
                    script.node,
                    &script.source,
                    &error,
                )?;
            }
        }
        Ok(())
    }

    fn report_inline_error(
        &mut self,
        runtime: &mut Runtime,
        index: usize,
        node: NodeId,
        source: &str,
        error: &JsError,
    ) -> Result<(), JsError> {
        let warning = diagnostics::ScriptOrigin::capture(
            index,
            node,
            false,
            false,
            None,
            self.state.document_url.as_ref(),
        )
        .warning("dynamic-inline", error, Some(source));
        if self.state.logs.len() < 32 {
            self.state.logs.push(warning.clone());
        }
        if self.state.dynamic_scripts.warnings.len() < 32 {
            self.state.dynamic_scripts.warnings.push(warning);
        }
        let callback = self
            .state
            .listeners
            .iter()
            .find(|listener| {
                listener.target == WINDOW && listener.kind == "error" && listener.property
            })
            .map(|listener| listener.callback.clone());
        if let Some(callback) = callback {
            let args = vec![
                Value::string(error.message.chars().take(1024).collect::<String>()),
                Value::string(""),
                Value::Number(0.),
                Value::Number(0.),
                error.thrown.clone().unwrap_or(Value::Null),
            ];
            match runtime.invoke_with_this(callback, args, Value::Host(WINDOW), self) {
                Err(error) if error.kind == ErrorKind::Resource => return Err(error),
                Err(error) if self.state.logs.len() < 32 => {
                    self.state.logs.push(diagnostics::callback_warning(
                        "error",
                        Some("window.onerror"),
                        &error,
                    ));
                }
                _ => {}
            }
        }
        Ok(())
    }

    pub(super) fn script_property(
        &mut self,
        id: NodeId,
        property: &str,
    ) -> Result<Option<Value>, JsError> {
        if !self.document.element(id).is_some_and(|e| e.tag == "script") {
            return Ok(None);
        }
        let element = self.document.element(id).unwrap();
        Ok(Some(match property {
            "src" => {
                let value = element.attr("src").unwrap_or("");
                Value::string(if element.attr("src").is_some() {
                    self.state
                        .document_url
                        .as_ref()
                        .and_then(|base| base.join(value).ok())
                        .map_or_else(|| value.to_owned(), |url| url.to_string())
                } else {
                    String::new()
                })
            }
            "type" => Value::string(element.attr("type").unwrap_or("")),
            "text" => Value::string(self.document.text_content(id)),
            "async" => Value::Bool(
                element.attr("async").is_some()
                    || self.state.dynamic_scripts.candidates.contains(&id)
                        && !self.state.dynamic_scripts.ordered.contains(&id),
            ),
            "defer" | "noModule" => Value::Bool(
                element
                    .attr(if property == "noModule" {
                        "nomodule"
                    } else {
                        property
                    })
                    .is_some(),
            ),
            _ => return Ok(None),
        }))
    }

    pub(super) fn set_script_property(
        &mut self,
        id: NodeId,
        property: &str,
        value: &Value,
    ) -> Result<bool, JsError> {
        if !self.document.element(id).is_some_and(|e| e.tag == "script") {
            return Ok(false);
        }
        match property {
            "src" | "type" => self.set_attribute(id, property.into(), value.primitive_text()?)?,
            "text" => self.replace_text(id, value.primitive_text()?)?,
            "async" | "defer" | "noModule" => {
                self.mutate(0)?;
                if property == "async" {
                    self.state.dynamic_scripts.ordered.insert(id);
                }
                let key = if property == "noModule" {
                    "nomodule"
                } else {
                    property
                };
                let NodeKind::Element(element) = &mut self.document.nodes[id].kind else {
                    unreachable!()
                };
                if value.truthy() {
                    element.attributes.insert(key.into(), String::new());
                } else {
                    element.attributes.remove(key);
                }
            }
            _ => return Ok(false),
        }
        Ok(true)
    }
}

impl DocumentSession {
    pub fn dynamic_script_warnings(&self) -> &[String] {
        &self.state.dynamic_scripts.warnings
    }

    pub fn take_script_requests(&mut self) -> Vec<DynamicScriptRequest> {
        if self.scripts_disabled {
            return Vec::new();
        }
        self.state
            .dynamic_scripts
            .pending
            .iter_mut()
            .filter_map(|pending| {
                if pending.sent {
                    None
                } else {
                    pending.sent = true;
                    Some(pending.request.clone())
                }
            })
            .collect()
    }

    /// Only an exact, ordered completion of the outstanding batch is accepted.
    /// Validate all sizes/identities before evaluating any supplied source.
    pub fn complete_scripts(
        &mut self,
        responses: &[DynamicScriptResponse],
    ) -> Result<ScriptReport, JsError> {
        let expected: Vec<_> = self
            .state
            .dynamic_scripts
            .pending
            .iter()
            .filter(|pending| pending.sent)
            .cloned()
            .collect();
        if self.scripts_disabled
            || expected.is_empty()
            || responses.len() != expected.len()
            || responses
                .iter()
                .zip(&expected)
                .any(|(response, pending)| response.id != pending.request.id)
        {
            return Err(JsError::runtime(
                "Unexpected dynamic script completion identity or order",
            ));
        }
        let mut bytes = self.state.dynamic_scripts.source_bytes;
        for response in responses {
            let size = response.source.as_ref().map_or(0, String::len);
            if size > MAX_DYNAMIC_SCRIPT_BYTES
                || size > MAX_DYNAMIC_SCRIPT_TOTAL_BYTES.saturating_sub(bytes)
            {
                self.disable_scripts();
                return Err(JsError::resource("Dynamic script source budget exceeded"));
            }
            bytes += size;
        }
        self.state.dynamic_scripts.source_bytes = bytes;
        self.state
            .dynamic_scripts
            .pending
            .retain(|pending| !pending.sent);
        let mut report = ScriptReport::default();
        for (response, pending) in responses.iter().zip(expected) {
            let kind = if let Some(source) = &response.source {
                self.state.dynamic_scripts.current = Some(pending.node);
                let result = self.execute(source);
                self.state.dynamic_scripts.current = None;
                match result {
                    Ok(_) => report.executed += 1,
                    Err(error) => {
                        if report.warnings.len() < 32 {
                            report.warnings.push(
                                diagnostics::ScriptOrigin::capture(
                                    response.id as usize - 1,
                                    pending.node,
                                    false,
                                    false,
                                    Some(&pending.request.src),
                                    self.state.document_url.as_ref(),
                                )
                                .warning(
                                    "dynamic-execute",
                                    &error,
                                    Some(source),
                                ),
                            );
                        }
                        if error.kind == ErrorKind::Resource {
                            break;
                        }
                    }
                }
                // A runtime exception does not turn a successful download into
                // a network error. execute already reports it to window.onerror.
                "load"
            } else {
                "error"
            };
            if let Err(error) = self.dispatch_target(NODE + pending.node as u64, kind) {
                if report.warnings.len() < 32 {
                    report.warnings.push(diagnostics::callback_warning(
                        "callback",
                        Some(kind),
                        &error,
                    ));
                }
                if error.kind == ErrorKind::Resource {
                    break;
                }
            }
        }
        Ok(report)
    }
}

#[cfg(test)]
mod inline_tests;
#[cfg(test)]
mod tests;
