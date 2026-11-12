//! Literal and constructor grammars use separate frames so each recursive
//! expression carries only the temporaries for its own grammar.
use super::*;

impl Parser {
    #[inline(never)]
    pub(super) fn atom_import(&mut self) -> Result<Expr, JsError> {
        if !self.module_context {
            return Err(self.error("Import expressions are only supported in module source"));
        }
        if self.eat(".") {
            if !matches!(&self.token().kind, Kind::Name(name) if name == "meta") {
                return Err(self.error("Expected import.meta"));
            }
            self.index += 1;
            return Ok(Expr::ImportMeta);
        }
        self.require("(")?;
        let specifier = Box::new(self.expression(1)?);
        let options = if self.eat(",") && !self.is(")") {
            let options = Box::new(self.expression(1)?);
            self.eat(",");
            Some(options)
        } else {
            None
        };
        self.require(")")?;
        Ok(Expr::ImportCall { specifier, options })
    }

    #[inline(never)]
    pub(super) fn atom_construct(&mut self) -> Result<Expr, JsError> {
        if self.is("import")
            && self
                .tokens
                .get(self.index + 1)
                .is_some_and(|token| matches!(&token.kind, Kind::Symbol(symbol) if *symbol == "("))
        {
            return Err(self.error("Import calls cannot be constructor targets"));
        }
        if self.depth >= 64 {
            return Err(JsError::resource("Constructor nesting budget exceeded"));
        }
        self.depth += 1;
        let target = self.atom();
        self.depth -= 1;
        let mut target = target?;
        let mut members = 0;
        loop {
            members += 1;
            if members > 64 {
                return Err(JsError::resource("Constructor chain budget exceeded"));
            }
            if self.eat(".") {
                let name = self.property_name()?;
                target = Expr::Member(
                    Box::new(target),
                    Box::new(Expr::Literal(Value::string(name))),
                );
            } else if self.eat("[") {
                let property = self.expression(0)?;
                self.require("]")?;
                target = Expr::Member(Box::new(target), Box::new(property));
            } else {
                break;
            }
        }
        let mut args = Vec::new();
        if self.eat("(") && !self.eat(")") {
            loop {
                crate::call_storage::check_arguments(args.len().saturating_add(1))?;
                args.push(if self.eat("...") {
                    Expr::Spread(Box::new(self.expression(1)?))
                } else {
                    self.expression(1)?
                });
                if self.eat(")") {
                    break;
                }
                self.require(",")?;
            }
        }
        let expression = Expr::Construct(Box::new(target), args);
        depth::validate_expression_depth(&expression)?;
        Ok(expression)
    }

    #[inline(never)]
    pub(super) fn atom_function(&mut self, kind: FunctionKind) -> Result<Expr, JsError> {
        let name = if self.is("(") {
            None
        } else {
            Some(self.name()?)
        };
        let (params, body, parameters) = self.function_body(kind)?;
        Ok(Expr::Function {
            kind,
            method: false,
            name,
            params,
            body,
            arrow: false,
            parameters: Some(parameters),
        })
    }

    #[inline(never)]
    pub(super) fn atom_async_arrow(&mut self) -> Result<Expr, JsError> {
        if self.token().newline {
            return Err(self.error("Line break after async is not supported"));
        }
        let parameters = if self.eat("(") {
            self.parameters(false)?
        } else {
            vec![Parameter {
                pattern: Pattern::Name(self.name()?),
                rest: false,
            }]
        };
        if self.token().newline {
            return Err(self.error("Line break before arrow is forbidden"));
        }
        self.require("=>")?;
        let params = parameter_length_names(&parameters);
        let mut function = self.arrow(params, FunctionKind::Async)?;
        if let Expr::Function {
            parameters: stored, ..
        } = &mut function
        {
            *stored = Some(Arc::new(parameters));
        }
        Ok(function)
    }

    #[inline(never)]
    pub(super) fn atom_array(&mut self) -> Result<Expr, JsError> {
        let mut values = Vec::new();
        if !self.eat("]") {
            loop {
                if self.eat(",") {
                    values.push(Expr::Hole);
                    if values.len() > 4096 {
                        return Err(JsError::resource("Array literal width exceeded"));
                    }
                    if self.eat("]") {
                        break;
                    }
                    continue;
                }
                values.push(if self.eat("...") {
                    Expr::Spread(Box::new(self.expression(1)?))
                } else {
                    self.expression(1)?
                });
                if self.eat("]") {
                    break;
                }
                self.require(",")?;
                if self.eat("]") {
                    break;
                }
            }
        }
        Ok(Expr::Array(values))
    }

    #[inline(never)]
    pub(super) fn atom_object(&mut self) -> Result<Expr, JsError> {
        let mut pairs = Vec::new();
        if !self.eat("}") {
            loop {
                if self.eat("...") {
                    pairs.push((String::new(), Expr::Spread(Box::new(self.expression(1)?))));
                    if self.eat("}") {
                        break;
                    }
                    self.require(",")?;
                    if self.eat("}") {
                        break;
                    }
                    continue;
                }
                let kind = self.method_kind()?;
                let key = self.property_name()?;
                let value = if self.is("(") {
                    let start = self.index;
                    let (params, body, parameters) = self.function_body(kind)?;
                    // Object literal methods do not yet carry a home object.
                    // Refuse super rather than accidentally capturing an outer class home.
                    if Self::object_method_has_super(&self.tokens[start..self.index], 0)? {
                        return Err(self.error("super in object literal methods is not supported"));
                    }
                    Expr::Function {
                        kind,
                        method: true,
                        name: None,
                        params,
                        body,
                        arrow: false,
                        parameters: Some(parameters),
                    }
                } else if kind != FunctionKind::Ordinary {
                    return Err(self.error("Async/generator modifier requires a method body"));
                } else if self.eat(":") {
                    self.expression(1)?
                } else {
                    Expr::Name(key.clone())
                };
                pairs.push((key, value));
                if self.eat("}") {
                    break;
                }
                self.require(",")?;
                if self.eat("}") {
                    break;
                }
            }
        }
        Ok(Expr::Object(pairs))
    }
}
