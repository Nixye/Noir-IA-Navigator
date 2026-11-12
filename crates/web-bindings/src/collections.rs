//! Bounded live collections. Queries retain roots/filters, never snapshots.
//! Host descriptors, expandos, HTMLCollection prototypes and symbol iteration
//! remain outside this interface; Array.from uses the VM's array-like path.
use super::*;
use std::sync::Arc;

pub(super) const BASE: u64 = 4_000_000;
pub(super) const METHOD: u64 = 120_020;
pub(super) const METHODS: [&str; 2] = ["item", "namedItem"];
pub(super) const NODE_ITEM: u64 = METHOD + 2;
const MAX_COLLECTIONS: usize = 64;
const MAX_NAME_BYTES: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Tag,
    Children,
    ChildNodes,
}

#[derive(Debug, Clone)]
struct Query {
    kind: Kind,
    root: NodeId,
    tag: Arc<str>,
}
#[derive(Debug, Clone, Default)]
pub(super) struct Collections {
    queries: Vec<Query>,
}

pub(super) fn is_collection(object: u64) -> bool {
    (BASE..BASE + MAX_COLLECTIONS as u64).contains(&object)
}
pub(super) fn is_method(object: u64) -> bool {
    (METHOD..=NODE_ITEM).contains(&object)
}
fn tag_method(object: u64) -> bool {
    [
        (DOCUMENT_METHOD, DOCUMENT_METHODS.as_slice()),
        (NODE_METHOD, NODE_METHODS.as_slice()),
    ]
    .iter()
    .any(|(base, methods)| {
        object
            .checked_sub(*base)
            .and_then(|offset| usize::try_from(offset).ok())
            .and_then(|index| methods.get(index))
            == Some(&"getElementsByTagName")
    })
}
pub(super) fn property_index(property: &str) -> Option<usize> {
    let index = property.parse::<u32>().ok()?;
    (index != u32::MAX && index.to_string() == property).then_some(index as usize)
}
fn reserved_name(name: &str) -> bool {
    matches!(
        name,
        "constructor"
            | "toString"
            | "toLocaleString"
            | "valueOf"
            | "hasOwnProperty"
            | "isPrototypeOf"
            | "propertyIsEnumerable"
            | "__proto__"
            | "__defineGetter__"
            | "__defineSetter__"
            | "__lookupGetter__"
            | "__lookupSetter__"
    )
}

enum Lookup<'a> {
    Length,
    Index(usize),
    Name(&'a str),
}

impl DomHost<'_> {
    pub(super) fn is_nodelist_receiver(&self, object: u64) -> bool {
        if (LIST..LIST + 64).contains(&object) {
            return self.state.lists.get((object - LIST) as usize).is_some();
        }
        is_collection(object)
            && self
                .state
                .collections
                .queries
                .get((object - BASE) as usize)
                .is_some_and(|query| query.kind == Kind::ChildNodes)
    }

    fn collection_query(&self, object: u64) -> Result<Query, JsError> {
        if !is_collection(object) {
            return Err(JsError::runtime("HTMLCollection receiver required"));
        }
        self.state
            .collections
            .queries
            .get((object - BASE) as usize)
            .cloned()
            .ok_or_else(|| JsError::runtime("Invalid HTMLCollection handle"))
    }

    pub(super) fn tag_collection(
        &mut self,
        root: u64,
        args: &[Value],
        runtime: &mut Runtime,
    ) -> Result<Value, JsError> {
        let root = self.node_argument(Some(&Value::Host(root)))?;
        if root != 0 && self.document.element(root).is_none() {
            return Err(JsError::runtime(
                "Tag queries require Document or Element receiver",
            ));
        }
        let argument = args
            .first()
            .cloned()
            .ok_or_else(|| JsError::runtime("getElementsByTagName requires an argument"))?;
        // Coercion may mutate the DOM or reenter this method. Read query storage
        // only after conversion, using the shared VM/DOM budgets.
        let tag = runtime.coerce_to_string(argument, self)?;
        if tag.len() > MAX_NAME_BYTES {
            return Err(JsError::resource("Tag query exceeds 1024 bytes"));
        }
        let work = self
            .state
            .collections
            .queries
            .iter()
            .map(|query| query.tag.len().saturating_add(tag.len()).saturating_add(1))
            .sum::<usize>();
        self.charge_tree_read(work.saturating_add(tag.len()).saturating_add(1))?;
        let tag = tag.to_ascii_lowercase();
        if let Some(index) = self.state.collections.queries.iter().position(|query| {
            query.kind == Kind::Tag && query.root == root && query.tag.as_ref() == tag
        }) {
            return Ok(Value::Host(BASE + index as u64));
        }
        if self.state.collections.queries.len() >= MAX_COLLECTIONS {
            return Err(JsError::resource("Live tag collection quota exceeded"));
        }
        let index = self.state.collections.queries.len();
        self.state.collections.queries.push(Query {
            kind: Kind::Tag,
            root,
            tag: Arc::from(tag),
        });
        Ok(Value::Host(BASE + index as u64))
    }

    pub(super) fn child_collection(
        &mut self,
        root: NodeId,
        elements: bool,
    ) -> Result<Value, JsError> {
        let node = self
            .document
            .nodes
            .get(root)
            .ok_or_else(|| JsError::runtime("Invalid collection root"))?;
        if elements && !matches!(node.kind, NodeKind::Document | NodeKind::Element(_)) {
            return Ok(Value::Undefined);
        }
        let kind = if elements {
            Kind::Children
        } else {
            Kind::ChildNodes
        };
        self.charge_tree_read(self.state.collections.queries.len().saturating_add(1))?;
        if let Some(index) = self
            .state
            .collections
            .queries
            .iter()
            .position(|query| query.root == root && query.kind == kind)
        {
            return Ok(Value::Host(BASE + index as u64));
        }
        if self.state.collections.queries.len() >= MAX_COLLECTIONS {
            return Err(JsError::resource("Live collection quota exceeded"));
        }
        let index = self.state.collections.queries.len();
        self.state.collections.queries.push(Query {
            root,
            kind,
            tag: Arc::from(""),
        });
        Ok(Value::Host(BASE + index as u64))
    }

    fn direct_collection_lookup(
        &mut self,
        query: &Query,
        lookup: Lookup<'_>,
    ) -> Result<Value, JsError> {
        let root = self
            .document
            .nodes
            .get(query.root)
            .ok_or_else(|| JsError::runtime("Invalid collection root"))?;
        // Text cannot own children. Direct collections reflect actual children,
        // including inert roots; only descendant tag queries prune inert trees.
        let children = if matches!(root.kind, NodeKind::Text(_)) {
            0
        } else {
            root.children.len()
        };
        if children > MAX_NODES {
            return Err(JsError::resource(
                "Live collection traversal limit exceeded",
            ));
        }
        self.charge_tree_read(children.saturating_add(1))?;
        let mut count = 0;
        for index in 0..children {
            let id = self.document.nodes[query.root].children[index];
            let node = self
                .document
                .nodes
                .get(id)
                .ok_or_else(|| JsError::runtime("Invalid collection node"))?;
            let eligible =
                query.kind == Kind::ChildNodes || matches!(node.kind, NodeKind::Element(_));
            if !eligible {
                continue;
            }
            let work = match (&lookup, &node.kind) {
                (Lookup::Name(name), NodeKind::Element(e)) => name
                    .len()
                    .saturating_add(e.attr("id").map_or(0, str::len))
                    .saturating_add(e.attr("name").map_or(0, str::len)),
                _ => 0,
            };
            self.charge_tree_read(work.saturating_add(1))?;
            let found = match lookup {
                Lookup::Length => false,
                Lookup::Index(index) => index == count,
                Lookup::Name(name) => self
                    .document
                    .element(id)
                    .is_some_and(|e| e.attr("id") == Some(name) || e.attr("name") == Some(name)),
            };
            if found {
                return Ok(Self::node_value(id));
            }
            count += 1;
        }
        Ok(if matches!(lookup, Lookup::Length) {
            Value::Number(count as f64)
        } else {
            Value::Null
        })
    }

    fn collection_lookup(&mut self, object: u64, lookup: Lookup<'_>) -> Result<Value, JsError> {
        let query = self.collection_query(object)?;
        let key_len = match lookup {
            Lookup::Name(name) => name.len(),
            _ => 0,
        };
        self.charge_tree_read(key_len.saturating_add(1))?;
        if key_len > MAX_NAME_BYTES {
            return Err(JsError::resource("Collection name exceeds 1024 bytes"));
        }
        if matches!(lookup, Lookup::Name("")) {
            return Ok(Value::Null);
        }
        if query.kind != Kind::Tag {
            return self.direct_collection_lookup(&query, lookup);
        }
        let root = self
            .document
            .nodes
            .get(query.root)
            .ok_or_else(|| JsError::runtime("Invalid collection root"))?;
        let inactive = matches!(&root.kind, NodeKind::Element(e) if matches!(e.tag.as_str(), "template" | "noscript"));
        let child_count = if inactive { 0 } else { root.children.len() };
        if child_count > MAX_NODES {
            return Err(JsError::resource(
                "Live collection traversal queue exceeded",
            ));
        }
        self.charge_tree_read(child_count)?;
        let mut pending = Vec::with_capacity(child_count);
        if !inactive {
            pending.extend(
                self.document.nodes[query.root]
                    .children
                    .iter()
                    .rev()
                    .map(|&node| (node, 1)),
            );
        }
        let mut count = 0;
        let mut visited = 0;
        while let Some((id, depth)) = pending.pop() {
            visited += 1;
            if depth > MAX_DEPTH || visited > MAX_NODES {
                return Err(JsError::resource(
                    "Live collection traversal limit exceeded",
                ));
            }
            let node = self
                .document
                .nodes
                .get(id)
                .ok_or_else(|| JsError::runtime("Invalid collection node"))?;
            let work = match &node.kind {
                NodeKind::Element(e) => e
                    .tag
                    .len()
                    .saturating_add(query.tag.len())
                    .saturating_add(e.attr("id").map_or(0, str::len))
                    .saturating_add(e.attr("name").map_or(0, str::len))
                    .saturating_add(key_len),
                _ => 0,
            };
            self.charge_tree_read(work.saturating_add(1))?;
            let node = &self.document.nodes[id];
            if let NodeKind::Element(element) = &node.kind {
                if query.tag.as_ref() == "*" || element.tag.eq_ignore_ascii_case(&query.tag) {
                    let found = match lookup {
                        Lookup::Length => false,
                        Lookup::Index(index) => index == count,
                        Lookup::Name(name) => {
                            element.attr("id") == Some(name) || element.attr("name") == Some(name)
                        }
                    };
                    if found {
                        return Ok(Self::node_value(id));
                    }
                    count += 1;
                }
                if matches!(element.tag.as_str(), "template" | "noscript") {
                    continue;
                }
            }
            let children = node.children.len();
            self.charge_tree_read(children)?;
            if pending.len().saturating_add(children) > MAX_NODES {
                return Err(JsError::resource(
                    "Live collection traversal queue exceeded",
                ));
            }
            pending.extend(
                self.document.nodes[id]
                    .children
                    .iter()
                    .rev()
                    .map(|&child| (child, depth + 1)),
            );
        }
        Ok(if matches!(lookup, Lookup::Length) {
            Value::Number(count as f64)
        } else {
            Value::Null
        })
    }

    pub(super) fn collection_get(
        &mut self,
        object: u64,
        property: &str,
    ) -> Result<Option<Value>, JsError> {
        if is_method(object) || tag_method(object) {
            return Ok(Some(match property {
                "length" => Value::Number(1.0),
                "name" => Value::string(if tag_method(object) {
                    "getElementsByTagName"
                } else if object == NODE_ITEM {
                    "item"
                } else {
                    METHODS[(object - METHOD) as usize]
                }),
                _ => Value::Undefined,
            }));
        }
        if !is_collection(object) {
            return Ok(None);
        }
        let query = self.collection_query(object)?;
        self.charge_tree_read(property.len().saturating_add(1))?;
        let value = if property == "length" {
            self.collection_lookup(object, Lookup::Length)?
        } else if query.kind == Kind::ChildNodes && property == "forEach" {
            Value::Native("Array.forEach")
        } else if query.kind == Kind::ChildNodes && property == "item" {
            Value::Host(NODE_ITEM)
        } else if query.kind != Kind::ChildNodes
            && let Some(index) = METHODS.iter().position(|name| *name == property)
        {
            Value::Host(METHOD + index as u64)
        } else if reserved_name(property) {
            Value::Undefined
        } else {
            let value = if let Some(index) = property_index(property) {
                self.collection_lookup(object, Lookup::Index(index))?
            } else if query.kind != Kind::ChildNodes {
                self.collection_lookup(object, Lookup::Name(property))?
            } else {
                Value::Null
            };
            if value == Value::Null {
                Value::Undefined
            } else {
                value
            }
        };
        Ok(Some(value))
    }

    pub(super) fn collection_has(
        &mut self,
        object: u64,
        property: &str,
    ) -> Result<Option<bool>, JsError> {
        if is_method(object) || tag_method(object) {
            return Ok(Some(matches!(property, "length" | "name")));
        }
        // Reads only immutable query data and current DOM metadata, never getters.
        self.collection_get(object, property)
            .map(|value| value.map(|value| value != Value::Undefined))
    }

    pub(super) fn collection_call(
        &mut self,
        object: u64,
        method: &str,
        args: &[Value],
        this: &Value,
        runtime: &mut Runtime,
    ) -> Result<Value, JsError> {
        let name = if is_method(object) {
            if method != "call" {
                return Err(JsError::runtime("Invalid collection method invocation"));
            }
            if object == NODE_ITEM {
                "item"
            } else {
                METHODS[(object - METHOD) as usize]
            }
        } else {
            method
        };
        let receiver = this
            .as_host()
            .ok_or_else(|| JsError::runtime("HTMLCollection receiver required"))?;
        if object == NODE_ITEM {
            let snapshot = (LIST..LIST + 64).contains(&receiver);
            if snapshot {
                if self.state.lists.get((receiver - LIST) as usize).is_none() {
                    return Err(JsError::runtime("Invalid NodeList handle"));
                }
            } else if self.collection_query(receiver)?.kind != Kind::ChildNodes {
                return Err(JsError::runtime("NodeList receiver required"));
            }
            let argument = args
                .first()
                .cloned()
                .ok_or_else(|| JsError::runtime("NodeList.item requires an argument"))?;
            let number = runtime.coerce_to_number(argument, self)?;
            let index = if !number.is_finite() || number == 0. {
                0
            } else {
                number.trunc().rem_euclid(4_294_967_296.) as u32
            };
            if snapshot {
                self.charge_tree_read(1)?;
                return Ok(self.state.lists[(receiver - LIST) as usize]
                    .get(index as usize)
                    .map_or(Value::Null, |&id| Self::node_value(id)));
            }
            return self.collection_lookup(receiver, Lookup::Index(index as usize));
        }
        let query = self.collection_query(receiver)?;
        if ((METHOD..NODE_ITEM).contains(&object) && query.kind == Kind::ChildNodes)
            || (name == "namedItem" && query.kind == Kind::ChildNodes)
        {
            return Err(JsError::runtime("Collection method receiver type mismatch"));
        }
        let argument = args
            .first()
            .cloned()
            .ok_or_else(|| JsError::runtime("Collection lookup requires an argument"))?;
        match name {
            "item" => {
                let number = runtime.coerce_to_number(argument, self)?;
                let index = if !number.is_finite() || number == 0.0 {
                    0
                } else {
                    number.trunc().rem_euclid(4_294_967_296.0) as u32
                };
                self.collection_lookup(receiver, Lookup::Index(index as usize))
            }
            "namedItem" => {
                let name = runtime.coerce_to_string(argument, self)?;
                self.collection_lookup(receiver, Lookup::Name(&name))
            }
            _ => Err(JsError::runtime("Unsupported collection method")),
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod recovery_r3_tests;

#[cfg(test)]
mod recovery_r4_tests;
