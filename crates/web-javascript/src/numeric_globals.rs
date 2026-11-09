//! Bounded, Host-aware numeric globals; prefix grammars operate on UTF-16 units.
use crate::{Host, JsError, Runtime, Value, coercion::Hint, to_uint32};

pub(super) fn native_length(name: &str) -> Option<usize> {
    match name {
        "parseInt" => Some(2),
        "parseFloat"
        | "isNaN"
        | "isFinite"
        | "Number.isNaN"
        | "Number.isFinite"
        | "Number.isInteger"
        | "Number.isSafeInteger" => Some(1),
        _ => None,
    }
}

pub(super) fn number_property(property: &str) -> Option<Value> {
    Some(Value::Native(match property {
        "parseInt" => "parseInt",
        "parseFloat" => "parseFloat",
        "isNaN" => "Number.isNaN",
        "isFinite" => "Number.isFinite",
        "isInteger" => "Number.isInteger",
        "isSafeInteger" => "Number.isSafeInteger",
        _ => return None,
    }))
}

fn whitespace(unit: u16) -> bool {
    matches!(unit, 0x9..=0xd | 0x20 | 0xa0 | 0x1680 | 0x2000..=0x200a
        | 0x2028 | 0x2029 | 0x202f | 0x205f | 0x3000 | 0xfeff)
}
fn digit(unit: u16) -> Option<u32> {
    match unit {
        0x30..=0x39 => Some(u32::from(unit - 0x30)),
        0x41..=0x5a => Some(u32::from(unit - 0x41) + 10),
        0x61..=0x7a => Some(u32::from(unit - 0x61) + 10),
        _ => None,
    }
}
fn decimal(unit: &u16) -> bool {
    (0x30..=0x39).contains(unit)
}
fn trimmed(units: &[u16]) -> &[u16] {
    let start = units
        .iter()
        .position(|unit| !whitespace(*unit))
        .unwrap_or(units.len());
    &units[start..]
}
fn decimal_value(units: &[u16]) -> f64 {
    // Only a recognized ASCII prefix reaches this allocation, never a surrogate.
    let text: String = units.iter().map(|unit| char::from(*unit as u8)).collect();
    text.parse::<f64>().unwrap_or(f64::NAN)
}

fn parse_float(units: &[u16]) -> f64 {
    let units = trimmed(units);
    let mut end = usize::from(matches!(units.first(), Some(0x2b | 0x2d)));
    let negative = units.first() == Some(&0x2d);
    const INFINITY: &[u16] = &[73, 110, 102, 105, 110, 105, 116, 121];
    if units[end..].starts_with(INFINITY) {
        return if negative {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        };
    }
    let integer = end;
    while units.get(end).is_some_and(decimal) {
        end += 1;
    }
    let mut digits = end - integer;
    if units.get(end) == Some(&0x2e) {
        end += 1;
        let fraction = end;
        while units.get(end).is_some_and(decimal) {
            end += 1;
        }
        digits += end - fraction;
    }
    if digits == 0 {
        return f64::NAN;
    }
    if matches!(units.get(end), Some(0x65 | 0x45)) {
        let exponent = end;
        end += 1;
        if matches!(units.get(end), Some(0x2b | 0x2d)) {
            end += 1;
        }
        let start = end;
        while units.get(end).is_some_and(decimal) {
            end += 1;
        }
        if end == start {
            end = exponent;
        }
    }
    decimal_value(&units[..end])
}

/// Convert an entire unsigned sequence in radix 2, 4, 8, 16 or 32.
/// Empty input, invalid digits and suffixes yield NaN; callers remove prefixes.
/// Constant space, one pass, with a single ties-to-even rounding operation.
pub(super) fn power_of_two_integer(units: impl IntoIterator<Item = u16>, radix: u32) -> f64 {
    if !matches!(radix, 2 | 4 | 8 | 16 | 32) {
        return f64::NAN;
    }
    let width = radix.trailing_zeros();
    let mut seen = false;
    let mut bits = 0usize;
    let mut mantissa = 0u64;
    let mut guard = false;
    let mut sticky = false;
    for unit in units {
        seen = true;
        let Some(value) = digit(unit).filter(|value| *value < radix) else {
            return f64::NAN;
        };
        for shift in (0..width).rev() {
            let bit = (value >> shift) & 1;
            if bits == 0 && bit == 0 {
                continue;
            }
            // Once overflow is certain, retain that state but still validate
            // every remaining unit: an invalid suffix must not yield Infinity.
            bits = (bits + 1).min(1025);
            if bits <= 53 {
                mantissa = (mantissa << 1) | u64::from(bit);
            } else if bits == 54 {
                guard = bit != 0;
            } else {
                sticky |= bit != 0;
            }
        }
    }
    if !seen {
        return f64::NAN;
    }
    if guard && (sticky || mantissa & 1 != 0) {
        mantissa += 1;
    }
    if bits > 1024 {
        f64::INFINITY
    } else {
        (mantissa as f64) * 2.0_f64.powi(bits.saturating_sub(53) as i32)
    }
}

fn parse_int(units: &[u16], radix: i32) -> f64 {
    let mut units = trimmed(units);
    let negative = units.first() == Some(&0x2d);
    if matches!(units.first(), Some(0x2b | 0x2d)) {
        units = &units[1..];
    }
    let strip_hex = radix == 0 || radix == 16;
    let mut radix = if radix == 0 { 10 } else { radix };
    if !(2..=36).contains(&radix) {
        return f64::NAN;
    }
    if strip_hex && units.first() == Some(&0x30) && matches!(units.get(1), Some(0x78 | 0x58)) {
        units = &units[2..];
        radix = 16;
    }
    let radix = radix as u32;
    let end = units
        .iter()
        .position(|unit| digit(*unit).is_none_or(|value| value >= radix))
        .unwrap_or(units.len());
    if end == 0 {
        return f64::NAN;
    }
    let units = &units[..end];
    let number = if radix == 10 {
        decimal_value(units)
    } else if radix.is_power_of_two() {
        power_of_two_integer(units.iter().copied(), radix)
    } else {
        // ECMAScript permits implementation-approximated values for these radices.
        units.iter().fold(0.0, |number, unit| {
            number * f64::from(radix) + f64::from(digit(*unit).unwrap_or(0))
        })
    };
    if negative { -number } else { number }
}

impl Runtime {
    fn numeric_input_budget(&mut self, value: &Value) -> Result<(), JsError> {
        let size = match value {
            Value::String(text) => text.len(),
            Value::Utf16(units) => units.len(),
            _ => 0,
        };
        if size > 65_536 {
            return Err(JsError::resource("Numeric input exceeds 65536 units"));
        }
        self.remaining = self
            .remaining
            .checked_sub((size as u64).saturating_mul(8).saturating_add(1))
            .ok_or_else(|| JsError::resource("Numeric conversion work budget exceeded"))?;
        self.charge_string(size.saturating_mul(5))
    }

    fn numeric_global_number(
        &mut self,
        value: Value,
        host: &mut impl Host,
    ) -> Result<f64, JsError> {
        let primitive = self.operator_primitive(value, Hint::Number, host)?;
        self.numeric_input_budget(&primitive)?;
        match self.operator_number(primitive, host)? {
            Value::Number(number) => Ok(number),
            _ => Err(JsError::runtime("Numeric conversion invariant failed")),
        }
    }

    pub(super) fn numeric_global_call(
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
            return Err(self.property_type_error("Numeric methods are not constructors"));
        }
        let first = args.first().cloned().unwrap_or(Value::Undefined);
        if name.starts_with("Number.") {
            let result = match first {
                Value::Number(number) => match name {
                    "Number.isNaN" => number.is_nan(),
                    "Number.isFinite" => number.is_finite(),
                    "Number.isInteger" => number.is_finite() && number.fract() == 0.0,
                    "Number.isSafeInteger" => {
                        number.is_finite()
                            && number.fract() == 0.0
                            && number.abs() <= 9_007_199_254_740_991.0
                    }
                    _ => unreachable!(),
                },
                _ => false,
            };
            return Ok(Some(Value::Bool(result)));
        }
        if matches!(name, "isNaN" | "isFinite") {
            let number = self.numeric_global_number(first, host)?;
            return Ok(Some(Value::Bool(if name == "isNaN" {
                number.is_nan()
            } else {
                number.is_finite()
            })));
        }
        let primitive = self.operator_primitive(first, Hint::String, host)?;
        if matches!(primitive, Value::Symbol(..)) {
            return Err(self.property_type_error("Cannot convert a Symbol to a string"));
        }
        // Explicit String has special Symbol semantics; reject Symbol above.
        // Reusing its primitive path also charges bounded BigInt formatting work.
        let text = self
            .conversion_builtin("String", &[primitive], false, host)?
            .ok_or_else(|| JsError::runtime("String conversion invariant failed"))?;
        self.numeric_input_budget(&text)?;
        let units = crate::strings::units(&text)
            .ok_or_else(|| JsError::runtime("String conversion invariant failed"))?;
        let number = if name == "parseInt" {
            // The first argument's ToString completes before radix coercion.
            let radix =
                self.numeric_global_number(args.get(1).cloned().unwrap_or(Value::Undefined), host)?;
            parse_int(&units, to_uint32(radix) as i32)
        } else {
            parse_float(&units)
        };
        Ok(Some(Value::Number(number)))
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_float, parse_int};
    use crate::{ErrorKind, Host, JsError, Runtime, Value};
    struct Empty;
    impl Host for Empty {
        fn global(&mut self, _: &str) -> Option<Value> {
            None
        }
        fn get(&mut self, _: u64, _: &str) -> Result<Value, JsError> {
            Ok(Value::Undefined)
        }
        fn set(&mut self, _: u64, _: &str, _: Value) -> Result<(), JsError> {
            Ok(())
        }
        fn call(&mut self, _: u64, _: &str, _: &[Value]) -> Result<Value, JsError> {
            Ok(Value::Undefined)
        }
    }
    fn run(source: &str) -> Result<Value, JsError> {
        Runtime::default().execute(source, &mut Empty)
    }
    fn units(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    #[test]
    fn prefix_grammars_and_ecmascript_whitespace() {
        for (text, expected) in [
            ("\u{feff}\u{2028}-1.25e+2tail", -125.0),
            ("1e+oops", 1.0),
            (".5e-1!", 0.05),
            ("1.2.3", 1.2),
            ("0x20", 0.0),
            ("+Infinitytail", f64::INFINITY),
            ("-Infinity!", f64::NEG_INFINITY),
        ] {
            assert_eq!(parse_float(&units(text)), expected, "{text}");
        }
        for text in ["", ".e2", "infinity", "\u{85}1", "\u{180e}1"] {
            assert!(parse_float(&units(text)).is_nan(), "{text}");
        }
        for (text, radix, expected) in [
            ("\u{feff}-0x20tail", 0, -32.0),
            ("0x20", 10, 0.0),
            ("0b11", 0, 0.0),
            ("077", 0, 77.0),
            ("zz?", 36, 1295.0),
            ("+12.9", 10, 12.0),
            ("20000000000003", 16, 9_007_199_254_740_996.0),
            ("9007199254740995", 10, 9_007_199_254_740_996.0),
        ] {
            assert_eq!(parse_int(&units(text), radix), expected, "{text}");
        }
        assert!(parse_int(&units("0x"), 0).is_nan());
        assert!(parse_int(&units("2"), 2).is_nan());
        assert!(parse_int(&units("12"), -2).is_nan());
        assert!(parse_int(&units("-0"), 10).is_sign_negative());
        assert!(parse_float(&units("-0e-999")).is_sign_negative());
        assert_eq!(parse_float(&[49, 50, 0xd800, 51]), 12.0);
        assert_eq!(parse_int(&[49, 50, 0xdc00, 51], 10), 12.0);
        assert!(parse_float(&[0xd800, 49]).is_nan());
    }

    #[test]
    fn global_coercion_order_radix_and_noncoercing_number_methods() {
        for source in [
            "let log='';let a={toString:function(){log+='s';return 'ff!'}};let r={valueOf:function(){log+='r';return 16}};parseInt(a,r)===255&&log==='sr';",
            "let log='';let a={};a[Symbol.toPrimitive]=function(h){log+=h;return '2.5tail'};parseFloat(a)===2.5&&log==='string';",
            "parseInt('10',4294967298)===2&&parseInt('10',-4294967294)===2&&parseInt('10',Infinity)===10;",
            "parseInt(123n)===123&&parseFloat(Object(45n))===45;",
            "let n=0;let a={valueOf:function(){n++;return '12'}};isFinite(a)&&!isNaN(a)&&n===2;",
            "isFinite(null)&&isFinite('')&&!isFinite()&&isNaN()&&!isNaN(null)&&!isNaN('');",
            "let a={valueOf:function(){throw 1}};!Number.isNaN(a)&&!Number.isFinite(a)&&!Number.isInteger(a)&&!Number.isSafeInteger(a);",
            "Number.isNaN(NaN)&&!Number.isNaN('x')&&Number.isFinite(1)&&!Number.isFinite('1')&&!Number.isFinite(1n);",
            "Number.isInteger(-0)&&!Number.isInteger(Infinity)&&!Number.isInteger(1.5)&&Number.isSafeInteger(9007199254740991)&&!Number.isSafeInteger(9007199254740992);",
            "Number.parseInt===parseInt&&Number.parseFloat===parseFloat;",
            "parseInt.length===2&&parseFloat.length===1&&isNaN.length===1&&isFinite.length===1&&Number.isInteger.length===1;",
            "let a={valueOf:function(){throw 9}};parseInt('12',10,a)===12&&parseFloat('2',a)===2&&isFinite(2,a)&&Number.isInteger(2,a);",
            "parseInt.call(null,'ff',16)===255&&parseFloat.apply(null,['1.5!'])===1.5&&isFinite.bind(null,'3')();",
            "let s=String.fromCharCode(49,50,55296);parseInt(s)===12&&parseFloat(s)===12;",
            "1/parseInt('-0')===-Infinity&&1/parseFloat('-0tail')===-Infinity;",
        ] {
            assert_eq!(run(source).unwrap(), Value::Bool(true), "{source}");
        }
    }

    #[test]
    fn symbols_bigint_numbers_and_construction_reject() {
        for source in [
            "parseInt(Symbol('x'));",
            "parseFloat(Symbol('x'));",
            "parseInt('12',1n);",
            "parseInt('12',Object(2n));",
            "isFinite(1n);",
            "isNaN(Object(1n));",
            "isNaN(Symbol());",
            "isFinite({valueOf:function(){return 1n}});",
            "parseFloat({toString:function(){return Symbol()}});",
            "new parseInt('1');",
            "new parseFloat('1');",
            "new isFinite(1);",
            "new isNaN(1);",
            "new Number.isNaN(1);",
            "new Number.isFinite(1);",
            "new Number.isInteger(1);",
            "new Number.isSafeInteger(1);",
        ] {
            assert_eq!(
                run(source).unwrap_err().kind,
                ErrorKind::Runtime,
                "{source}"
            );
        }
        assert_eq!(run("let log='';try{parseInt({toString:function(){throw 'stop'}},{valueOf:function(){log+='r';return 10}})}catch(e){}log==='';").unwrap(), Value::Bool(true));
    }

    #[test]
    fn complete_power_of_two_strings_round_once_and_reject_suffixes() {
        let integer = (1u64 << 57) + 17;
        let expected = ((1u64 << 57) + 32) as f64;
        for (text, radix) in [
            (format!("{integer:b}"), 2),
            (format!("{integer:o}"), 8),
            (format!("{integer:x}"), 16),
        ] {
            assert_eq!(
                super::power_of_two_integer(text.encode_utf16(), radix),
                expected
            );
        }
        // Exact halfway cases round to the even significand on either side.
        assert_eq!(
            super::power_of_two_integer("200000000000010".encode_utf16(), 16),
            (1u64 << 57) as f64
        );
        assert_eq!(
            super::power_of_two_integer("200000000000030".encode_utf16(), 16),
            ((1u64 << 57) + 64) as f64
        );
        assert_eq!(super::power_of_two_integer("0000".encode_utf16(), 2), 0.0);
        for (text, radix) in [
            ("", 16),
            ("ffg", 16),
            ("0x10", 16),
            ("102", 2),
            ("78", 8),
            ("-1", 16),
            ("1 ", 16),
        ] {
            assert!(super::power_of_two_integer(text.encode_utf16(), radix).is_nan());
        }
        assert!(super::power_of_two_integer([49, 0xd800], 2).is_nan());
        let overflow = "f".repeat(300);
        assert_eq!(
            super::power_of_two_integer(overflow.encode_utf16(), 16),
            f64::INFINITY
        );
        assert!(super::power_of_two_integer(format!("{overflow}g").encode_utf16(), 16).is_nan());

        for text in [
            format!("0b{integer:b}"),
            format!("0o{integer:o}"),
            format!("0x{integer:x}"),
        ] {
            for source in [
                format!("Number('{text}');"),
                format!("+'{text}';"),
                format!("Number(Object('{text}'));"),
                format!("+'{text}' + 0;"),
            ] {
                assert_eq!(run(&source).unwrap(), Value::Number(expected), "{source}");
            }
            let source = format!("parseInt('10','{text}');");
            assert_eq!(run(&source).unwrap(), Value::Number(32.0), "{source}");
        }
        for text in [
            "0x", "0b", "0o", "0x1g", "0b102", "0o78", "0xff!", "0b1.0", "+0x1", "-0x1",
        ] {
            let source = format!(
                "Number.isNaN(Number('{text}'))&&Number.isNaN(+'{text}')&&parseInt('10','{text}')===10;"
            );
            assert_eq!(run(&source).unwrap(), Value::Bool(true), "{source}");
        }
        assert_eq!(run("let s='0x1'+String.fromCharCode(55296);Number.isNaN(Number(s))&&Number.isNaN(+s)&&parseInt('10',s)===10;").unwrap(), Value::Bool(true));
    }
    #[test]
    fn prefix_work_and_allocations_are_bounded() {
        let mut runtime = Runtime {
            remaining: 2,
            ..Runtime::default()
        };
        assert_eq!(
            runtime
                .numeric_global_call(
                    "parseFloat",
                    &[Value::string("123456789")],
                    false,
                    &mut Empty
                )
                .unwrap_err()
                .kind,
            ErrorKind::Resource
        );
        let mut runtime = Runtime::default();
        runtime.limits.string_bytes = 1;
        assert_eq!(
            runtime
                .numeric_global_call("parseInt", &[Value::string("123456789")], false, &mut Empty)
                .unwrap_err()
                .kind,
            ErrorKind::Resource
        );
    }
}
