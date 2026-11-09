//! Bounded VM Math namespace. random is absent: no authorized entropy source exists.
//! Transcendentals use Rust f64 operations, without universal rounding guarantees.
use crate::{Host, JsError, Runtime, Value, descriptors::PropertyMetadata, to_uint32};
use std::collections::BTreeMap;

const METHODS: &[(&str, &str)] = &[
    ("abs", "Math.abs"),
    ("acos", "Math.acos"),
    ("acosh", "Math.acosh"),
    ("asin", "Math.asin"),
    ("asinh", "Math.asinh"),
    ("atan", "Math.atan"),
    ("atanh", "Math.atanh"),
    ("atan2", "Math.atan2"),
    ("cbrt", "Math.cbrt"),
    ("ceil", "Math.ceil"),
    ("clz32", "Math.clz32"),
    ("cos", "Math.cos"),
    ("cosh", "Math.cosh"),
    ("exp", "Math.exp"),
    ("expm1", "Math.expm1"),
    ("floor", "Math.floor"),
    ("fround", "Math.fround"),
    ("hypot", "Math.hypot"),
    ("imul", "Math.imul"),
    ("log", "Math.log"),
    ("log1p", "Math.log1p"),
    ("log2", "Math.log2"),
    ("log10", "Math.log10"),
    ("max", "Math.max"),
    ("min", "Math.min"),
    ("pow", "Math.pow"),
    ("round", "Math.round"),
    ("sign", "Math.sign"),
    ("sin", "Math.sin"),
    ("sinh", "Math.sinh"),
    ("sqrt", "Math.sqrt"),
    ("tan", "Math.tan"),
    ("tanh", "Math.tanh"),
    ("trunc", "Math.trunc"),
];

pub(super) fn native_length(name: &str) -> Option<usize> {
    let method = name.strip_prefix("Math.")?;
    if !METHODS.iter().any(|(key, _)| *key == method) {
        return None;
    }
    Some(
        if matches!(method, "atan2" | "hypot" | "imul" | "max" | "min" | "pow") {
            2
        } else {
            1
        },
    )
}

impl Runtime {
    pub(super) fn math_namespace(&mut self) -> Result<Value, JsError> {
        if let Some(value) = self.reflection.prototypes.get("Math") {
            return Ok(value.clone());
        }
        use std::f64::consts;
        let constants = [
            ("E", consts::E),
            ("LN10", consts::LN_10),
            ("LN2", consts::LN_2),
            ("LOG10E", consts::LOG10_E),
            ("LOG2E", consts::LOG2_E),
            ("PI", consts::PI),
            ("SQRT1_2", consts::FRAC_1_SQRT_2),
            ("SQRT2", consts::SQRT_2),
        ];
        let mut entries = BTreeMap::new();
        for (key, value) in constants {
            entries.insert(key.into(), Value::Number(value));
        }
        for &(key, name) in METHODS {
            entries.insert(key.into(), Value::Native(name));
        }
        let value = self.ordinary_object(entries)?;
        let id = self.heap_owner(&value)?;
        self.remember_property_order(
            id,
            constants
                .iter()
                .map(|(key, _)| (*key).into())
                .chain(METHODS.iter().map(|(key, _)| (*key).into()))
                .collect(),
        );
        for (key, _) in constants {
            self.property_metadata.entry(id).or_default().insert(
                key.into(),
                PropertyMetadata {
                    writable: false,
                    enumerable: false,
                    configurable: false,
                    accessor: None,
                },
            );
        }
        for &(key, _) in METHODS {
            self.property_metadata.entry(id).or_default().insert(
                key.into(),
                PropertyMetadata {
                    writable: true,
                    enumerable: false,
                    configurable: true,
                    accessor: None,
                },
            );
        }
        if let Value::Symbol(key, _) = self.well_known_symbol("toStringTag")? {
            let label = self.string("Math".into())?;
            self.symbol_properties
                .entry(id)
                .or_default()
                .insert(key, label);
            self.symbol_order.entry(id).or_default().push(key);
            self.symbol_metadata.entry(id).or_default().insert(
                key,
                PropertyMetadata {
                    writable: false,
                    enumerable: false,
                    configurable: true,
                    accessor: None,
                },
            );
        }
        self.reflection.prototypes.insert("Math", value.clone());
        Ok(value)
    }

    fn math_number(&mut self, value: Value, host: &mut impl Host) -> Result<f64, JsError> {
        self.tick()?;
        match self.operator_number(value, host)? {
            Value::Number(number) => Ok(number),
            _ => Err(JsError::runtime("Math numeric conversion invariant failed")),
        }
    }

    pub(super) fn math_call(
        &mut self,
        name: &str,
        args: &[Value],
        construct: bool,
        host: &mut impl Host,
    ) -> Result<Option<Value>, JsError> {
        if native_length(name).is_none() {
            return Ok(None);
        }
        if construct {
            return Err(self.property_type_error("Math methods are not constructors"));
        }
        let method = &name[5..];
        if matches!(method, "min" | "max" | "hypot") {
            // All conversions happen in order, even after NaN or Infinity.
            // Accumulators use constant space and each conversion charges the VM budget.
            let mut result = if method == "min" {
                f64::INFINITY
            } else {
                f64::NEG_INFINITY
            };
            let mut scale = 0.0_f64;
            let mut sum = 0.0_f64;
            let mut nan = false;
            let mut infinite = false;
            for value in args {
                let x = self.math_number(value.clone(), host)?;
                nan |= x.is_nan();
                if method == "hypot" {
                    let x = x.abs();
                    infinite |= x.is_infinite();
                    if x.is_finite() && x != 0.0 {
                        if x > scale {
                            sum = 1.0 + sum * (scale / x) * (scale / x);
                            scale = x;
                        } else {
                            sum += (x / scale) * (x / scale);
                        }
                    }
                } else if (method == "min"
                    && (x < result || (x == 0.0 && result == 0.0 && x.is_sign_negative())))
                    || (method == "max"
                        && (x > result || (x == 0.0 && result == 0.0 && x.is_sign_positive())))
                {
                    result = x;
                }
            }
            result = if method == "hypot" && infinite {
                f64::INFINITY
            } else if nan {
                f64::NAN
            } else if method == "hypot" {
                scale * sum.sqrt()
            } else {
                result
            };
            return Ok(Some(Value::Number(result)));
        }
        let x = self.math_number(args.first().cloned().unwrap_or(Value::Undefined), host)?;
        let result = if matches!(method, "atan2" | "imul" | "pow") {
            let y = self.math_number(args.get(1).cloned().unwrap_or(Value::Undefined), host)?;
            match method {
                "atan2" => x.atan2(y),
                "imul" => to_uint32(x).wrapping_mul(to_uint32(y)) as i32 as f64,
                _ => number_pow(x, y),
            }
        } else {
            match method {
                "abs" => x.abs(),
                "acos" => x.acos(),
                "acosh" => x.acosh(),
                "asin" => x.asin(),
                "asinh" => x.asinh(),
                "atan" => x.atan(),
                "atanh" => x.atanh(),
                "cbrt" => x.cbrt(),
                "ceil" => x.ceil(),
                "clz32" => to_uint32(x).leading_zeros() as f64,
                "cos" => x.cos(),
                "cosh" => x.cosh(),
                "exp" => x.exp(),
                "expm1" => x.exp_m1(),
                "floor" => x.floor(),
                "fround" => (x as f32) as f64,
                "log" => x.ln(),
                "log1p" => x.ln_1p(),
                "log2" => x.log2(),
                "log10" => x.log10(),
                "round" => number_round(x),
                "sign" => {
                    if x == 0.0 || x.is_nan() {
                        x
                    } else {
                        x.signum()
                    }
                }
                "sin" => x.sin(),
                "sinh" => x.sinh(),
                "sqrt" => x.sqrt(),
                "tan" => x.tan(),
                "tanh" => x.tanh(),
                "trunc" => x.trunc(),
                _ => return Err(JsError::runtime("Unknown Math method")),
            }
        };
        Ok(Some(Value::Number(result)))
    }
}

fn number_round(x: f64) -> f64 {
    if !x.is_finite() || x == 0.0 {
        return x;
    }
    if (-0.5..0.0).contains(&x) {
        return -0.0;
    }
    let floor = x.floor();
    if x - floor < 0.5 { floor } else { floor + 1.0 }
}

fn number_pow(x: f64, y: f64) -> f64 {
    if y.is_nan() {
        return f64::NAN;
    }
    if y == 0.0 {
        return 1.0;
    }
    if x.is_nan() {
        return f64::NAN;
    }
    if y.is_infinite() {
        return match x.abs().partial_cmp(&1.0) {
            Some(std::cmp::Ordering::Equal) => f64::NAN,
            Some(std::cmp::Ordering::Greater) => {
                if y > 0.0 {
                    f64::INFINITY
                } else {
                    0.0
                }
            }
            _ => {
                if y > 0.0 {
                    0.0
                } else {
                    f64::INFINITY
                }
            }
        };
    }
    let odd = y.fract() == 0.0 && y.abs() % 2.0 == 1.0;
    if x == 0.0 || x.is_infinite() {
        let magnitude = if (x == 0.0) == (y > 0.0) {
            0.0
        } else {
            f64::INFINITY
        };
        return if x.is_sign_negative() && odd {
            -magnitude
        } else {
            magnitude
        };
    }
    if x < 0.0 && y.fract() != 0.0 {
        return f64::NAN;
    }
    x.powf(y)
}

#[cfg(test)]
mod tests {
    use crate::{ErrorKind, Host, JsError, Limits, Runtime, Value};

    struct Empty;
    impl Host for Empty {
        fn global(&mut self, _: &str) -> Option<Value> {
            None
        }
        fn get(&mut self, _: u64, _: &str) -> Result<Value, JsError> {
            Err(JsError::runtime("No host"))
        }
        fn set(&mut self, _: u64, _: &str, _: Value) -> Result<(), JsError> {
            Err(JsError::runtime("No host"))
        }
        fn call(&mut self, _: u64, _: &str, _: &[Value]) -> Result<Value, JsError> {
            Err(JsError::runtime("No host"))
        }
    }
    fn run(source: &str) -> Result<Value, JsError> {
        Runtime::default().execute(source, &mut Empty)
    }

    #[test]
    fn namespace_descriptors_detached_calls_and_arities() {
        for source in [
            "typeof Math==='object'&&Math===Math&&Object.getPrototypeOf(Math)===Object.prototype&&Object.keys(Math).length===0;",
            "Math[Symbol.toStringTag]==='Math'&&typeof Math.random==='undefined';",
            "const d=Object.getOwnPropertyDescriptor(Math,'PI');d.value===Math.PI&&!d.writable&&!d.configurable&&!d.enumerable;",
            "const d=Object.getOwnPropertyDescriptor(Math,'abs');d.writable&&d.configurable&&!d.enumerable;",
            "const d=Object.getOwnPropertyDescriptor(Math,Symbol.toStringTag);!d.writable&&d.configurable&&!d.enumerable;",
            "Math.E>2.718&&Math.PI>3.141&&Math.LN2<0.694&&Math.LN10>2.302&&Math.LOG2E>1.442&&Math.LOG10E<0.435&&Math.SQRT2>1.414&&Math.SQRT1_2>0.707;",
            "Math.abs.length===1&&Math.pow.length===2&&Math.max.length===2&&Math.hypot.length===2&&Math.imul.length===2&&Math.atan2.length===2;",
            "const abs=Math.abs;abs.call(null,-4)===4&&Reflect.apply(Math.max,null,[3,9,2])===9;",
            "try{Math();false;}catch(e){e.name==='TypeError';}",
            "try{new Math();false;}catch(e){e.name==='TypeError';}",
            "try{new Math.abs(2);false;}catch(e){e.name==='TypeError';}",
            "var Math;const old=Math;Math={x:7};Math.x===7&&old.abs(-2)===2;",
            "var Math;var Reflect;var JSON;typeof Math==='object'&&Math.abs(-2)===2&&typeof Reflect==='object'&&Reflect.has({x:1},'x')&&typeof JSON==='object'&&JSON.stringify([1])==='[1]';",
        ] {
            assert_eq!(run(source).unwrap(), Value::Bool(true), "{source}");
        }
    }

    #[test]
    fn signed_zeros_nan_infinity_and_integer_operations() {
        for source in [
            "Object.is(Math.round(-0.5),-0)&&Object.is(Math.round(-0.1),-0)&&Math.round(0.5)===1&&Math.round(-1.5)===-1;",
            "Math.round(0.49999999999999994)===0&&Math.round(4503599627370497)===4503599627370497;",
            "Object.is(Math.sign(-0),-0)&&Object.is(Math.trunc(-0.1),-0)&&Object.is(Math.ceil(-0.1),-0)&&Object.is(Math.abs(-0),0);",
            "Object.is(Math.min(0,-0),-0)&&Object.is(Math.min(-0,0),-0)&&Object.is(Math.max(-0,0),0)&&Object.is(Math.max(0,-0),0);",
            "Math.min()===Infinity&&Math.max()===-Infinity&&Math.hypot()===0&&Math.hypot(-0)===0;",
            "isNaN(Math.max(NaN,Infinity))&&isNaN(Math.min(NaN,-Infinity))&&Math.hypot(NaN,Infinity)===Infinity;",
            "Math.pow(NaN,0)===1&&isNaN(Math.pow(1,Infinity))&&isNaN(Math.pow(-1,-Infinity));",
            "Object.is(Math.pow(-0,3),-0)&&Math.pow(-0,-3)===-Infinity&&Math.pow(-0,-2)===Infinity&&Object.is(Math.pow(-Infinity,-3),-0);",
            "isNaN(Math.pow(-2,0.5))&&Math.pow(-2,3)===-8&&Math.pow(Infinity,-1)===0;",
            "Math.clz32(0)===32&&Math.clz32(-1)===0&&Math.clz32(4294967296)===32&&Math.imul(4294967295,5)===-5;",
            "Object.is(Math.fround(-0),-0)&&Math.fround(16777217)===16777216&&Math.fround(Infinity)===Infinity;",
            "Math.hypot(3,4)===5&&Math.hypot(1e308,1e308)<Infinity&&Math.hypot(1e-300,1e-300)>0;",
            "Math.sqrt(9)===3&&Math.cbrt(8)===2&&Math.exp(0)===1&&Math.expm1(0)===0&&Math.log(1)===0&&Math.log1p(0)===0&&Math.log2(8)===3&&Math.log10(100)===2;",
            "Math.sin(0)===0&&Math.cos(0)===1&&Math.tan(0)===0&&Math.asin(0)===0&&Math.acos(1)===0&&Math.atan(0)===0&&Object.is(Math.atan2(-0,1),-0);",
            "Math.sinh(0)===0&&Math.cosh(0)===1&&Math.tanh(0)===0&&Math.asinh(0)===0&&Math.acosh(1)===0&&Math.atanh(0)===0;",
        ] {
            assert_eq!(run(source).unwrap(), Value::Bool(true), "{source}");
        }
    }

    #[test]
    fn conversions_observe_order_hooks_exceptions_and_ignored_extras() {
        for source in [
            "let s='';const a={valueOf:function(){s+='a';return 2;}};const b={valueOf:function(){s+='b';return 3;}};Math.pow(a,b)===8&&s==='ab';",
            "let s='';const a={valueOf:function(){s+='a';return NaN;}};const b={valueOf:function(){s+='b';return 3;}};isNaN(Math.min(a,b))&&s==='ab';",
            "const bad={valueOf:function(){throw 17;}};try{Math.max(NaN,bad);false;}catch(e){e===17;}",
            "const bad={valueOf:function(){throw 17;}};try{Math.hypot(Infinity,bad);false;}catch(e){e===17;}",
            "const bad={valueOf:function(){throw 17;}};Math.abs(-3,bad,1n)===3&&Math.pow(2,3,bad)===8;",
            "const a={};a[Symbol.toPrimitive]=function(hint){return hint==='number'?7:0;};Math.abs(a)===7;",
            "Math.abs({valueOf:function(){return {};},toString:function(){return '-4';}})===4;",
            "try{Math.abs(1n);false;}catch(e){e.name==='TypeError';}",
            "try{Math.abs(Object(1n));false;}catch(e){e.name==='TypeError';}",
            "try{Math.abs(Symbol());false;}catch(e){e.name==='TypeError';}",
            "let called=false;try{Math.pow({valueOf:function(){throw 3;}},{valueOf:function(){called=true;return 0;}});}catch(e){}!called;",
        ] {
            assert_eq!(run(source).unwrap(), Value::Bool(true), "{source}");
        }
    }

    #[test]
    fn math_work_and_namespace_allocation_obey_limits() {
        let mut vm = Runtime::new(Limits {
            instructions: 1,
            ..Limits::default()
        });
        let error = vm
            .math_call(
                "Math.max",
                &[Value::Number(1.0), Value::Number(2.0), Value::Number(3.0)],
                false,
                &mut Empty,
            )
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
        let mut vm = Runtime::new(Limits {
            heap_objects: 0,
            ..Limits::default()
        });
        assert_eq!(vm.math_namespace().unwrap_err().kind, ErrorKind::Resource);
        let error =
            run("const a={valueOf:function(){return Math.abs(a);}};try{Math.abs(a);}catch(e){42;}")
                .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
    }

    #[test]
    fn vm_object_coercion_calls_host_hooks_but_host_exotics_stay_unsupported() {
        struct Hooks {
            calls: usize,
        }
        impl Host for Hooks {
            fn global(&mut self, name: &str) -> Option<Value> {
                match name {
                    "hook" => Some(Value::Host(7)),
                    "exotic" => Some(Value::Host(8)),
                    _ => None,
                }
            }
            fn get(&mut self, _: u64, _: &str) -> Result<Value, JsError> {
                Ok(Value::Undefined)
            }
            fn set(&mut self, _: u64, _: &str, _: Value) -> Result<(), JsError> {
                Ok(())
            }
            fn is_callable(&self, object: u64) -> bool {
                object == 7
            }
            fn call(&mut self, object: u64, method: &str, _: &[Value]) -> Result<Value, JsError> {
                assert_eq!(object, 7);
                assert_eq!(method, "call");
                self.calls += 1;
                Ok(Value::Number(-9.0))
            }
        }
        let mut host = Hooks { calls: 0 };
        let result = Runtime::default()
            .execute(
                "Math.abs({valueOf:hook})===9&&Math.abs(2,{valueOf:hook})===2;",
                &mut host,
            )
            .unwrap();
        assert_eq!(result, Value::Bool(true));
        assert_eq!(host.calls, 1);
        let result = Runtime::default()
            .execute(
                "try{Math.abs(exotic);false;}catch(e){e.name==='TypeError';}",
                &mut host,
            )
            .unwrap();
        assert_eq!(result, Value::Bool(true));
    }
}
