//! Lexical class environments and branded private storage.
use crate::{
    Heap, Host, JsError, Runtime, Value,
    language::{ClassInfo, Construction, MethodInfo, PrivateSlot},
    parser::{ClassDefinition, ClassMember, Parameter},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

impl Runtime {
    pub(super) fn language_gc_cached_roots(&self) -> (Vec<Value>, Vec<usize>) {
        let mut values = Vec::new();
        let mut scopes = Vec::new();
        for frame in &self.language.construction {
            values.push(Value::Function(frame.class));
            values.push(Value::Function(frame.new_target));
            values.extend(frame.instance.clone());
            scopes.extend(frame.this_scope);
        }
        (values, scopes)
    }
    pub(super) fn language_gc_trace(&self, id: usize) -> (Vec<Value>, Vec<usize>) {
        self.language.trace_owner(id)
    }
    pub(super) fn language_gc_sweep(&mut self, live: &[bool], _scopes: &[bool]) {
        let live = live
            .iter()
            .enumerate()
            .filter_map(|(id, live)| live.then_some(id))
            .collect::<BTreeSet<_>>();
        self.language.sweep(&live);
    }
    pub(super) fn attach_parameters(
        &mut self,
        function: &Value,
        parameters: Option<Arc<Vec<Parameter>>>,
    ) -> Result<(), JsError> {
        let Value::Function(id) = function else {
            return Err(JsError::runtime("Invalid parameter owner"));
        };
        let Some(Heap::Function(function)) = self.heap.get_mut(*id) else {
            return Err(JsError::runtime("Invalid parameter owner"));
        };
        function.parameters = parameters;
        Ok(())
    }
    pub(super) fn is_class(&self, id: usize) -> bool {
        self.language.classes.contains_key(&id)
    }
    pub(super) fn class_static_parent(&self, id: usize) -> Option<Value> {
        self.language
            .classes
            .get(&id)
            .and_then(|class| class.base.clone())
            .filter(|base| *base != Value::Null)
    }
    pub(super) fn private_class(&self, scope: usize) -> Result<usize, JsError> {
        let id = self
            .lookup_binding(scope, "[[private-class]]")
            .and_then(|scope| self.scopes[scope].bindings.get("[[private-class]]"))
            .and_then(|binding| binding.value.as_number())
            .ok_or_else(|| JsError::runtime("Private name outside its class environment"))?;
        Ok(id as usize)
    }
    pub(super) fn attach_method_scope(
        &mut self,
        function: usize,
        scope: usize,
    ) -> Result<(), JsError> {
        if let Some(info) = self.language.methods.get(&function).cloned() {
            self.declare(scope, "[[home]]", false, true)?;
            self.scopes[scope]
                .bindings
                .get_mut("[[home]]")
                .unwrap()
                .value = info.home;
        }
        Ok(())
    }
    pub(super) fn attach_constructor_scope(
        &mut self,
        frame: Option<usize>,
        scope: usize,
    ) -> Result<bool, JsError> {
        let Some(frame) = frame else { return Ok(false) };
        let class = self
            .language
            .construction
            .get_mut(frame)
            .ok_or_else(|| JsError::runtime("Missing construction frame"))?;
        class.this_scope = Some(scope);
        let derived = self
            .language
            .classes
            .get(&class.class)
            .is_some_and(|class| class.base.is_some());
        Ok(derived)
    }
    pub(super) fn eval_class(
        &mut self,
        definition: &ClassDefinition,
        outer: usize,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        let scope = self.scope(outer, false)?;
        if let Some(name) = &definition.name {
            self.declare(scope, name, false, false)?;
        }
        let previous = self.strict;
        self.strict = true;
        let base_result = match &definition.base {
            Some(expr) => self.eval(expr, scope, host).map(Some),
            None => Ok(None),
        };
        self.strict = previous;
        let base = base_result?;
        if base.as_ref().is_some_and(|base|*base!=Value::Null&&!matches!(base,Value::Function(id)if self.is_class(*id)||matches!(self.heap.get(*id),Some(Heap::Function(function))if function.constructible))){return Err(self.property_type_error("Class extends value is not a supported constructor"));}
        let parent = if let Some(base) = &base {
            if *base == Value::Null {
                None
            } else {
                let value = self.get(base.clone(), "prototype", host)?;
                if !matches!(
                    value,
                    Value::Object(_) | Value::Array(_) | Value::Function(_) | Value::Null
                ) {
                    return Err(
                        self.property_type_error("Superclass prototype must be an object or null")
                    );
                }
                Some(value)
            }
        } else {
            Some(self.object_prototype_value()?)
        };
        let previous = self.strict;
        self.strict = true;
        let result = (|| {
            let value =
                self.make_function(Arc::new(Vec::new()), Arc::new(Vec::new()), scope, false)?;
            let Value::Function(id) = value else {
                unreachable!()
            };
            self.declare(scope, "[[private-class]]", false, true)?;
            self.scopes[scope]
                .bindings
                .get_mut("[[private-class]]")
                .unwrap()
                .value = Value::Number(id as f64);
            if let Some(name) = &definition.name {
                let binding = self.scopes[scope].bindings.get_mut(name).unwrap();
                binding.value = value.clone();
                binding.initialized = true;
            }
            let prototype = self.object(
                BTreeMap::from([("constructor".into(), value.clone())]),
                parent,
            )?;
            self.hide_properties(&prototype, &["constructor"]);
            self.set(value.clone(), "prototype", prototype.clone(), host)?;
            self.property_metadata.entry(id).or_default().insert(
                "prototype".into(),
                crate::descriptors::PropertyMetadata {
                    writable: false,
                    enumerable: false,
                    configurable: false,
                    accessor: None,
                },
            );
            let name = self.string(definition.name.clone().unwrap_or_default())?;
            self.define_class_function_property(value.clone(), "name", name, host)?;
            self.language.classes.insert(
                id,
                ClassInfo {
                    base: base.clone(),
                    prototype: prototype.clone(),
                    constructor: None,
                    fields: definition
                        .members
                        .iter()
                        .filter(|member| !member.method)
                        .cloned()
                        .collect(),
                    private_methods: Vec::new(),
                    scope,
                },
            );
            for member in definition.members.iter().filter(|member| member.method) {
                let function = self.eval(
                    member
                        .value
                        .as_ref()
                        .ok_or_else(|| JsError::runtime("Missing method body"))?,
                    scope,
                    host,
                )?;
                let Value::Function(function_id) = function else {
                    return Err(JsError::runtime("Class method is not a function"));
                };
                if let Some(Heap::Function(data)) = self.heap.get_mut(function_id) {
                    data.strict = true;
                    data.constructible = false;
                }
                let method_name = self.string(member.name.clone())?;
                self.define_class_function_property(function.clone(), "name", method_name, host)?;
                let home = if member.static_member {
                    value.clone()
                } else {
                    prototype.clone()
                };
                self.language.methods.insert(
                    function_id,
                    MethodInfo {
                        class: id,
                        home: home.clone(),
                    },
                );
                if !member.static_member && !member.private && member.name == "constructor" {
                    self.language.classes.get_mut(&id).unwrap().constructor = Some(function_id);
                    let length = self.get(function.clone(), "length", host)?;
                    self.define_class_function_property(value.clone(), "length", length, host)?;
                    continue;
                }
                if member.private {
                    if member.static_member {
                        self.install_private(id, id, member, function)?;
                    } else {
                        self.language
                            .classes
                            .get_mut(&id)
                            .unwrap()
                            .private_methods
                            .push((member.clone(), function));
                    }
                    continue;
                }
                self.install_class_member(home, member, function, host)?;
            }
            for member in definition
                .members
                .iter()
                .filter(|member| !member.method && member.static_member)
            {
                let value_scope = self.scope(scope, false)?;
                self.declare(value_scope, "this", false, true)?;
                self.scopes[value_scope]
                    .bindings
                    .get_mut("this")
                    .unwrap()
                    .value = value.clone();
                self.declare(value_scope, "[[home]]", false, true)?;
                self.scopes[value_scope]
                    .bindings
                    .get_mut("[[home]]")
                    .unwrap()
                    .value = value.clone();
                let initial = match &member.value {
                    Some(expr) => self.eval(expr, value_scope, host)?,
                    None => Value::Undefined,
                };
                if member.private {
                    self.install_private(id, id, member, initial)?;
                } else {
                    self.define_field(value.clone(), &member.name, initial)?;
                }
            }
            Ok(value)
        })();
        self.strict = previous;
        result
    }
    fn define_class_function_property(
        &mut self,
        target: Value,
        key: &str,
        value: Value,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        let descriptor = self.object(
            BTreeMap::from([
                ("value".into(), value),
                ("writable".into(), Value::Bool(false)),
                ("enumerable".into(), Value::Bool(false)),
                ("configurable".into(), Value::Bool(true)),
            ]),
            None,
        )?;
        let key = self.string(key.to_owned())?;
        self.descriptor_native("Object.defineProperty", &[target, key, descriptor], host)?;
        Ok(())
    }
    pub(super) fn define_field(
        &mut self,
        target: Value,
        name: &str,
        value: Value,
    ) -> Result<(), JsError> {
        let id = self.heap_owner(&target)?;
        if !matches!(self.heap.get(id), Some(Heap::Object(_) | Heap::Function(_))) {
            return Err(JsError::runtime("Unsupported class field receiver"));
        }
        if self.reserve_named_property(id, name)? {
            self.property_order
                .entry(id)
                .or_default()
                .push(name.to_owned());
        }
        match &mut self.heap[id] {
            Heap::Object(object) => {
                object.properties.insert(name.to_owned(), value);
            }
            Heap::Function(function) => {
                function.properties.insert(name.to_owned(), value);
            }
            _ => unreachable!(),
        };
        self.property_metadata.entry(id).or_default().insert(
            name.to_owned(),
            crate::descriptors::PropertyMetadata {
                writable: true,
                enumerable: true,
                configurable: true,
                accessor: None,
            },
        );
        Ok(())
    }
    fn install_class_member(
        &mut self,
        target: Value,
        member: &ClassMember,
        function: Value,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        let owner = self.heap_owner(&target)?;
        if let Some(getter) = member.accessor {
            let old = self
                .property_metadata
                .get(&owner)
                .and_then(|map| map.get(&member.name))
                .and_then(|meta| meta.accessor.clone())
                .unwrap_or((Value::Undefined, Value::Undefined));
            self.set_unchecked(target.clone(), &member.name, Value::Undefined, host)?;
            self.property_metadata.entry(owner).or_default().insert(
                member.name.clone(),
                crate::descriptors::PropertyMetadata {
                    writable: false,
                    enumerable: false,
                    configurable: true,
                    accessor: Some(if getter {
                        (function, old.1)
                    } else {
                        (old.0, function)
                    }),
                },
            );
        } else {
            self.set_unchecked(target.clone(), &member.name, function, host)?;
            self.property_metadata.entry(owner).or_default().insert(
                member.name.clone(),
                crate::descriptors::PropertyMetadata {
                    writable: true,
                    enumerable: false,
                    configurable: true,
                    accessor: None,
                },
            );
        }
        Ok(())
    }
    fn install_private(
        &mut self,
        owner: usize,
        class: usize,
        member: &ClassMember,
        value: Value,
    ) -> Result<(), JsError> {
        let slots = self.language.private.entry(owner).or_default();
        if slots.len() >= 128 && !slots.contains_key(&(class, member.name.clone())) {
            return Err(JsError::resource("Private field slot limit exceeded"));
        }
        let key = (class, member.name.clone());
        if let Some(getter) = member.accessor {
            let (get, set) = match slots.remove(&key) {
                Some(PrivateSlot::Accessor(get, set)) => (get, set),
                _ => (None, None),
            };
            slots.insert(
                key,
                if getter {
                    PrivateSlot::Accessor(Some(value), set)
                } else {
                    PrivateSlot::Accessor(get, Some(value))
                },
            );
        } else {
            slots.insert(key, PrivateSlot::Data(value, !member.method));
        }
        Ok(())
    }
    pub(super) fn private_get(
        &mut self,
        object: Value,
        class: usize,
        name: &str,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        let owner = self.heap_owner(&object)?;
        let slot = self
            .language
            .private
            .get(&owner)
            .and_then(|slots| slots.get(&(class, name.to_owned())))
            .cloned()
            .ok_or_else(|| {
                self.property_type_error("Object does not carry the private field brand")
            })?;
        match slot {
            PrivateSlot::Data(value, _) => Ok(value),
            PrivateSlot::Accessor(Some(getter), _) => self.call(getter, Vec::new(), object, host),
            PrivateSlot::Accessor(None, _) => {
                Err(self.property_type_error("Private accessor has no getter"))
            }
        }
    }
    pub(super) fn private_set(
        &mut self,
        object: Value,
        class: usize,
        name: &str,
        value: Value,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        let owner = self.heap_owner(&object)?;
        let key = (class, name.to_owned());
        let slot = self
            .language
            .private
            .get(&owner)
            .and_then(|slots| slots.get(&key))
            .cloned()
            .ok_or_else(|| {
                self.property_type_error("Object does not carry the private field brand")
            })?;
        match slot {
            PrivateSlot::Data(_, true) => {
                self.language
                    .private
                    .get_mut(&owner)
                    .unwrap()
                    .insert(key, PrivateSlot::Data(value, true));
                Ok(())
            }
            PrivateSlot::Accessor(_, Some(setter)) => {
                self.call(setter, vec![value], object, host)?;
                Ok(())
            }
            _ => Err(self.property_type_error("Private member is not writable")),
        }
    }
    fn initialize_fields(
        &mut self,
        class: usize,
        instance: Value,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        let info = self
            .language
            .classes
            .get(&class)
            .cloned()
            .ok_or_else(|| JsError::runtime("Missing class metadata"))?;
        let owner = self.heap_owner(&instance)?;
        for (member, value) in &info.private_methods {
            self.install_private(owner, class, member, value.clone())?;
        }
        for member in &info.fields {
            if member.static_member {
                continue;
            }
            let scope = self.scope(info.scope, false)?;
            self.declare(scope, "this", false, true)?;
            self.scopes[scope].bindings.get_mut("this").unwrap().value = instance.clone();
            self.declare(scope, "[[home]]", false, true)?;
            self.scopes[scope]
                .bindings
                .get_mut("[[home]]")
                .unwrap()
                .value = info.prototype.clone();
            let value = match &member.value {
                Some(expr) => self.eval(expr, scope, host)?,
                None => Value::Undefined,
            };
            if member.private {
                self.install_private(owner, class, member, value)?;
            } else {
                self.define_field(instance.clone(), &member.name, value)?;
            }
        }
        Ok(())
    }
    pub(super) fn construct_class(
        &mut self,
        class: usize,
        args: Vec<Value>,
        new_target: usize,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        crate::call_storage::check_arguments(args.len())?;
        self.tick()?;
        if self.call_depth >= self.limits.call_depth {
            return Err(JsError::resource("Class construction depth exceeded"));
        }
        self.call_depth += 1;
        let previous = self.strict;
        self.strict = true;
        let result = (|| {
            let info = self
                .language
                .classes
                .get(&class)
                .cloned()
                .ok_or_else(|| JsError::runtime("Missing class constructor"))?;
            let instance = if info.base.is_none() {
                let prototype = self.get(Value::Function(new_target), "prototype", host)?;
                let instance = self.object(BTreeMap::new(), Some(prototype))?;
                self.initialize_fields(class, instance.clone(), host)?;
                Some(instance)
            } else {
                None
            };
            let frame = self.language.construction.len();
            self.language.construction.push(Construction {
                class,
                new_target,
                instance: instance.clone(),
                this_scope: None,
            });
            let body_result = if let Some(constructor) = info.constructor {
                self.invoke_function(
                    constructor,
                    args,
                    instance.unwrap_or(Value::Undefined),
                    Some(frame),
                    host,
                )
            } else if info.base.is_some() {
                self.super_construct_frame(frame, args, host)
            } else {
                Ok(Value::Undefined)
            };
            let frame = self
                .language
                .construction
                .pop()
                .ok_or_else(|| JsError::runtime("Missing class construction frame"))?;
            let value = body_result?;
            if matches!(
                value,
                Value::Object(_) | Value::Array(_) | Value::Function(_) | Value::Host(_)
            ) {
                return Ok(value);
            }
            if info.base.is_some() && value != Value::Undefined {
                return Err(self.property_type_error("Derived constructor returned a primitive"));
            }
            frame.instance.ok_or_else(|| {
                self.property_type_error("Derived constructor must initialize this through super")
            })
        })();
        self.strict = previous;
        self.call_depth -= 1;
        result
    }
    fn super_construct_frame(
        &mut self,
        frame: usize,
        args: Vec<Value>,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        crate::call_storage::check_arguments(args.len())?;
        let construction = self
            .language
            .construction
            .get(frame)
            .cloned()
            .ok_or_else(|| JsError::runtime("Missing super construction frame"))?;
        if construction.instance.is_some() {
            return Err(self.property_type_error("Super constructor has already initialized this"));
        }
        let base = self
            .language
            .classes
            .get(&construction.class)
            .and_then(|class| class.base.clone())
            .ok_or_else(|| self.property_type_error("Super call requires a derived constructor"))?;
        let instance = if let Value::Function(base_class) = base.clone()
            && self.is_class(base_class)
        {
            self.construct_class(base_class, args, construction.new_target, host)?
        } else if base != Value::Null {
            let result = self.construct(base, args, host)?;
            let prototype =
                self.get(Value::Function(construction.new_target), "prototype", host)?;
            if let Value::Object(id) = &result
                && let Some(Heap::Object(object)) = self.heap.get_mut(*id)
            {
                object.prototype = Some(prototype);
            }
            result
        } else {
            return Err(self.property_type_error("Null superclass is not a constructor"));
        };
        self.language.construction[frame].instance = Some(instance.clone());
        if let Some(scope) = construction.this_scope {
            let binding = self.scopes[scope]
                .bindings
                .get_mut("this")
                .ok_or_else(|| JsError::runtime("Missing constructor this binding"))?;
            binding.value = instance.clone();
            binding.initialized = true;
        }
        self.initialize_fields(construction.class, instance.clone(), host)?;
        Ok(instance)
    }
    pub(super) fn super_construct(
        &mut self,
        args: Vec<Value>,
        scope: usize,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        crate::call_storage::check_arguments(args.len())?;
        let class = self.private_class(scope)?;
        let frame = self
            .language
            .construction
            .len()
            .checked_sub(1)
            .ok_or_else(|| self.property_type_error("Super call outside construction"))?;
        if self.language.construction[frame].class != class {
            return Err(self.property_type_error("Super constructor belongs to a different class"));
        }
        self.super_construct_frame(frame, args, host)
    }
    pub(super) fn super_base(
        &mut self,
        scope: usize,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        let home = self.read(scope, "[[home]]", host)?;
        match home {
            Value::Function(id) => self
                .class_static_parent(id)
                .ok_or_else(|| self.property_type_error("No static superclass")),
            Value::Object(id) => match self.heap.get(id) {
                Some(Heap::Object(object)) => Ok(object.prototype.clone().unwrap_or(Value::Null)),
                _ => Err(JsError::runtime("Invalid super home")),
            },
            _ => Err(self.property_type_error("Invalid super home")),
        }
    }
    /// Base and receiver must be captured before evaluating the key operand.
    /// The value helper retains the existing explicit Symbol subset boundary.
    pub(super) fn super_get_values(
        &mut self,
        base: Value,
        receiver: Value,
        key: Value,
        host: &mut impl Host,
    ) -> Result<Value, JsError> {
        let key = self.coerce_property_key(key, host)?;
        if matches!(key, Value::Symbol(..)) {
            Err(self.property_type_error("Symbol super access is not implemented"))
        } else {
            let key = self.text(&key, 0)?;
            self.get_with_receiver(base, &key, receiver, host)
        }
    }
    pub(super) fn super_place_values(
        &mut self,
        base: Value,
        receiver: Value,
        key: Value,
        host: &mut impl Host,
    ) -> Result<crate::Place, JsError> {
        let key = self.coerce_property_key(key, host)?;
        if matches!(key, Value::Symbol(..)) {
            return Err(self.property_type_error("Symbol super assignment is not implemented"));
        }
        Ok(crate::Place::SuperMember(
            base,
            receiver,
            self.sync_property_metadata(&key)?,
        ))
    }
    pub(super) fn super_set(
        &mut self,
        mut base: Value,
        receiver: Value,
        key: &str,
        value: Value,
        host: &mut impl Host,
    ) -> Result<(), JsError> {
        for _ in 0..64 {
            self.tick()?;
            if base == Value::Null {
                return self.set(receiver, key, value, host);
            }
            if !matches!(
                base,
                Value::Object(_)
                    | Value::Function(_)
                    | Value::Array(_)
                    | Value::Native("Number" | "Symbol")
            ) && crate::reflection::intrinsic_ancestors::readonly_ancestor_names(&base).is_none()
            {
                return Err(self.property_type_error("Invalid superclass property receiver"));
            }
            let descriptor =
                if crate::reflection::intrinsic_ancestors::readonly_ancestor_names(&base).is_some()
                {
                    self.readonly_ancestor_descriptor(&base, key, false)?
                } else {
                    self.own_descriptor(&base, key, host)?
                };
            if let Some(descriptor) = descriptor {
                if let Some((_, setter)) = descriptor.metadata.accessor {
                    if setter == Value::Undefined {
                        return Err(self.property_type_error("Superclass accessor has no setter"));
                    }
                    self.call(setter, vec![value], receiver, host)?;
                    return Ok(());
                }
                if !descriptor.metadata.writable {
                    return Err(self.property_type_error("Superclass property is not writable"));
                }
                return self.set(receiver, key, value, host);
            }
            base = match base {
                Value::Function(id) => self.function_internal_prototype(id)?,
                Value::Object(id) => match &self.heap[id] {
                    Heap::Object(object) => object.prototype.clone().unwrap_or(Value::Null),
                    _ => unreachable!(),
                },
                Value::Array(id) => {
                    if Some(id) == self.array_prototype {
                        self.object_prototype_value()?
                    } else {
                        self.array_prototype_value()?
                    }
                }
                ref value
                    if crate::reflection::intrinsic_ancestors::readonly_ancestor_names(value)
                        .is_some() =>
                {
                    self.readonly_ancestor_parent(value)?.unwrap()
                }
                Value::Native("Number" | "Symbol") => Value::Native("Function.prototype"),
                _ => unreachable!(),
            };
        }
        Err(JsError::resource("Superclass property depth exceeded"))
    }
}
