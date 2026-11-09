//! Bounded UTF-8 session storage. No named properties, Storage prototype or events.
//! Isolated UTF-16 surrogates are rejected by the observable host string boundary.
use super::*;
use browser_ipc::{
    SESSION_STORAGE_MAX_ENTRIES, SESSION_STORAGE_MAX_KEY_BYTES, SESSION_STORAGE_MAX_TOTAL_BYTES,
    SESSION_STORAGE_MAX_VALUE_BYTES, SessionStorageAccess, SessionStorageSeed,
    SessionStorageSnapshot, validate_session_storage_seed,
};
pub(super) const OBJECT: u64 = 9;
pub(super) const METHOD: u64 = 90;
pub(super) const METHODS: [&str; 5] = ["key", "getItem", "setItem", "removeItem", "clear"];
#[derive(Debug, Clone, Default)]
pub(super) struct StorageState {
    seed: SessionStorageSeed,
    origin: Option<String>,
    initialized: bool,
    bytes: usize,
}
impl DocumentSession {
    /// Trusted supervisor metadata only, once, before any page execution.
    pub fn set_session_storage(&mut self, seed: SessionStorageSeed) -> Result<(), JsError> {
        if self.storage_started || self.initial_started || self.state.storage.initialized {
            return Err(JsError::runtime("Session storage seed is already fixed"));
        }
        validate_session_storage_seed(&seed)
            .map_err(|_| JsError::runtime("Invalid session storage seed"))?;
        let origin = self
            .state
            .document_url
            .as_ref()
            .filter(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some())
            .map(|url| url.origin().ascii_serialization());
        if seed.access != SessionStorageAccess::Unavailable && origin.is_none() {
            return Err(JsError::runtime(
                "Session storage requires an HTTP(S) document origin",
            ));
        }
        let bytes = seed.entries.iter().map(|(k, v)| k.len() + v.len()).sum();
        self.state.storage = StorageState {
            seed,
            origin,
            initialized: true,
            bytes,
        };
        Ok(())
    }
    /// Call after the operation and its resource rollback have completed.
    pub fn session_storage_snapshot(&self) -> SessionStorageSnapshot {
        self.state.storage.seed.entries.clone()
    }
}
impl DomHost<'_> {
    pub(super) fn storage_available(&mut self) -> Result<(), JsError> {
        self.charge_tree_read(1)?;
        let storage = &self.state.storage;
        if storage.seed.access == SessionStorageAccess::Unavailable
            || storage.origin.as_deref()
                != self
                    .state
                    .document_url
                    .as_ref()
                    .filter(|url| matches!(url.scheme(), "http" | "https"))
                    .map(|url| url.origin().ascii_serialization())
                    .as_deref()
        {
            return Err(JsError::runtime(
                "Session storage is unavailable for this document origin",
            ));
        }
        Ok(())
    }
    pub(super) fn storage_get(&mut self, object: u64, property: &str) -> Result<Value, JsError> {
        self.storage_available()?;
        if object == WINDOW {
            return Ok(Value::Host(OBJECT));
        }
        if property == "length" {
            return Ok(Value::Number(self.state.storage.seed.entries.len() as f64));
        }
        Ok(METHODS
            .iter()
            .position(|name| *name == property)
            .map_or(Value::Undefined, |index| Value::Host(METHOD + index as u64)))
    }
    pub(super) fn storage_call(
        &mut self,
        object: u64,
        method: &str,
        args: &[Value],
        this: &Value,
        runtime: &mut Runtime,
    ) -> Result<Value, JsError> {
        let name = if (METHOD..METHOD + METHODS.len() as u64).contains(&object) {
            if method != "call" || *this != Value::Host(OBJECT) {
                return Err(JsError::runtime("Illegal session storage receiver"));
            }
            METHODS[(object - METHOD) as usize]
        } else {
            if *this != Value::Host(OBJECT) {
                return Err(JsError::runtime("Illegal session storage receiver"));
            }
            method
        };
        let required = match name {
            "clear" => 0,
            "setItem" => 2,
            "key" | "getItem" | "removeItem" => 1,
            _ => return Err(JsError::runtime("Unsupported session storage method")),
        };
        if args.len() < required {
            return Err(JsError::runtime("Missing session storage argument"));
        }
        self.storage_available()?;
        // Charge each linear search/copy before performing it. Coercion hooks have
        // their own VM budgets and may reenter storage, so obtain state afterwards.
        if name == "key" {
            let number = runtime.coerce_to_number(args[0].clone(), self)?;
            let index = if !number.is_finite() || number == 0. {
                0
            } else {
                number.trunc().rem_euclid(4_294_967_296.) as u32
            };
            let bytes = self
                .state
                .storage
                .seed
                .entries
                .get(index as usize)
                .map_or(0, |(k, _)| k.len());
            self.charge_tree_read(bytes + 1)?;
            return Ok(self
                .state
                .storage
                .seed
                .entries
                .get(index as usize)
                .map_or(Value::Null, |(k, _)| Value::string(k.clone())));
        }
        let key = if required > 0 {
            runtime.coerce_to_string(args[0].clone(), self)?
        } else {
            String::new()
        };
        let value = if name == "setItem" {
            Some(runtime.coerce_to_string(args[1].clone(), self)?)
        } else {
            None
        };
        let work = self
            .state
            .storage
            .bytes
            .checked_add(key.len())
            .and_then(|n| n.checked_add(value.as_ref().map_or(0, String::len)))
            .and_then(|n| n.checked_add(1))
            .ok_or_else(|| JsError::resource("Storage work overflow"))?;
        self.charge_tree_read(work)?;
        let storage = &mut self.state.storage;
        let index = storage.seed.entries.iter().position(|(k, _)| *k == key);
        match name {
            "getItem" => Ok(index.map_or(Value::Null, |i| {
                Value::string(storage.seed.entries[i].1.clone())
            })),
            "setItem" => {
                let value = value.expect("setItem value was converted");
                let old_bytes = index.map_or(0, |i| {
                    storage.seed.entries[i].0.len() + storage.seed.entries[i].1.len()
                });
                let total = storage
                    .bytes
                    .checked_sub(old_bytes)
                    .and_then(|n| n.checked_add(key.len()))
                    .and_then(|n| n.checked_add(value.len()));
                if storage.seed.access != SessionStorageAccess::ReadWrite
                    || key.len() > SESSION_STORAGE_MAX_KEY_BYTES
                    || value.len() > SESSION_STORAGE_MAX_VALUE_BYTES
                    || total.is_none_or(|n| n > SESSION_STORAGE_MAX_TOTAL_BYTES)
                    || index.is_none() && storage.seed.entries.len() >= SESSION_STORAGE_MAX_ENTRIES
                {
                    return Err(JsError::runtime("Session storage quota exceeded"));
                }
                if let Some(i) = index {
                    storage.seed.entries[i].1 = value;
                } else {
                    storage.seed.entries.push((key, value));
                }
                storage.bytes = total.expect("quota preflight passed");
                Ok(Value::Undefined)
            }
            "removeItem" => {
                if let Some(i) = index {
                    let (k, v) = storage.seed.entries.remove(i);
                    storage.bytes -= k.len() + v.len();
                }
                Ok(Value::Undefined)
            }
            "clear" => {
                storage.seed.entries.clear();
                storage.bytes = 0;
                Ok(Value::Undefined)
            }
            _ => unreachable!(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn session() -> DocumentSession {
        let mut s = DocumentSession::new(web_html::parse("").unwrap());
        s.set_document_url("https://example.test/path").unwrap();
        s.set_session_storage(SessionStorageSeed {
            access: SessionStorageAccess::ReadWrite,
            entries: vec![],
        })
        .unwrap();
        s
    }
    #[test]
    fn crud_identity_insertion_order_and_unsigned_index() {
        let mut s = session();
        assert_eq!(
            s.execute("sessionStorage===window.sessionStorage && sessionStorage===sessionStorage;")
                .unwrap(),
            Value::Bool(true)
        );
        assert_eq!(s.execute("const s=sessionStorage;s.setItem('','');s.setItem('a','1');s.setItem('b','2');s.setItem('a','3');s.removeItem('a');s.setItem('a','4');s.length===3 && s.key(0)==='' && s.key(1)==='b' && s.key(2)==='a' && s.key(-1)===null && s.key(4294967297)==='b' && s.getItem('missing')===null && s.getItem('')==='';").unwrap(), Value::Bool(true));
        assert_eq!(
            s.execute("sessionStorage.clear();sessionStorage.length===0;")
                .unwrap(),
            Value::Bool(true)
        );
    }
    #[test]
    fn observable_coercions_arity_receivers_and_abrupt_completion() {
        for source in [
            "let log='';const k={toString:function(){log+='k';return 'x'}};const v={toString:function(){log+='v';return 'y'}};sessionStorage.setItem(k,v);log==='kv' && sessionStorage.getItem(k)==='y';",
            "let n=0;const k={};Object.defineProperty(k,'toString',{get:function(){n++;return function(){return 'x'}}});sessionStorage.getItem(k);n===1;",
            "let n=0;const k={toString:function(){n++;throw 17}};const v={toString:function(){n+=10;return 'v'}};let ok=false;try{sessionStorage.setItem(k,v)}catch(e){ok=e===17}ok && n===1;",
            "let n=0;const k={toString:function(){n++;return 'x'}};let ok=false;try{sessionStorage.setItem(k)}catch(e){ok=true}ok && n===0;",
            "let count=0;const get=sessionStorage.getItem;try{get('x')}catch(e){count++}try{get.call({},'x')}catch(e){count++}count===2 && get.call(sessionStorage,'x')===null;",
            "sessionStorage.setItem('a','b');let log='';const i={valueOf:function(){log+='v';return 4294967296}};sessionStorage.key(i)==='a' && log==='v';",
            "let ok=0;try{sessionStorage.key(1n)}catch(e){ok++}try{sessionStorage.key(Symbol())}catch(e){ok++}ok===2;",
            "let count=0;try{sessionStorage.key()}catch(e){count++}try{sessionStorage.getItem()}catch(e){count++}try{sessionStorage.removeItem()}catch(e){count++}count===3;",
            "let ok=false;try{window.sessionStorage={}}catch(e){ok=true}ok && sessionStorage===window.sessionStorage;",
            r"let ok=false;try{sessionStorage.setItem(String.fromCharCode(55296),'v')}catch(e){ok=true}ok && sessionStorage.length===0;",
        ] {
            assert_eq!(
                session().execute(source).unwrap(),
                Value::Bool(true),
                "{source}"
            );
        }
    }
    #[test]
    fn quotas_are_ordinary_atomic_and_prior_writes_survive_ordinary_throw() {
        let mut s = session();
        assert_eq!(s.execute("sessionStorage.setItem('x','old');let ok=false;try{sessionStorage.setItem('x','v'.repeat(16385))}catch(e){ok=true}ok && sessionStorage.getItem('x')==='old';").unwrap(),Value::Bool(true));
        assert!(
            s.execute("sessionStorage.setItem('x','new');throw 17;")
                .is_err()
        );
        assert_eq!(
            s.session_storage_snapshot(),
            vec![("x".into(), "new".into())]
        );
        let entries = (0..128).map(|i| (i.to_string(), String::new())).collect();
        let mut s = DocumentSession::new(web_html::parse("").unwrap());
        s.set_document_url("https://example.test").unwrap();
        s.set_session_storage(SessionStorageSeed {
            access: SessionStorageAccess::ReadWrite,
            entries,
        })
        .unwrap();
        assert_eq!(s.execute("let ok=false;try{sessionStorage.setItem('new','v')}catch(e){ok=true}sessionStorage.setItem('0','changed');ok && sessionStorage.length===128 && sessionStorage.getItem('0')==='changed';").unwrap(),Value::Bool(true));
    }
    #[test]
    fn seed_and_origin_validation_and_capacity_refusal() {
        for url in ["file:///x", "data:text/html,x", "noir://home"] {
            let mut s = DocumentSession::new(web_html::parse("").unwrap());
            s.set_document_url(url).unwrap();
            assert!(
                s.set_session_storage(SessionStorageSeed {
                    access: SessionStorageAccess::ReadWrite,
                    entries: vec![]
                })
                .is_err()
            );
            assert_eq!(
                s.execute("let ok=false;try{sessionStorage}catch(e){ok=true}ok;")
                    .unwrap(),
                Value::Bool(true)
            );
        }
        let mut s = DocumentSession::new(web_html::parse("").unwrap());
        s.set_document_url("https://example.test").unwrap();
        assert!(
            s.set_session_storage(SessionStorageSeed {
                access: SessionStorageAccess::ReadWrite,
                entries: vec![("x".into(), "1".into()), ("x".into(), "2".into())]
            })
            .is_err()
        );
        s.set_session_storage(SessionStorageSeed {
            access: SessionStorageAccess::QuotaExceeded,
            entries: vec![],
        })
        .unwrap();
        assert_eq!(s.execute("let ok=false;try{sessionStorage.setItem('x','1')}catch(e){ok=true}sessionStorage.clear();sessionStorage.removeItem('x');ok && sessionStorage.length===0 && sessionStorage.getItem('x')===null;").unwrap(),Value::Bool(true));
        assert!(
            s.set_session_storage(SessionStorageSeed::default())
                .is_err()
        );
    }
    #[test]
    fn cumulative_work_resource_failure_rolls_back_storage() {
        let mut s = session();
        s.execute("sessionStorage.setItem('x','before');").unwrap();
        s.state.read_work = 1_999_990;
        assert_eq!(s.execute("sessionStorage.setItem('x','after');sessionStorage.getItem('x');sessionStorage.getItem('x');").unwrap_err().kind,ErrorKind::Resource);
        assert_eq!(
            s.session_storage_snapshot(),
            vec![("x".into(), "before".into())]
        );
    }
    #[test]
    fn dispatch_and_timer_resource_failures_restore_prior_storage() {
        let mut s = session();
        s.execute("sessionStorage.setItem('x','before');document.addEventListener('click',function(){sessionStorage.setItem('x','after');while(true){}});").unwrap();
        assert_eq!(
            s.dispatch_event(0, "click").unwrap_err().kind,
            ErrorKind::Resource
        );
        assert_eq!(
            s.session_storage_snapshot(),
            vec![("x".into(), "before".into())]
        );
        let mut s = session();
        s.execute("sessionStorage.setItem('x','before');setTimeout(function(){sessionStorage.setItem('x','after');while(true){}},0);").unwrap();
        assert_eq!(s.run_due_timers(0).unwrap_err().kind, ErrorKind::Resource);
        assert_eq!(
            s.session_storage_snapshot(),
            vec![("x".into(), "before".into())]
        );
    }
    #[test]
    fn total_byte_quota_and_invalid_seed_are_atomic() {
        let mut s = DocumentSession::new(web_html::parse("").unwrap());
        s.set_document_url("https://example.test").unwrap();
        assert!(
            s.set_session_storage(SessionStorageSeed {
                access: SessionStorageAccess::ReadWrite,
                entries: vec![("k".repeat(1025), String::new())]
            })
            .is_err()
        );
        let entries = (0..4).map(|i| (i.to_string(), "v".repeat(16383))).collect();
        s.set_session_storage(SessionStorageSeed {
            access: SessionStorageAccess::ReadWrite,
            entries,
        })
        .unwrap();
        assert_eq!(s.execute("let ok=false;try{sessionStorage.setItem('0','v'.repeat(16384))}catch(e){ok=true}ok && sessionStorage.getItem('0').length===16383;").unwrap(),Value::Bool(true));
    }
    #[test]
    fn unavailable_accessors_throw_even_with_global_var_bindings() {
        for source in [
            "let n=0;try{sessionStorage}catch(e){n++}try{window.sessionStorage}catch(e){n++}n===2;",
            "var sessionStorage;let n=0;try{sessionStorage}catch(e){n++}try{window.sessionStorage}catch(e){n++}n===2;",
            "let sessionStorage=17;let caught=false;try{window.sessionStorage}catch(e){caught=true}sessionStorage===17 && caught;",
            "function f(){let sessionStorage=17;return sessionStorage}f()===17;",
        ] {
            let mut s = DocumentSession::new(web_html::parse("").unwrap());
            assert_eq!(s.execute(source).unwrap(), Value::Bool(true), "{source}");
        }
    }
    #[test]
    fn global_accessor_overrides_property_backing_but_not_lexical_scope() {
        let mut s = session();
        assert_eq!(
            s.execute("var sessionStorage;sessionStorage===window.sessionStorage;")
                .unwrap(),
            Value::Bool(true)
        );
        // A pre-existing global property backing value cannot forge host authority.
        assert_eq!(s.execute("var sessionStorage=17;sessionStorage===window.sessionStorage && sessionStorage.length===0;").unwrap(),Value::Bool(true));
        let mut s = session();
        assert_eq!(
            s.execute(
                "let sessionStorage=17;sessionStorage===17 && window.sessionStorage.length===0;"
            )
            .unwrap(),
            Value::Bool(true)
        );
    }
}
