use super::*;

impl DomHost<'_> {
    pub(super) fn dispatch_existing(
        &mut self,
        runtime: &mut Runtime,
        event: usize,
        path: Vec<u64>,
        transactional: bool,
    ) -> Result<(), JsError> {
        if self.state.dispatch_depth >= MAX_DISPATCH_DEPTH {
            return Err(JsError::resource("Event dispatch depth exceeded"));
        }
        self.state.dispatch_depth += 1;
        self.state.events[event].dispatching = true;
        self.state.events[event].target = Some(path[0]);
        let result = (|| {
            let mut first_error = None;
            for &current in path.iter().skip(1).rev() {
                self.dispatch_matching(
                    runtime,
                    transactional,
                    event,
                    current,
                    true,
                    1,
                    &mut first_error,
                )?;
                if self.state.events[event].stopped {
                    return first_error.map_or(Ok(()), Err);
                }
            }
            self.dispatch_matching(
                runtime,
                transactional,
                event,
                path[0],
                true,
                2,
                &mut first_error,
            )?;
            if !self.state.events[event].immediate {
                self.dispatch_matching(
                    runtime,
                    transactional,
                    event,
                    path[0],
                    false,
                    2,
                    &mut first_error,
                )?;
            }
            if self.state.events[event].bubbles && !self.state.events[event].stopped {
                for &current in path.iter().skip(1) {
                    self.dispatch_matching(
                        runtime,
                        transactional,
                        event,
                        current,
                        false,
                        3,
                        &mut first_error,
                    )?;
                    if self.state.events[event].stopped {
                        break;
                    }
                }
            }
            first_error.map_or(Ok(()), Err)
        })();
        if let Some(event) = self.state.events.get_mut(event) {
            event.current = None;
            event.phase = 0;
            event.passive = false;
            event.stopped = false;
            event.immediate = false;
            event.dispatching = false;
        }
        self.state.dispatch_depth -= 1;
        result
    }
    #[allow(clippy::too_many_arguments)]
    fn dispatch_matching(
        &mut self,
        runtime: &mut Runtime,
        transactional: bool,
        event: usize,
        current: u64,
        capture: bool,
        phase: u8,
        first_error: &mut Option<JsError>,
    ) -> Result<(), JsError> {
        // Each capture/non-capture group is a separate invocation.
        if self.state.events[event].stopped {
            return Ok(());
        }
        let kind = self.state.events[event].kind.clone();
        self.state.events[event].current = Some(current);
        self.state.events[event].phase = phase;
        let callbacks = self
            .state
            .listeners
            .iter()
            .filter(|l| l.target == current && l.kind == kind && l.options.capture == capture)
            .cloned()
            .collect::<Vec<_>>();
        for listener in callbacks {
            if self.state.events[event].immediate {
                break;
            }
            if !self
                .state
                .listeners
                .iter()
                .any(|pending| pending.id == listener.id)
            {
                continue;
            }
            if listener.options.once {
                self.state
                    .listeners
                    .retain(|pending| pending.id != listener.id);
            }
            let old_document = transactional.then(|| self.document.clone());
            let old_state = transactional.then(|| self.state.clone());
            let old_runtime = transactional.then(|| runtime.clone());
            self.state.events[event].passive = listener.options.passive;
            let mut result = runtime.invoke_with_this(
                listener.callback,
                vec![Value::Host(EVENT_BASE + event as u64)],
                Value::Host(current),
                self,
            );
            self.state.events[event].passive = false;
            if listener.property
                && result
                    .as_ref()
                    .is_ok_and(|value| *value == Value::Bool(false))
                && self.state.events[event].cancelable
            {
                self.state.events[event].canceled = true;
            }
            if let Err(error) = &result
                && error.kind != ErrorKind::Resource
                && let Err(handler_error) = self.report_event_error(runtime, error)
            {
                result = Err(handler_error);
            }
            if let Err(error) = result {
                if error.kind == ErrorKind::Resource {
                    if let (Some(document), Some(state), Some(snapshot)) =
                        (old_document, old_state, old_runtime)
                    {
                        *self.document = document;
                        *self.state = state;
                        runtime.rollback_to(snapshot);
                    }
                    return Err(error);
                }
                if transactional {
                    first_error.get_or_insert(error);
                }
            }
        }
        Ok(())
    }
}

impl DomHost<'_> {
    pub(super) fn construct_event(
        &mut self,
        object: u64,
        args: &[Value],
        runtime: &mut Runtime,
    ) -> Result<Value, JsError> {
        if !matches!(object, EVENT_CONSTRUCTOR | CUSTOM_EVENT_CONSTRUCTOR) {
            return Err(JsError::runtime("Unsupported host constructor"));
        }
        if self.state.events.len() >= MAX_EVENTS {
            return Err(JsError::resource("DOM event count/type budget exceeded"));
        }
        let value = args
            .first()
            .ok_or_else(|| JsError::runtime("Event type required"))?;
        let kind = runtime.coerce_to_string(value.clone(), self)?;
        if kind.len() > 64 {
            return Err(JsError::resource("DOM event type exceeds 64 bytes"));
        }
        let mut bubbles = false;
        let mut cancelable = false;
        let mut detail = Value::Null;
        if let Some(init) = args.get(1) {
            match init {
                Value::Undefined | Value::Null => {}
                Value::Object(_)
                | Value::Array(_)
                | Value::Function(_)
                | Value::Native(_)
                | Value::Host(_) => {
                    bubbles = runtime
                        .read_property(init.clone(), "bubbles", self)?
                        .truthy();
                    cancelable = runtime
                        .read_property(init.clone(), "cancelable", self)?
                        .truthy();
                    if runtime
                        .read_property(init.clone(), "composed", self)?
                        .truthy()
                    {
                        return Err(JsError::runtime(
                            "Composed events and Shadow DOM are not implemented",
                        ));
                    }
                    if object == CUSTOM_EVENT_CONSTRUCTOR {
                        let value = runtime.read_property(init.clone(), "detail", self)?;
                        if value != Value::Undefined {
                            detail = value;
                        }
                    }
                }
                _ => return Err(JsError::runtime("Event init must be an object or null")),
            }
        }
        // Getters and type coercion may recursively construct events.
        if self.state.events.len() >= MAX_EVENTS {
            return Err(JsError::resource("DOM event count/type budget exceeded"));
        }
        let index = self.state.events.len();
        self.state.events.push(DomEvent {
            kind,
            target: None,
            detail,
            custom: object == CUSTOM_EVENT_CONSTRUCTOR,
            trusted: false,
            dispatching: false,
            current: None,
            phase: 0,
            bubbles,
            cancelable,
            canceled: false,
            stopped: false,
            immediate: false,
            passive: false,
            timestamp: self.state.clock,
        });
        Ok(Value::Host(EVENT_BASE + index as u64))
    }
    pub(super) fn dispatch_programmatic(
        &mut self,
        runtime: &mut Runtime,
        target: u64,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let object = args
            .first()
            .and_then(Value::as_host)
            .ok_or_else(|| JsError::runtime("dispatchEvent requires a genuine Event"))?;
        let event = self.event(object)?;
        if event.dispatching || event.kind.is_empty() {
            return Err(JsError::runtime(
                "Event is already dispatching or has an empty type",
            ));
        }
        if self.state.dispatch_depth >= MAX_DISPATCH_DEPTH {
            return Err(JsError::resource("Event dispatch depth exceeded"));
        }
        let index = (object - EVENT_BASE) as usize;
        let path = self.event_path(target)?;
        self.state.events[index].trusted = false;
        self.dispatch_existing(runtime, index, path, false)?;
        Ok(Value::Bool(!self.state.events[index].canceled))
    }
    fn event_path(&self, target: u64) -> Result<Vec<u64>, JsError> {
        if target == WINDOW {
            return Ok(vec![WINDOW]);
        }
        if target == DOC {
            return Ok(vec![DOC, WINDOW]);
        }
        let mut node = self.node(target)?;
        let mut path = vec![target];
        while let Some(parent) = self.document.nodes[node].parent {
            if path.len() >= MAX_DEPTH {
                return Err(JsError::resource("Event path depth exceeded"));
            }
            path.push(Self::node_value(parent).as_host().expect("node host"));
            node = parent;
        }
        // A detached tree never acquires synthetic document/window ancestors.
        if path.last() == Some(&DOC) {
            path.push(WINDOW);
        }
        Ok(path)
    }
    fn report_event_error(
        &mut self,
        runtime: &mut Runtime,
        error: &JsError,
    ) -> Result<(), JsError> {
        if self.state.logs.len() < 32 {
            self.state.logs.push(
                format!("Event listener: {error}")
                    .chars()
                    .take(1024)
                    .collect(),
            );
        }
        let callback = self
            .state
            .listeners
            .iter()
            .find(|listener| {
                listener.target == WINDOW && listener.kind == "error" && listener.property
            })
            .map(|listener| listener.callback.clone());
        let Some(callback) = callback else {
            return Ok(());
        };
        let args = vec![
            Value::string(error.message.chars().take(1024).collect::<String>()),
            Value::string(""),
            Value::Number(0.),
            Value::Number(0.),
            error.thrown.clone().unwrap_or(Value::Null),
        ];
        match runtime.invoke_with_this(callback, args, Value::Host(WINDOW), self) {
            Ok(_) => Ok(()),
            Err(error) if error.kind == ErrorKind::Resource => Err(error),
            Err(error) => {
                if self.state.logs.len() < 32 {
                    self.state.logs.push(
                        format!("window.onerror: {error}")
                            .chars()
                            .take(1024)
                            .collect(),
                    );
                }
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn session() -> DocumentSession {
        DocumentSession::new(
            web_html::parse("<div id=p><button id=b>Before</button></div>").unwrap(),
        )
    }
    fn yes(session: &mut DocumentSession, source: &str) {
        assert_eq!(session.execute(source).unwrap(), Value::Bool(true));
    }
    #[test]
    fn construction_metadata_defaults_and_explicit_capability_errors() {
        let mut s = session();
        yes(
            &mut s,
            "const e=new Event('x'); const c=new CustomEvent('x'); typeof Event==='function' && typeof CustomEvent==='function' && Event.name==='Event' && CustomEvent.name==='CustomEvent' && Event.length===1 && CustomEvent.length===1 && e.target===null && e.currentTarget===null && e.eventPhase===0 && !e.bubbles && !e.cancelable && !e.isTrusted && !e.composed && !e.defaultPrevented && c.detail===null;",
        );
        for source in [
            "Event('x');",
            "Event.call(null,'x');",
            "Event.apply(null,['x']);",
            "Reflect.apply(Event,null,['x']);",
            "Event.bind(null)('x');",
            "new Event();",
            "new Event('x',1);",
            "Event.prototype;",
            "CustomEvent.prototype;",
            "e instanceof Event;",
            "class Derived extends Event {}",
            "new Event('x',{composed:true});",
            "document.dispatchEvent({type:'x'});",
            "document.dispatchEvent(new Event(''));",
        ] {
            assert!(s.execute(source).is_err(), "{source}");
        }
        yes(
            &mut s,
            "new Event(undefined).type==='undefined' && new Event('x',null).bubbles===false && Reflect.construct(Event,['x']).type==='x' && new (CustomEvent.bind(null,'x'))().detail===null;",
        );
    }
    #[test]
    fn observable_conversion_dictionary_order_and_detail_roots() {
        let mut s = session();
        yes(
            &mut s,
            "let order=''; let payload={answer:42}; const options={}; Object.defineProperty(options,'bubbles',{get:function(){order+='b';return true;},enumerable:true,configurable:true}); Object.defineProperty(options,'cancelable',{get:function(){order+='c';return true;},enumerable:true,configurable:true}); Object.defineProperty(options,'composed',{get:function(){order+='s';return false;},enumerable:true,configurable:true}); Object.defineProperty(options,'detail',{get:function(){order+='d';return payload;},enumerable:true,configurable:true}); const c=new CustomEvent({toString(){order+='t';return 'x';}},options); const weak=new WeakRef(payload); payload=null; order==='tbcsd' && c.detail.answer===42;",
        );
        s.runtime
            .collect_garbage(
                &[],
                &DomHost {
                    document: &mut s.document,
                    state: &mut s.state,
                },
            )
            .unwrap();
        yes(&mut s, "weak.deref()===c.detail && c.detail.answer===42;");
        yes(
            &mut s,
            "let abrupt=''; const invalid={}; Object.defineProperty(invalid,'bubbles',{get:function(){throw 'stop';},enumerable:true,configurable:true}); Object.defineProperty(invalid,'cancelable',{get:function(){abrupt='bad';},enumerable:true,configurable:true}); try {new Event('x',invalid);} catch(e) {abrupt=e;} abrupt==='stop';",
        );
    }
    #[test]
    fn phases_this_frozen_path_and_detached_nodes() {
        let mut s = session();
        yes(
            &mut s,
            "const b=document.getElementById('b');const p=document.getElementById('p');let seen='';window.addEventListener('x',function(e){seen+='w'+e.eventPhase;},{capture:true});document.addEventListener('x',function(e){seen+='d'+e.eventPhase;},{capture:true});p.addEventListener('x',function(e){seen+='p'+e.eventPhase;b.remove();},{capture:true});b.addEventListener('x',function(e){if(this===b&&e.target===b&&e.currentTarget===b)seen+='b'+e.eventPhase;});p.addEventListener('x',e=>{seen+='P'+e.eventPhase;});const e=new Event('x',{bubbles:true});b.dispatchEvent(e);seen==='w1d1p1b2P3'&&e.target===b&&e.currentTarget===null&&e.eventPhase===0;",
        );
        yes(
            &mut s,
            "seen='';b.dispatchEvent(e);seen==='b2' && !e.isTrusted;",
        );
        yes(
            &mut s,
            "let calls=0;const text=document.createTextNode('t');text.addEventListener('text',()=>{calls++;});text.dispatchEvent(new Event('text'));calls===1;",
        );
    }
    #[test]
    fn cancellation_passive_once_removal_nested_and_replay() {
        let mut s = session();
        yes(
            &mut s,
            "const b=document.getElementById('b');let seen='';const e=new Event('x',{cancelable:true});function removed(){seen+='bad';}b.addEventListener('x',function(ev){ev.preventDefault();seen+='p';},{passive:true,once:true});b.addEventListener('x',function(ev){if(!ev.defaultPrevented)seen+='a';ev.preventDefault();b.removeEventListener('x',removed);try{b.dispatchEvent(ev);}catch(err){seen+='r';}b.dispatchEvent(new Event('nested'));ev.stopImmediatePropagation();},{once:true});b.addEventListener('x',removed);b.addEventListener('nested',()=>{seen+='n';});const result=b.dispatchEvent(e);!result&&e.defaultPrevented&&seen==='parn'&&!e.cancelBubble;",
        );
        yes(
            &mut s,
            "b.addEventListener('x',()=>{seen+='z';});!b.dispatchEvent(e)&&seen==='parnz'&&e.eventPhase===0&&e.currentTarget===null;",
        );
        yes(
            &mut s,
            "const non=new Event('no');non.preventDefault();b.dispatchEvent(non)&&!non.defaultPrevented;",
        );
    }
    #[test]
    fn listener_errors_do_not_escape_and_jobs_follow_dispatch() {
        let mut s = session();
        yes(
            &mut s,
            "let order='';window.onerror=()=>{order+='E';};document.addEventListener('x',()=>{order+='a';queueMicrotask(()=>{order+='m';});throw 'broken';});document.addEventListener('x',()=>{order+='b';});let escaped=false;try{document.dispatchEvent(new Event('x'));order+='c';}catch(e){escaped=true;}!escaped&&order==='aEbc';",
        );
        yes(&mut s, "order==='aEbcm';");
        assert!(
            s.state
                .logs
                .iter()
                .any(|line| line.contains("Event listener"))
        );
    }
    #[test]
    fn resource_limits_roll_back_outer_script_without_active_runtime_rollback() {
        let mut s = session();
        let before = s.document.document_title();
        s.execute("document.addEventListener('x',()=>{document.title='lost';new Event('x'.repeat(65));});").unwrap();
        let error = s
            .execute("document.dispatchEvent(new Event('x'));document.title='unreachable';")
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
        assert_eq!(s.document.document_title(), before);
        assert!(s.scripts_disabled);
        assert_eq!(s.state.dispatch_depth, 0);
        let mut s = session();
        s.execute("for(let i=0;i<256;i++){new Event('x');}")
            .unwrap();
        assert_eq!(s.state.events.len(), MAX_EVENTS);
        assert_eq!(
            s.execute("new Event('overflow');").unwrap_err().kind,
            ErrorKind::Resource
        );
        let mut s = session();
        s.execute("document.addEventListener('x',()=>{document.dispatchEvent(new Event('x'));});")
            .unwrap();
        assert_eq!(
            s.execute("document.dispatchEvent(new Event('x'));")
                .unwrap_err()
                .kind,
            ErrorKind::Resource
        );
        assert_eq!(s.state.dispatch_depth, 0);
    }
    #[test]
    fn native_callback_rollback_contains_nested_dispatch_resources() {
        let mut s = session();
        s.execute("const b=document.getElementById('b');b.addEventListener('click',()=>{b.textContent='committed';});b.addEventListener('click',()=>{b.textContent='lost';document.dispatchEvent(new Event('x'));});document.addEventListener('x',()=>{new Event('x'.repeat(65));});").unwrap();
        let id = s.query_selector("#b").unwrap().unwrap();
        assert_eq!(
            s.dispatch_event(id, "click").unwrap_err().kind,
            ErrorKind::Resource
        );
        assert_eq!(s.document.text_content(id), "committed");
        assert_eq!(s.state.dispatch_depth, 0);
        assert!(
            s.state
                .events
                .iter()
                .all(|event| !event.dispatching && event.current.is_none() && event.phase == 0)
        );
    }

    #[test]
    fn node_and_window_dispatch_keep_jobs_after_outer_script_and_require_receivers() {
        let mut s = session();
        yes(
            &mut s,
            "const b=document.getElementById('b');let order='';b.addEventListener('x',()=>{order+='a';queueMicrotask(()=>{order+='m';});});b.addEventListener('x',()=>{order+='b';});const dispatch=b.dispatchEvent;dispatch.call(b,new Event('x'));order+='c';order==='abc';",
        );
        yes(&mut s, "order==='abcm';");
        yes(
            &mut s,
            "let n=0;window.addEventListener('x',function(e){if(this===window&&e.target===window&&e.eventPhase===2)n++;});dispatchEvent(new Event('x'));window.dispatchEvent(new Event('x'));n===2;",
        );
        assert!(s.execute("dispatch.call({},new Event('x'));").is_err());
        assert!(
            s.execute("window.dispatchEvent.call({},new Event('x'));")
                .is_err()
        );
    }
    #[test]
    fn listener_quota_and_native_redispatch_trust_are_explicit() {
        let mut s = session();
        s.execute("for(let i=0;i<128;i++){document.addEventListener('x'+i,()=>{});}")
            .unwrap();
        assert_eq!(
            s.execute("document.addEventListener('extra',()=>{});")
                .unwrap_err()
                .kind,
            ErrorKind::Resource
        );
        let mut s = session();
        s.execute("const b=document.getElementById('b');let saved=null;let trusted=false;b.addEventListener('click',e=>{saved=e;trusted=e.isTrusted;},{once:true});").unwrap();
        let id = s.query_selector("#b").unwrap().unwrap();
        s.dispatch_event(id, "click").unwrap();
        yes(&mut s, "trusted&&saved.isTrusted;");
        yes(
            &mut s,
            "b.dispatchEvent(saved);!saved.isTrusted&&saved.currentTarget===null;",
        );
    }

    #[test]
    fn prestopped_events_skip_callbacks_then_can_be_reused() {
        let mut s = session();
        yes(
            &mut s,
            "let calls=0;window.addEventListener('x',()=>{calls++;},{capture:true});window.addEventListener('x',()=>{calls++;});document.addEventListener('x',()=>{calls++;});const e=new Event('x');e.stopPropagation();window.dispatchEvent(e);calls===0&&!e.cancelBubble&&e.currentTarget===null&&e.eventPhase===0;",
        );
        yes(&mut s, "window.dispatchEvent(e);calls===2;");
        yes(
            &mut s,
            "e.stopPropagation();document.dispatchEvent(e);calls===2&&!e.cancelBubble;",
        );
        yes(&mut s, "document.dispatchEvent(e);calls===4;");
        yes(
            &mut s,
            "e.stopImmediatePropagation();document.dispatchEvent(e);calls===4&&!e.cancelBubble;",
        );
        yes(&mut s, "document.dispatchEvent(e);calls===6;");
    }

    #[test]
    fn target_capture_stop_keeps_its_group_but_skips_target_bubble() {
        let mut s = session();
        yes(
            &mut s,
            "const b=document.getElementById('b');let seen='';b.addEventListener('x',e=>{seen+='a';e.stopPropagation();},{capture:true});b.addEventListener('x',()=>{seen+='b';},{capture:true});b.addEventListener('x',()=>{seen+='bad';});const e=new Event('x',{bubbles:true});b.dispatchEvent(e);seen==='ab'&&!e.cancelBubble&&e.currentTarget===null&&e.eventPhase===0;",
        );
    }
}
