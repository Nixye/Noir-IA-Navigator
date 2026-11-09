//! Bounded local SHA-2 digest. No randomness, keys, I/O, or background execution.
//! Argument failures reject an intrinsic Promise; receiver/construction failures
//! throw TypeError synchronously. Resource exhaustion always remains fatal.
use crate::{ErrorKind, Host, JsError, Runtime, Value, descriptors::PropertyMetadata};
use sha2::{Digest, Sha256, Sha384, Sha512};
use std::collections::BTreeMap;

const MAX_INPUT: usize = 256 * 1024;
const MAX_INPUT_TOTAL: usize = 2 * 1024 * 1024;
const MAX_ATTEMPTS: usize = 512;

#[derive(Clone, Debug, Default)]
pub(super) struct CryptoState {
    namespace: Option<usize>,
    subtle: Option<usize>,
    input_bytes: usize,
    attempts: usize,
}

impl Runtime {
    pub(super) fn crypto_preserve_budgets(&mut self, consumed: &Self) {
        self.crypto.input_bytes = self.crypto.input_bytes.max(consumed.crypto.input_bytes);
        self.crypto.attempts = self.crypto.attempts.max(consumed.crypto.attempts);
    }

    pub(super) fn crypto_roots(&self) -> Vec<Value> {
        self.crypto
            .namespace
            .into_iter()
            .chain(self.crypto.subtle)
            .map(Value::Object)
            .collect()
    }

    fn crypto_readonly(&mut self, id: usize, name: &str) {
        self.property_metadata.entry(id).or_default().insert(
            name.into(),
            PropertyMetadata {
                writable: false,
                enumerable: false,
                configurable: false,
                accessor: None,
            },
        );
        self.reflection.non_extensible.insert(id);
    }

    pub(super) fn crypto_namespace(&mut self, host: &impl Host) -> Result<Value, JsError> {
        // Never accept a page property, var binding, or host-returned marker as authority.
        if !host.allows_subtle_crypto() {
            return Ok(Value::Undefined);
        }
        if let Some(id) = self.crypto.namespace {
            return Ok(Value::Object(id));
        }
        let subtle = self.ordinary_object(BTreeMap::from([(
            "digest".into(),
            Value::Native("SubtleCrypto.digest"),
        )]))?;
        let Value::Object(subtle_id) = subtle else {
            unreachable!()
        };
        self.crypto_readonly(subtle_id, "digest");
        self.hide_properties(&subtle, &["digest"]);
        let namespace = self.ordinary_object(BTreeMap::from([("subtle".into(), subtle)]))?;
        let Value::Object(id) = namespace else {
            unreachable!()
        };
        self.crypto_readonly(id, "subtle");
        self.hide_properties(&namespace, &["subtle"]);
        self.crypto.subtle = Some(subtle_id);
        self.crypto.namespace = Some(id);
        Ok(namespace)
    }

    pub(super) fn crypto_digest(
        &mut self,
        args: &[Value],
        this: &Value,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        if !host.allows_subtle_crypto()
            || !matches!(this, Value::Object(id) if Some(*id) == self.crypto.subtle)
        {
            return Err(self.property_type_error("Invalid SubtleCrypto.digest receiver or context"));
        }
        if self.crypto.attempts >= MAX_ATTEMPTS {
            return Err(JsError::resource(
                "Crypto cumulative digest attempt budget exceeded",
            ));
        }
        self.crypto.attempts += 1;
        // All fallible conversion/hash/allocation work precedes Promise allocation:
        // a fatal resource failure cannot leave a pending orphan Promise behind.
        let result = self.crypto_digest_result(args, host);
        let (rejected, value) = match result {
            Ok(value) => (false, value),
            Err(error) if error.kind == ErrorKind::Resource => return Err(error),
            Err(error) => (true, self.exception_value(&error)?),
        };
        let promise = self.new_async_promise()?;
        let Value::Object(id) = promise else {
            unreachable!()
        };
        // Direct fulfillment avoids observing a poisoned ArrayBuffer.prototype.then.
        // This fresh Promise has no reactions, so settlement cannot exhaust jobs.
        self.settle_promise(id, rejected, value)?;
        Ok(promise)
    }

    fn crypto_digest_result(
        &mut self,
        args: &[Value],
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        if args.len() < 2 {
            return Err(self.property_type_error("digest requires algorithm and data"));
        }
        let algorithm = args[0].clone();
        // The AlgorithmIdentifier union first chooses dictionary vs DOMString.
        // Primitive DOMString conversion precedes input validation; dictionary.name
        // is deliberately not touched until the exact native byte window is copied.
        let dictionary = matches!(
            algorithm,
            Value::Object(_)
                | Value::Array(_)
                | Value::Function(_)
                | Value::Native(_)
                | Value::Host(_)
        );
        let primitive_name = if dictionary {
            None
        } else {
            Some(self.coerce_to_string(algorithm.clone(), host)?)
        };
        let bytes = self.snapshot_buffer_source(
            &args[1],
            MAX_INPUT_TOTAL.saturating_sub(self.crypto.input_bytes),
            MAX_INPUT,
            "Crypto",
            |runtime, length| {
                runtime.crypto.input_bytes += length;
                // Reserve fixed and proportional backend work before any hashing.
                runtime.remaining = runtime
                    .remaining
                    .checked_sub(length as u64 + 128)
                    .ok_or_else(|| JsError::resource("Crypto digest work budget exceeded"))?;
                Ok(())
            },
        )?;
        let name = if let Some(name) = primitive_name {
            name
        } else {
            let name = self.get(algorithm, "name", host)?;
            if matches!(name, Value::Undefined) {
                return Err(self.property_type_error("Algorithm.name is required"));
            }
            self.coerce_to_string(name, host)?
        };
        let output = if name.eq_ignore_ascii_case("SHA-256") {
            Sha256::digest(&bytes).to_vec()
        } else if name.eq_ignore_ascii_case("SHA-384") {
            Sha384::digest(&bytes).to_vec()
        } else if name.eq_ignore_ascii_case("SHA-512") {
            Sha512::digest(&bytes).to_vec()
        } else {
            let name = self.string("NotSupportedError".into())?;
            let message = self.string("Only SHA-256, SHA-384 and SHA-512 are supported".into())?;
            let thrown = self.ordinary_object(BTreeMap::from([
                ("name".into(), name),
                ("message".into(), message),
            ]))?;
            self.hide_properties(&thrown, &["name", "message"]);
            let mut error = JsError::runtime("Unsupported digest algorithm");
            error.thrown = Some(thrown);
            return Err(error);
        };
        self.buffer_from_bytes(&output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Limits, PromiseInspection};

    struct Trusted(bool);
    impl Host for Trusted {
        fn allows_subtle_crypto(&self) -> bool {
            self.0
        }
        fn global(&mut self, _: &str) -> Option<Value> {
            None
        }
        fn get(&mut self, _: u64, _: &str) -> Result<Value, JsError> {
            panic!("digest must not perform host I/O")
        }
        fn set(&mut self, _: u64, _: &str, _: Value) -> Result<(), JsError> {
            panic!("digest must not perform host I/O")
        }
        fn call(&mut self, _: u64, _: &str, _: &[Value]) -> Result<Value, JsError> {
            panic!("digest must not perform host I/O")
        }
    }

    fn check(source: &str) {
        let mut vm = Runtime::default();
        vm.execute(source, &mut Trusted(true)).unwrap();
        assert_eq!(
            vm.execute("ok;", &mut Trusted(true)).unwrap(),
            Value::Bool(true),
            "{source}"
        );
    }

    #[test]
    fn published_empty_and_abc_vectors() {
        for (algorithm, input, expected) in [
            (
                "SHA-256",
                "[]",
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            ),
            (
                "SHA-256",
                "[97,98,99]",
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            ),
            (
                "SHA-384",
                "[]",
                "38b060a751ac96384cd9327eb1b1e36a21fdb71114be07434c0cc7bf63f6e1da274edebfe76f65fbd51ad2f14898b95b",
            ),
            (
                "SHA-384",
                "[97,98,99]",
                "cb00753f45a35e8bb5a03d699ac65007272c32ab0eded1631a8b605a43ff5bed8086072ba1e7cc2358baeca134c825a7",
            ),
            (
                "SHA-512",
                "[]",
                "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e",
            ),
            (
                "SHA-512",
                "[97,98,99]",
                "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f",
            ),
        ] {
            let mut vm = Runtime::default();
            let promise = vm
                .execute(
                    &format!("crypto.subtle.digest('{algorithm}',new Uint8Array({input}));"),
                    &mut Trusted(true),
                )
                .unwrap();
            let Some(PromiseInspection::Fulfilled(value)) = vm.promise_status(&promise) else {
                panic!("digest not fulfilled")
            };
            let bytes = vm.wasm_buffer_source(&value, 64).unwrap();
            let actual: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn every_native_view_uses_its_exact_byte_window() {
        for kind in [
            "Int8Array",
            "Uint8Array",
            "Uint8ClampedArray",
            "Int16Array",
            "Uint16Array",
            "Int32Array",
            "Uint32Array",
            "Float32Array",
            "Float64Array",
            "BigInt64Array",
            "BigUint64Array",
            "DataView",
        ] {
            check(&format!(
                "var ok=false;const b=new ArrayBuffer(32);const all=new Uint8Array(b);for(let i=0;i<32;i++){{all[i]=i+1;}}const v=new {kind}(b,8,2);const expected=new Uint8Array(b,8,v.byteLength);let a;crypto.subtle.digest('SHA-256',v).then(x=>a=new Uint8Array(x).join(','));crypto.subtle.digest('SHA-256',expected).then(x=>ok=a===new Uint8Array(x).join(','));"
            ));
        }
        check(
            "var ok=false;const b=new Uint8Array([97,98,99]);let reads=0;const a={};Object.defineProperty(a,'name',{get:function(){reads++;b[0]=0;return {toString(){return 'sHa-256';}};},enumerable:true,configurable:true});crypto.subtle.digest(a,b).then(x=>ok=reads===1&&new Uint8Array(x)[0]===186&&b[0]===0);",
        );
    }

    #[test]
    fn conversions_reject_without_spoofed_buffer_getters() {
        // Accessors use descriptors: object-literal accessor syntax is not supported.
        check(
            "var ok=false;let reads=0;const b={};Object.defineProperty(b,'buffer',{get:function(){reads++;throw 9;},enumerable:true,configurable:true});Object.defineProperty(b,'byteLength',{get:function(){reads++;throw 8;},enumerable:true,configurable:true});crypto.subtle.digest('SHA-256',b).catch(e=>ok=e.name==='TypeError'&&reads===0);",
        );
        check(
            "var ok=false;const marker={};const a={};Object.defineProperty(a,'name',{get:function(){throw marker;},enumerable:true,configurable:true});crypto.subtle.digest(a,new Uint8Array(0)).catch(e=>ok=e===marker);",
        );
        check(
            "var ok=false;crypto.subtle.digest({name:Symbol('x')},new Uint8Array(0)).catch(e=>ok=e.name==='TypeError');",
        );
        check(
            "var ok=false;const b={};Object.defineProperty(b,'buffer',{get:function(){throw 9;},enumerable:true,configurable:true});crypto.subtle.digest(Symbol('x'),b).catch(e=>ok=e.name==='TypeError');",
        );
        check("var ok=false;crypto.subtle.digest().catch(e=>ok=e.name==='TypeError');");
        for algorithm in ["SHA1", "SHA-1", "SHA256", " SHA-256", "MD5"] {
            check(&format!(
                "var ok=false;crypto.subtle.digest('{algorithm}',new Uint8Array(0)).catch(e=>ok=e.name==='NotSupportedError');"
            ));
        }
    }

    #[test]
    fn intrinsic_promises_results_and_fifo_survive_poisoning() {
        check(
            "var ok=false;let reads=0;Object.defineProperty(ArrayBuffer.prototype,'then',{get(){reads++;throw 7;}});crypto.subtle.digest('SHA-256',new Uint8Array(0)).then(x=>ok=x.byteLength===32&&reads===0);",
        );
        check(
            "var ok=false;var Promise;const P=Promise;Promise=function(){throw 3;};let order='';const p=crypto.subtle.digest('SHA-256',new Uint8Array(0));order+='s';p.then(x=>{order+='a';ok=x instanceof ArrayBuffer&&x.byteLength===32&&p instanceof P;});queueMicrotask(()=>order+='b');p.then(()=>ok=ok&&order==='sab');",
        );
        check(
            "var ok=false;let order='';async function f(){const x=await crypto.subtle.digest('SHA-384',new Uint8Array(0));order+='a';ok=x.byteLength===48&&order==='sa';}f();order+='s';",
        );
        check(
            "var ok=false;let a;crypto.subtle.digest('SHA-512',new Uint8Array(0)).then(x=>{a=x;new Uint8Array(x)[0]=0;});crypto.subtle.digest('SHA-512',new Uint8Array(0)).then(x=>ok=x!==a&&new Uint8Array(x)[0]===207);",
        );
    }

    #[test]
    fn brands_readonly_names_and_permission_recheck() {
        check(
            "var ok=false;const s=crypto.subtle;let count=0;try{s.digest.call({},'SHA-256',new Uint8Array(0));}catch(e){if(e.name==='TypeError')count++;}try{new s.digest();}catch(e){if(e.name==='TypeError')count++;}ok=count===2&&s.digest.length===2&&s.digest.name==='digest'&&crypto===crypto&&s===crypto.subtle&&typeof Crypto==='undefined'&&typeof SubtleCrypto==='undefined'&&Object.isFrozen(s)&&Object.isFrozen(crypto);",
        );
        let mut vm = Runtime::default();
        let subtle = vm.execute("crypto.subtle;", &mut Trusted(true)).unwrap();
        let error = vm
            .invoke_with_this(
                Value::Native("SubtleCrypto.digest"),
                vec![],
                subtle,
                &mut Trusted(false),
            )
            .unwrap_err();
        assert_ne!(error.kind, ErrorKind::Resource);
        assert_eq!(
            vm.execute("typeof crypto;", &mut Trusted(false)).unwrap(),
            Value::string("undefined")
        );
    }

    #[test]
    fn quotas_are_fatal_and_survive_gc_and_rollback() {
        let mut vm = Runtime::default();
        vm.execute("var b=new Uint8Array([1,2,3]);", &mut Trusted(true))
            .unwrap();
        let before = vm.clone();
        vm.execute("crypto.subtle.digest('SHA-1',b);", &mut Trusted(true))
            .unwrap();
        assert_eq!(vm.crypto.input_bytes, 3);
        vm.rollback_to(before);
        vm.collect_garbage(&[], &Trusted(true)).unwrap();
        assert_eq!((vm.crypto.input_bytes, vm.crypto.attempts), (3, 1));
        vm.crypto.attempts = MAX_ATTEMPTS;
        assert_eq!(
            vm.execute(
                "try{crypto.subtle.digest('SHA-256',b);}catch(e){false;}",
                &mut Trusted(true)
            )
            .unwrap_err()
            .kind,
            ErrorKind::Resource
        );
        let mut vm = Runtime::default();
        vm.crypto.input_bytes = MAX_INPUT_TOTAL;
        assert_eq!(
            vm.execute(
                "crypto.subtle.digest('SHA-256',new Uint8Array(1));",
                &mut Trusted(true)
            )
            .unwrap_err()
            .kind,
            ErrorKind::Resource
        );
        let mut vm = Runtime::default();
        assert_eq!(
            vm.execute(
                "crypto.subtle.digest('SHA-256',new ArrayBuffer(262145));",
                &mut Trusted(true)
            )
            .unwrap_err()
            .kind,
            ErrorKind::Resource
        );
    }

    #[test]
    fn fuel_failure_never_creates_a_pending_digest_promise() {
        let mut vm = Runtime::new(Limits {
            instructions: 2000,
            ..Limits::default()
        });
        vm.execute("var b=new ArrayBuffer(65536);", &mut Trusted(true))
            .unwrap();
        let namespace = vm.execute("crypto;", &mut Trusted(true)).unwrap();
        let before = (0..vm.heap.len())
            .filter(|id| {
                matches!(
                    vm.promise_status(&Value::Object(*id)),
                    Some(PromiseInspection::Pending)
                )
            })
            .count();
        assert_eq!(
            vm.execute(
                "try{crypto.subtle.digest('SHA-512',b);}catch(e){false;}",
                &mut Trusted(true)
            )
            .unwrap_err()
            .kind,
            ErrorKind::Resource
        );
        assert_eq!(
            (0..vm.heap.len())
                .filter(|id| matches!(
                    vm.promise_status(&Value::Object(*id)),
                    Some(PromiseInspection::Pending)
                ))
                .count(),
            before
        );
        vm.collect_garbage(&[namespace], &Trusted(true)).unwrap();
        assert!(vm.crypto.namespace.is_some());
    }
    #[test]
    fn output_buffer_heap_and_promise_limits_remain_fatal() {
        let mut vm = Runtime::default();
        vm.execute("var keep=[new ArrayBuffer(1048576),new ArrayBuffer(1048576),new ArrayBuffer(1048576),new ArrayBuffer(1048576)];var input=new ArrayBuffer(0);", &mut Trusted(true)).unwrap();
        assert_eq!(
            vm.execute(
                "try{crypto.subtle.digest('SHA-256',input);}catch(e){false;}",
                &mut Trusted(true)
            )
            .unwrap_err()
            .kind,
            ErrorKind::Resource
        );

        let mut vm = Runtime::default();
        vm.execute("var ps=[];for(let i=0;i<512;i++){ps.push(Promise.resolve(0));}var input=new ArrayBuffer(0);", &mut Trusted(true)).unwrap();
        assert_eq!(
            vm.execute("crypto.subtle.digest('SHA-256',input);", &mut Trusted(true))
                .unwrap_err()
                .kind,
            ErrorKind::Resource
        );
        assert!((0..vm.heap.len()).all(|id| !matches!(
            vm.promise_status(&Value::Object(id)),
            Some(PromiseInspection::Pending)
        )));

        let mut vm = Runtime::default();
        vm.execute(
            "var input=new ArrayBuffer(0);var subtle=crypto.subtle;",
            &mut Trusted(true),
        )
        .unwrap();
        vm.collect_garbage(&[], &Trusted(true)).unwrap();
        vm.limits.heap_objects = vm.heap.len();
        vm.free_heap.clear();
        let input = vm.global_value("input").unwrap();
        let subtle = vm.global_value("subtle").unwrap();
        assert_eq!(
            vm.crypto_digest(
                &[Value::string("SHA-256"), input],
                &subtle,
                &mut Trusted(true)
            )
            .unwrap_err()
            .kind,
            ErrorKind::Resource
        );
    }
}
