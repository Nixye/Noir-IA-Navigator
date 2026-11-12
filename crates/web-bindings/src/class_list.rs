//! Bounded, live DOMTokenList for the class attribute. No separate token cache.
use super::*;
const MAX_BYTES: usize = 32_768;
const MAX_TOKENS: usize = 4096;
const MAX_ARGS: usize = 256;
fn space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{c}')
}
pub(super) fn property_index(name: &str) -> Option<usize> {
    let n = name.parse::<u32>().ok()?;
    (n != u32::MAX && n.to_string() == name).then_some(n as usize)
}
fn bounded(text: &str) -> Result<(), JsError> {
    if text.len() > MAX_BYTES {
        Err(JsError::resource("DOMTokenList text exceeds 32768 bytes"))
    } else {
        Ok(())
    }
}
fn token(text: &str) -> Result<(), JsError> {
    if text.is_empty() || text.chars().any(space) {
        Err(JsError::runtime(
            "DOMTokenList token is empty or contains ASCII whitespace",
        ))
    } else {
        Ok(())
    }
}
fn count(name: &str, args: &[Value]) -> Result<usize, JsError> {
    let n = match name {
        "add" | "remove" => args.len(),
        "replace" => 2,
        "contains" | "toggle" | "item" => 1,
        "toString" => 0,
        _ => return Err(JsError::runtime("Unsupported DOMTokenList method")),
    };
    if args.len() < n {
        return Err(JsError::runtime("DOMTokenList method requires arguments"));
    }
    if n > MAX_ARGS {
        return Err(JsError::resource("DOMTokenList argument limit exceeded"));
    }
    Ok(n)
}
fn index(number: f64) -> usize {
    if !number.is_finite() || number == 0. {
        0
    } else {
        number.trunc().rem_euclid(4_294_967_296.) as u32 as usize
    }
}
impl DomHost<'_> {
    pub(super) fn class_node(&self, object: u64) -> Result<NodeId, JsError> {
        if !(CLASS..LIST).contains(&object) {
            return Err(JsError::runtime("DOMTokenList receiver required"));
        }
        let id = self.node(NODE + object - CLASS)?;
        if self.document.element(id).is_none() {
            return Err(JsError::runtime("DOMTokenList requires an element"));
        }
        Ok(id)
    }
    fn class_raw(&mut self, object: u64) -> Result<Option<String>, JsError> {
        let id = self.class_node(object)?;
        let raw = self.document.element(id).unwrap().attr("class");
        if let Some(raw) = raw {
            bounded(raw)?;
        }
        let size = raw.map_or(0, str::len);
        self.charge_tree_read(size.saturating_add(1))?;
        Ok(self
            .document
            .element(id)
            .unwrap()
            .attr("class")
            .map(str::to_owned))
    }
    pub(super) fn class_tokens(&mut self, object: u64) -> Result<Vec<String>, JsError> {
        let raw = self.class_raw(object)?.unwrap_or_default();
        let mut tokens: Vec<String> = Vec::new();
        for part in raw.split(space).filter(|s| !s.is_empty()) {
            let mut seen = false;
            // Deterministic linear lookup: charge an upper bound for every
            // byte comparison before performing it, including duplicates.
            for existing in &tokens {
                self.charge_tree_read(existing.len().min(part.len()).saturating_add(1))?;
                if existing == part {
                    seen = true;
                    break;
                }
            }
            if !seen {
                if tokens.len() == MAX_TOKENS {
                    return Err(JsError::resource("DOMTokenList token count exceeded"));
                }
                tokens.push(part.to_owned());
            }
        }
        Ok(tokens)
    }
    pub(super) fn class_stringifier(&mut self, object: u64) -> Result<String, JsError> {
        Ok(self.class_raw(object)?.unwrap_or_default())
    }
    pub(super) fn class_get(&mut self, object: u64, property: &str) -> Result<Value, JsError> {
        self.class_node(object)?;
        if let Some(i) = CLASS_METHODS.iter().position(|m| *m == property) {
            return Ok(Value::Host(CLASS_METHOD + i as u64));
        }
        if property == "value" {
            return Ok(Value::string(self.class_raw(object)?.unwrap_or_default()));
        }
        if property == "length" {
            return Ok(Value::Number(self.class_tokens(object)?.len() as f64));
        }
        if let Some(i) = property_index(property) {
            return Ok(self
                .class_tokens(object)?
                .get(i)
                .map_or(Value::Undefined, |s| Value::string(s.clone())));
        }
        Ok(Value::Undefined)
    }
    pub(super) fn class_set_value(&mut self, object: u64, text: String) -> Result<(), JsError> {
        let id = self.class_node(object)?;
        bounded(&text)?;
        self.charge_tree_read(text.len().saturating_add(1))?;
        self.set_attribute(id, "class".into(), text)
    }
    pub(super) fn class_call_runtime(
        &mut self,
        object: u64,
        method: &str,
        args: &[Value],
        this: &Value,
        runtime: &mut Runtime,
    ) -> Result<Value, JsError> {
        let (receiver, name) =
            if (CLASS_METHOD..CLASS_METHOD + CLASS_METHODS.len() as u64).contains(&object) {
                if method != "call" {
                    return Err(JsError::runtime("Invalid DOMTokenList invocation"));
                }
                (
                    this.as_host()
                        .ok_or_else(|| JsError::runtime("DOMTokenList receiver required"))?,
                    CLASS_METHODS[(object - CLASS_METHOD) as usize],
                )
            } else {
                (object, method)
            };
        self.class_node(receiver)?;
        let n = count(name, args)?;
        if name == "item" {
            let i = index(runtime.coerce_to_number(args[0].clone(), self)?);
            return Ok(self
                .class_tokens(receiver)?
                .get(i)
                .map_or(Value::Null, |s| Value::string(s.clone())));
        }
        let mut texts = Vec::with_capacity(n);
        let mut bytes = 0usize;
        for value in args.iter().take(n) {
            let text = runtime.coerce_to_string(value.clone(), self)?;
            bounded(&text)?;
            bytes = bytes.saturating_add(text.len());
            if bytes > MAX_BYTES {
                return Err(JsError::resource(
                    "DOMTokenList converted arguments exceed 32768 bytes",
                ));
            }
            texts.push(text);
        }
        self.class_apply(
            receiver,
            name,
            &texts,
            args.get(1)
                .filter(|v| !matches!(v, Value::Undefined))
                .map(Value::truthy),
        )
    }
    pub(super) fn class_call_primitive(
        &mut self,
        object: u64,
        method: &str,
        args: &[Value],
    ) -> Result<Value, JsError> {
        self.class_node(object)?;
        let n = count(method, args)?;
        if method == "item" {
            let number = args[0].as_number().ok_or_else(|| {
                JsError::runtime(
                    "Direct item lookup requires numeric argument; VM coercion unavailable",
                )
            })?;
            return Ok(self
                .class_tokens(object)?
                .get(index(number))
                .map_or(Value::Null, |s| Value::string(s.clone())));
        }
        let mut texts = Vec::with_capacity(n);
        let mut bytes = 0usize;
        for value in args.iter().take(n) {
            let text = value.primitive_text()?;
            bounded(&text)?;
            bytes = bytes.saturating_add(text.len());
            if bytes > MAX_BYTES {
                return Err(JsError::resource(
                    "DOMTokenList converted arguments exceed 32768 bytes",
                ));
            }
            texts.push(text);
        }
        self.class_apply(
            object,
            method,
            &texts,
            args.get(1)
                .filter(|v| !matches!(v, Value::Undefined))
                .map(Value::truthy),
        )
    }
    fn class_apply(
        &mut self,
        object: u64,
        method: &str,
        args: &[String],
        force: Option<bool>,
    ) -> Result<Value, JsError> {
        let id = self.class_node(object)?;
        if method == "toString" {
            return Ok(Value::string(self.class_raw(object)?.unwrap_or_default()));
        }
        // contains is a membership query, not token validation (DOM standard).
        if method != "contains" {
            for arg in args {
                token(arg)?;
            }
        }
        let raw = self.class_raw(object)?;
        let mut tokens = self.class_tokens(object)?;
        self.charge_tree_read(
            raw.as_ref()
                .map_or(0, String::len)
                .saturating_add(args.iter().map(String::len).sum::<usize>())
                .saturating_mul(args.len().saturating_add(1))
                .saturating_add(1),
        )?;
        let mut result = Value::Undefined;
        match method {
            "contains" => return Ok(Value::Bool(tokens.contains(&args[0]))),
            "add" => {
                for arg in args {
                    if !tokens.contains(arg) {
                        if tokens.len() == MAX_TOKENS {
                            return Err(JsError::resource("DOMTokenList token count exceeded"));
                        }
                        tokens.push(arg.clone());
                    }
                }
            }
            "remove" => tokens.retain(|t| !args.contains(t)),
            "toggle" => {
                let present = tokens.contains(&args[0]);
                let enable = force.unwrap_or(!present);
                if present == enable {
                    return Ok(Value::Bool(enable));
                }
                if enable {
                    if tokens.len() == MAX_TOKENS {
                        return Err(JsError::resource("DOMTokenList token count exceeded"));
                    }
                    tokens.push(args[0].clone());
                } else {
                    tokens.retain(|t| t != &args[0]);
                }
                result = Value::Bool(enable);
            }
            "replace" => {
                let Some(old) = tokens.iter().position(|t| t == &args[0]) else {
                    return Ok(Value::Bool(false));
                };
                if let Some(new) = tokens.iter().position(|t| t == &args[1]) {
                    if old != new {
                        if old < new {
                            tokens[old] = args[1].clone();
                            tokens.remove(new);
                        } else {
                            tokens.remove(old);
                        }
                    }
                } else {
                    tokens[old] = args[1].clone();
                }
                result = Value::Bool(true);
            }
            _ => return Err(JsError::runtime("Unsupported DOMTokenList method")),
        }
        if raw.is_none() && tokens.is_empty() {
            return Ok(result);
        }
        let bytes = tokens
            .iter()
            .map(String::len)
            .sum::<usize>()
            .saturating_add(tokens.len().saturating_sub(1));
        if bytes > MAX_BYTES {
            return Err(JsError::resource("DOMTokenList result exceeds 32768 bytes"));
        }
        self.set_attribute(id, "class".into(), tokens.join(" "))?;
        Ok(result)
    }
}
