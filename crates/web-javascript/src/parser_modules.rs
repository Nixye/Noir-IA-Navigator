//! Static module grammar. Resolution and evaluation belong to the module loader.
use super::*;
use std::collections::BTreeSet;

#[cfg(test)]
#[path = "parser_module_entry_tests.rs"]
mod entry_tests;

fn bounded_module_entries(current: usize, additional: usize) -> Result<(), JsError> {
    if current.saturating_add(additional) > crate::modules::MAX_ENTRIES {
        return Err(JsError::resource(
            "Module requests, imports or exports exceed 256",
        ));
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct ModuleImport {
    pub source: String,
    pub imported: String,
    pub local: String,
}

#[derive(Debug, Clone)]
pub struct ModuleExport {
    pub exported: String,
    pub local: String,
}

#[derive(Debug, Clone, Default)]
pub struct Module {
    pub body: Vec<Stmt>,
    pub requests: Vec<String>,
    pub imports: Vec<ModuleImport>,
    pub exports: Vec<ModuleExport>,
    pub star_exports: Vec<String>,
}

pub fn parse_module(source: &str) -> Result<Module, JsError> {
    if source.len() > 128 * 1024 {
        return Err(JsError::resource("Module exceeds 128 KiB"));
    }
    let mut parser = Parser {
        tokens: lex(source)?,
        index: 0,
        depth: 0,
        expression_frames: 0,
        function_depth: 0,
        function_kind: FunctionKind::Ordinary,
        module_context: true,
        loop_depth: 0,
        switch_depth: 0,
        labels: Vec::new(),
        visits: 0,
        group_visits: 0,
        flat_storage_bytes: 0,
    };
    let mut module = Module::default();
    let mut requests = BTreeSet::new();
    while !matches!(parser.token().kind, Kind::End) {
        parser.count()?;
        if parser.is("import")
            && !parser
                .tokens
                .get(parser.index + 1)
                .is_some_and(|token| matches!(&token.kind, Kind::Symbol(symbol) if *symbol == "(" || *symbol == "."))
        {
            parser.index += 1;
            parser.module_import(&mut module, &mut requests)?;
        } else if parser.eat("export") {
            parser.module_export(&mut module, &mut requests)?;
        } else {
            module.body.push(parser.statement()?);
        }
    }
    validate_module(&module, &mut parser).map_err(JsError::without_input_source_offset)?;
    Ok(module)
}

impl Parser {
    fn module_binding(&mut self) -> Result<String, JsError> {
        let name = self.name()?;
        if matches!(
            name.as_str(),
            "eval"
                | "arguments"
                | "implements"
                | "interface"
                | "package"
                | "private"
                | "protected"
                | "public"
                | "static"
                | "enum"
        ) {
            return Err(self.error("Invalid strict module binding"));
        }
        Ok(name)
    }

    fn module_export_name(&mut self) -> Result<String, JsError> {
        match &self.token().kind {
            Kind::Name(name) | Kind::String(name) => {
                let name = name.clone();
                self.index += 1;
                Ok(name)
            }
            _ => Err(self.error("Expected module export name")),
        }
    }

    fn module_source(
        &mut self,
        module: &mut Module,
        requests: &mut BTreeSet<String>,
    ) -> Result<String, JsError> {
        let Kind::String(source) = &self.token().kind else {
            return Err(self.error("Expected string module specifier"));
        };
        if !requests.contains(source) {
            bounded_module_entries(requests.len(), 1)?;
        }
        let source = source.clone();
        self.index += 1;
        if self.is("with") || self.is("assert") {
            return Err(self.error("Import attributes are not supported"));
        }
        self.semicolon()?;
        if requests.insert(source.clone()) {
            module.requests.push(source.clone());
        }
        Ok(source)
    }

    fn module_import(
        &mut self,
        module: &mut Module,
        requests: &mut BTreeSet<String>,
    ) -> Result<(), JsError> {
        if matches!(self.token().kind, Kind::String(_)) {
            self.module_source(module, requests)?;
            return Ok(());
        }
        let mut bindings = Vec::new();
        if !self.is("*") && !self.is("{") {
            bounded_module_entries(module.imports.len(), 1)?;
            bindings.push(("default".to_owned(), self.module_binding()?));
            if !self.eat(",") {
                self.require("from")?;
                let source = self.module_source(module, requests)?;
                for (imported, local) in bindings {
                    module.imports.push(ModuleImport {
                        source: source.clone(),
                        imported,
                        local,
                    });
                }
                return Ok(());
            }
        }
        if self.eat("*") {
            self.require("as")?;
            bounded_module_entries(module.imports.len(), bindings.len() + 1)?;
            bindings.push(("*".to_owned(), self.module_binding()?));
        } else {
            self.require("{")?;
            while !self.eat("}") {
                self.count()?;
                bounded_module_entries(module.imports.len(), bindings.len() + 1)?;
                let start = self.index;
                let imported = self.module_export_name()?;
                if imported == "*" {
                    return Err(self.error("String-named star imports are not supported"));
                }
                let local = if self.eat("as") {
                    self.module_binding()?
                } else {
                    self.index = start;
                    self.module_binding()?
                };
                bindings.push((imported, local));
                if self.eat("}") {
                    break;
                }
                self.require(",")?;
            }
        }
        self.require("from")?;
        let source = self.module_source(module, requests)?;
        for (imported, local) in bindings {
            module.imports.push(ModuleImport {
                source: source.clone(),
                imported,
                local,
            });
        }
        Ok(())
    }

    fn module_reexport(
        module: &mut Module,
        source: &str,
        imported: String,
        exported: String,
    ) -> Result<(), JsError> {
        bounded_module_entries(module.imports.len(), 1)?;
        bounded_module_entries(module.exports.len(), 1)?;
        let local = format!("[[reexport{}]]", module.imports.len());
        module.imports.push(ModuleImport {
            source: source.to_owned(),
            imported,
            local: local.clone(),
        });
        module.exports.push(ModuleExport { exported, local });
        Ok(())
    }

    fn module_export(
        &mut self,
        module: &mut Module,
        requests: &mut BTreeSet<String>,
    ) -> Result<(), JsError> {
        if self.eat("*") {
            let exported = if self.eat("as") {
                bounded_module_entries(module.imports.len(), 1)?;
                bounded_module_entries(module.exports.len(), 1)?;
                Some(self.module_export_name()?)
            } else {
                bounded_module_entries(module.star_exports.len(), 1)?;
                None
            };
            self.require("from")?;
            let source = self.module_source(module, requests)?;
            if let Some(exported) = exported {
                Self::module_reexport(module, &source, "*".to_owned(), exported)?;
            } else {
                module.star_exports.push(source);
            }
            return Ok(());
        }
        if self.eat("{") {
            let mut entries = Vec::new();
            while !self.eat("}") {
                self.count()?;
                bounded_module_entries(module.exports.len(), entries.len() + 1)?;
                let start = self.index;
                let local = self.module_export_name()?;
                let end = self.index;
                let exported = if self.eat("as") {
                    self.module_export_name()?
                } else {
                    local.clone()
                };
                entries.push((local, exported, start, end));
                if self.eat("}") {
                    break;
                }
                self.require(",")?;
            }
            if self.eat("from") {
                // Local export lists do not consume imports. Check reexports once
                // `from` disambiguates the list, before copying its source.
                bounded_module_entries(module.imports.len(), entries.len())?;
                let source = self.module_source(module, requests)?;
                for (imported, exported, _, _) in entries {
                    if imported == "*" {
                        return Err(self.error("String-named star reexports are not supported"));
                    }
                    Self::module_reexport(module, &source, imported, exported)?;
                }
            } else {
                // Without `from`, the left-hand side must be a local IdentifierReference.
                let resume = self.index;
                for (local, exported, start, end) in entries {
                    self.index = start;
                    self.module_binding()?;
                    debug_assert_eq!(self.index, end);
                    module.exports.push(ModuleExport { exported, local });
                }
                self.index = resume;
                self.semicolon()?;
            }
            return Ok(());
        }
        if self.eat("default") {
            bounded_module_entries(module.exports.len(), 1)?;
            let local;
            let statement;
            if let Some(kind) = self.function_prefix()? {
                local = if self.is("(") {
                    "[[default]]".to_owned()
                } else {
                    self.module_binding()?
                };
                let (params, body, parameters) = self.function_body(kind)?;
                statement = Stmt::FunctionDefinition(
                    local.clone(),
                    Expr::Function {
                        kind,
                        method: false,
                        name: None,
                        params,
                        body,
                        arrow: false,
                        parameters: Some(parameters),
                    },
                );
            } else if self.eat("class") {
                let name = if self.is("{") || self.is("extends") {
                    None
                } else {
                    Some(self.module_binding()?)
                };
                local = name.clone().unwrap_or_else(|| "[[default]]".to_owned());
                statement = Stmt::ClassDefinition(local.clone(), self.class_body(name)?);
            } else {
                local = "[[default]]".to_owned();
                statement = Stmt::Variables(
                    Declaration::Const,
                    vec![(local.clone(), Some(self.expression(1)?))],
                );
                self.semicolon()?;
            }
            depth::validate_statement_depth(&statement)?;
            module.body.push(statement);
            module.exports.push(ModuleExport {
                exported: "default".to_owned(),
                local,
            });
            return Ok(());
        }
        if !(self.is("var")
            || self.is("let")
            || self.is("const")
            || self.is("function")
            || (self.is("async")
                && self.tokens.get(self.index + 1).is_some_and(
                    |token| matches!(&token.kind, Kind::Name(name) if name == "function"),
                ))
            || self.is("class"))
        {
            return Err(self.error("Expected an export declaration"));
        }
        let statement = self.statement()?;
        module_export_declaration(module, &statement)?;
        module.body.push(statement);
        Ok(())
    }
}

fn module_export_binding(module: &mut Module, name: &str) -> Result<(), JsError> {
    bounded_module_entries(module.exports.len(), 1)?;
    module.exports.push(ModuleExport {
        exported: name.to_owned(),
        local: name.to_owned(),
    });
    Ok(())
}

fn module_export_pattern(module: &mut Module, pattern: &Pattern) -> Result<(), JsError> {
    match pattern {
        Pattern::Name(name) => module_export_binding(module, name)?,
        Pattern::Array(values, rest) => {
            for value in values.iter().flatten() {
                module_export_pattern(module, value)?;
            }
            if let Some(rest) = rest {
                module_export_pattern(module, rest)?;
            }
        }
        Pattern::Object(values, rest) => {
            for (_, value) in values {
                module_export_pattern(module, value)?;
            }
            if let Some(rest) = rest {
                module_export_pattern(module, rest)?;
            }
        }
        Pattern::Default(pattern, _) => module_export_pattern(module, pattern)?,
    }
    Ok(())
}

fn module_export_declaration(module: &mut Module, statement: &Stmt) -> Result<(), JsError> {
    match statement {
        Stmt::Variables(_, values) => {
            for (name, _) in values {
                module_export_binding(module, name)?;
            }
        }
        Stmt::PatternVariables(_, values) => {
            for (pattern, _) in values {
                module_export_pattern(module, pattern)?;
            }
        }
        Stmt::FunctionDefinition(name, _) | Stmt::ClassDefinition(name, _) => {
            module_export_binding(module, name)?;
        }
        _ => {}
    }
    Ok(())
}

fn declaration_names(statement: &Stmt, names: &mut Vec<String>) {
    match statement {
        Stmt::Variables(_, values) => names.extend(values.iter().map(|(name, _)| name.clone())),
        Stmt::PatternVariables(_, values) => {
            for (pattern, _) in values {
                pattern.names(names);
            }
        }
        Stmt::FunctionDefinition(name, _) | Stmt::ClassDefinition(name, _) => {
            names.push(name.clone())
        }
        _ => {}
    }
}

// Collect var names across statement scopes, but never descend into functions.
fn module_var_names(statement: &Stmt, names: &mut Vec<String>) {
    match statement {
        Stmt::Variables(Declaration::Var, _) | Stmt::PatternVariables(Declaration::Var, _) => {
            declaration_names(statement, names)
        }
        Stmt::Block(body) => {
            for child in body {
                module_var_names(child, names);
            }
        }
        Stmt::If(_, yes, no) => {
            module_var_names(yes, names);
            if let Some(no) = no {
                module_var_names(no, names);
            }
        }
        Stmt::IfChain(chain) => {
            for (_, body) in chain.clauses() {
                module_var_names(body, names);
            }
            if let Some(fallback) = chain.fallback() {
                module_var_names(fallback, names);
            }
        }
        Stmt::While(_, body) | Stmt::DoWhile(body, _) | Stmt::Label(_, body) => {
            module_var_names(body, names)
        }
        Stmt::For(initial, _, _, body) => {
            if let Some(initial) = initial {
                module_var_names(initial, names);
            }
            module_var_names(body, names);
        }
        Stmt::ForIn(binding, _, body)
        | Stmt::ForOf(binding, _, body)
        | Stmt::ForAwaitOf(binding, _, body) => {
            match binding {
                ForBinding::Declaration(Declaration::Var, name) => names.push(name.clone()),
                ForBinding::Pattern(Declaration::Var, pattern) => pattern.names(names),
                _ => {}
            }
            module_var_names(body, names);
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for child in body
                .iter()
                .chain(catch.iter().flat_map(|(_, body)| body))
                .chain(finally.iter().flatten())
            {
                module_var_names(child, names);
            }
        }
        Stmt::Switch(_, clauses) => {
            for child in clauses.iter().flat_map(|(_, body)| body) {
                module_var_names(child, names);
            }
        }
        _ => {}
    }
}

fn validate_module(module: &Module, parser: &mut Parser) -> Result<(), JsError> {
    let mut lexical = BTreeSet::new();
    for import in &module.imports {
        if !lexical.insert(import.local.clone()) {
            return Err(parser.error("Duplicate module import binding"));
        }
    }
    let mut vars = Vec::new();
    for statement in &module.body {
        parser.count()?;
        module_var_names(statement, &mut vars);
        if !matches!(
            statement,
            Stmt::Variables(Declaration::Var, _) | Stmt::PatternVariables(Declaration::Var, _)
        ) {
            let mut names = Vec::new();
            declaration_names(statement, &mut names);
            for name in names {
                if !lexical.insert(name) {
                    return Err(parser.error("Duplicate module lexical binding"));
                }
            }
        }
    }
    for name in vars {
        if lexical.contains(&name) {
            return Err(parser.error("Module var conflicts with lexical binding"));
        }
        // Var duplicates are legal; do not add to lexical until after checking all vars.
    }
    for statement in &module.body {
        let mut names = Vec::new();
        module_var_names(statement, &mut names);
        lexical.extend(names);
    }
    let mut exported = BTreeSet::new();
    for export in &module.exports {
        if !exported.insert(&export.exported) {
            return Err(parser.error("Duplicate module export"));
        }
        if !lexical.contains(&export.local) {
            return Err(parser.error("Export refers to an undeclared local binding"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_static_imports_exports_and_order() {
        let module = parse_module("import d, { x as y } from 'a'; import * as ns from 'b'; import 'a'; export { y as z }; export * from 'c'; export * as all from 'd'; export { q as r } from 'e';").unwrap();
        assert_eq!(module.requests, ["a", "b", "c", "d", "e"]);
        assert_eq!(module.imports[0].imported, "default");
        assert_eq!(module.imports[2].imported, "*");
        assert_eq!(module.star_exports, ["c"]);
        assert_eq!(module.exports[2].exported, "r");
    }

    #[test]
    fn default_functions_are_hoistable() {
        let module = parse_module("export default function () { return 42; }").unwrap();
        assert!(
            matches!(&module.body[0], Stmt::FunctionDefinition(name, _) if name == "[[default]]")
        );
        assert!(parse_module("export default function named() {} export { named };").is_ok());
        assert!(parse_module("export default class {} ").is_ok());
        assert!(parse_module("export const { a, b: c } = value;").is_ok());
        assert!(parse_module("export default /a+/;").is_ok());
    }

    #[test]
    fn dynamic_import_arguments_and_module_context() {
        let module = parse_module("import('./a'); import('./b',); import('./c', {},);").unwrap();
        assert!(module.requests.is_empty());
        assert!(matches!(
            &module.body[0],
            Stmt::Expression(Expr::ImportCall { options: None, .. })
        ));
        assert!(matches!(
            &module.body[1],
            Stmt::Expression(Expr::ImportCall { options: None, .. })
        ));
        assert!(matches!(
            &module.body[2],
            Stmt::Expression(Expr::ImportCall {
                options: Some(_),
                ..
            })
        ));
        for source in [
            "export function load() { return import('./a'); }",
            "export const load = () => import('./a');",
            "export const text = `${import('./a')}`;",
            "import((a, b), (c, d));",
            "import('a').then(done);",
            "import 'static'; import('dynamic');",
        ] {
            assert!(parse_module(source).is_ok(), "rejected {source}");
        }
        for source in [
            "import();",
            "import('a', {}, {});",
            "import('a',,);",
            "import(...items);",
            "import('a', ...items);",
            "new import('a');",
            "let x = import;",
        ] {
            assert!(parse_module(source).is_err(), "accepted {source}");
        }
        assert!(parse("import('a');").is_err());
        assert!(parse_dynamic_function("", "return import('a');").is_err());
    }

    #[test]
    fn import_meta_is_a_module_expression_and_not_an_assignment_target() {
        let module = parse_module("import.meta;").unwrap();
        assert!(module.requests.is_empty());
        assert!(matches!(
            &module.body[0],
            Stmt::Expression(Expr::ImportMeta)
        ));
        for source in [
            "export const url=import.meta.url;",
            "export function read(){return import.meta;}",
            "export const read=()=>import.meta;",
            "export const text=`${import.meta.url}`;",
            "import.meta.url='changed'; delete import.meta.url;",
            "new import.meta.UserConstructor();",
            "new import.meta;",
            "import './dep.js'; import.meta; import('./dep.js');",
        ] {
            assert!(parse_module(source).is_ok(), "rejected {source}");
        }
        for source in [
            "import.other;",
            "import.'meta';",
            "import[meta];",
            "import.meta=1;",
            "import.meta++;",
            "++import.meta;",
            "new import('./dep.js');",
        ] {
            assert!(parse_module(source).is_err(), "accepted {source}");
        }
        assert!(parse("import.meta;").is_err());
        assert!(parse("function f(){return import.meta;}").is_err());
        assert!(parse_dynamic_function("", "return import.meta;").is_err());
    }

    #[test]
    fn module_early_errors() {
        for source in [
            "import { x } from 'a'; let x;",
            "import { x, x } from 'a';",
            "export let x; export { x };",
            "export { missing };",
            "export default 1; export default 2;",
            "let x; { var x; }",
            "function x() {} var x;",
            "await work();",
            "import x from 'a' with { type: 'json' };",
            "import x, from 'a';",
            "import { default } from 'a';",
            "export { 'x' };",
            "export default 1, 2;",
            "export if (true) {}",
            "import { '*' as x } from 'a';",
            "export { '*' as x } from 'a';",
        ] {
            assert!(parse_module(source).is_err(), "accepted {source}");
        }
        assert!(parse_module("var x; var x; export { x };").is_ok());
        assert!(parse_module("export { x }; if (true) { var x = 1; }").is_ok());
    }
}

#[cfg(test)]
mod async_generator_export_tests {
    use super::*;

    #[test]
    fn exports_async_generator_function_kinds() {
        for (source, exported) in [
            (
                "export async function* stream() { await work(); yield 1; }",
                "stream",
            ),
            ("export default async function*() { yield 1; }", "default"),
            (
                "export default async function*() { yield* source; }",
                "default",
            ),
            (
                "export default async function* stream() { for await (const x of source) { yield x; } }",
                "default",
            ),
        ] {
            let module = parse_module(source).unwrap();
            assert_eq!(module.exports.len(), 1);
            assert_eq!(module.exports[0].exported, exported);
            assert!(matches!(
                &module.body[0],
                Stmt::FunctionDefinition(
                    _,
                    Expr::Function {
                        kind: FunctionKind::AsyncGenerator,
                        ..
                    }
                )
            ));
        }
        assert!(parse_module("export const stream = async function*() { yield 1; };").is_ok());
        for source in [
            "export async\nfunction* stream() {}",
            "export default async\nfunction*() {}",
            "export async function* stream(x = await work()) {}",
            "export async function* stream() {} export { stream };",
        ] {
            assert!(parse_module(source).is_err(), "accepted {source}");
        }
    }
}

#[cfg(test)]
mod if_chain_tests {
    use super::*;

    #[test]
    fn flat_if_module_vars_follow_source_order_and_exclude_nested_functions() {
        let module = parse_module("if (function(){var hiddenCondition;}) {var first;} else if (false) {var [second] = []; function f(){var hiddenBody;}} else {var last;} export {first, second, last};").unwrap();
        let Stmt::IfChain(chain) = &module.body[0] else {
            panic!("expected flat if chain")
        };
        let mut tail = chain.fallback().cloned();
        for (condition, body) in chain.clauses().iter().rev() {
            tail = Some(Stmt::If(
                condition.clone(),
                Box::new(body.clone()),
                tail.map(Box::new),
            ));
        }
        let mut flat = Vec::new();
        module_var_names(&module.body[0], &mut flat);
        let mut nested = Vec::new();
        module_var_names(&tail.unwrap(), &mut nested);
        assert_eq!(flat, nested);
        assert_eq!(flat, ["first", "second", "last"]);
    }

    #[test]
    fn flat_if_module_binding_checks_reach_later_clauses_and_fallback() {
        for source in [
            "import x from 'a'; if (true) {} else if (false) {var x;}",
            "let x; if (true) {} else if (false) {} else {var x;}",
            "if (true) {} else if (false) {function f(){var hidden;}} export {hidden};",
        ] {
            assert!(parse_module(source).is_err(), "accepted {source}");
        }
        assert!(parse_module("let hidden; if (true) {} else if (false) {function f(){var hidden;} let local;} else {let local;}").is_ok());
    }
}
