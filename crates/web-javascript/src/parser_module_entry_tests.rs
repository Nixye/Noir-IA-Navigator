use super::*;
use crate::{ErrorKind, Runtime};

const LIMIT: usize = crate::modules::MAX_ENTRIES;
const ENTRY_ERROR: &str = "Module requests, imports or exports exceed 256";

fn names(start: usize, count: usize) -> String {
    (start..start + count)
        .map(|index| format!("n{index}"))
        .collect::<Vec<_>>()
        .join(",")
}

fn named_imports(start: usize, count: usize, source: &str) -> String {
    format!("import {{{}}} from '{source}';", names(start, count))
}

fn local_exports(start: usize, count: usize) -> String {
    let entries = (start..start + count)
        .map(|index| format!("local as e{index}"))
        .collect::<Vec<_>>()
        .join(",");
    format!("export {{{entries}}};")
}

fn assert_entry_error(error: JsError) {
    assert_eq!(error.kind, ErrorKind::Resource);
    assert_eq!(error.message, ENTRY_ERROR);
    assert_eq!(error.offset, None);
    assert!(error.thrown.is_none());
}

fn assert_rejected(source: &str) {
    assert_entry_error(parse_module(source).unwrap_err());
    assert_entry_error(crate::module_requests(source).unwrap_err());
    assert_entry_error(
        Runtime::default()
            .register_module("https://example.test/main.js", source)
            .unwrap_err(),
    );
}

fn assert_registered(source: &str) -> Module {
    let module = parse_module(source).unwrap();
    assert_eq!(crate::module_requests(source).unwrap(), module.requests);
    Runtime::default()
        .register_module("https://example.test/main.js", source)
        .unwrap();
    module
}

#[test]
fn named_import_limit_precedes_long_specifier_amplification() {
    // module_requests accepts unresolved specifiers; the loader's URL bound
    // must not silently become a new restriction of that read-only helper.
    let source = format!("./{}", "s".repeat(64 * 1024 - 2));
    let accepted = named_imports(0, LIMIT, &source);
    let module = parse_module(&accepted).unwrap();
    assert_eq!(module.imports.len(), LIMIT);
    assert!(module.imports.iter().all(|import| import.source == source));
    assert_eq!(
        crate::module_requests(&accepted).unwrap(),
        std::slice::from_ref(&source)
    );
    assert_rejected(&named_imports(0, LIMIT + 1, &source));
    // This input previously expanded a 64 KiB specifier thousands of times.
    assert_rejected(&named_imports(0, 4096, &source));
}

#[test]
fn default_and_namespace_imports_obey_cumulative_limit() {
    for kind in ["default", "namespace"] {
        let mut source = String::new();
        for index in 0..LIMIT {
            source.push_str(&match kind {
                "default" => format!("import n{index} from './dep.js';"),
                _ => format!("import * as n{index} from './dep.js';"),
            });
        }
        let module = assert_registered(&source);
        assert_eq!(module.imports.len(), LIMIT);
        assert_eq!(module.requests, ["./dep.js"]);
        source.push_str("import overflow from './dep.js';");
        assert_rejected(&source);
    }
}

#[test]
fn combined_import_clauses_reserve_each_binding() {
    let accepted = format!("import first, {{{}}} from './dep.js';", names(0, LIMIT - 1));
    assert_eq!(assert_registered(&accepted).imports.len(), LIMIT);
    assert_rejected(&format!(
        "import first, {{{}}} from './dep.js';",
        names(0, LIMIT)
    ));
    for (count, accepted) in [(LIMIT - 2, true), (LIMIT - 1, false)] {
        let source = format!(
            "{}import first, * as ns from './dep.js';",
            named_imports(0, count, "./dep.js")
        );
        if accepted {
            assert_eq!(assert_registered(&source).imports.len(), LIMIT);
        } else {
            assert_rejected(&source);
        }
    }
    let source = format!(
        "{}{}",
        named_imports(0, LIMIT / 2, "./dep.js"),
        named_imports(LIMIT / 2, LIMIT / 2, "./dep.js")
    );
    assert_eq!(assert_registered(&source).imports.len(), LIMIT);
    assert_rejected(&(source + &named_imports(LIMIT, 1, "./dep.js")));
}

#[test]
fn named_reexports_bound_both_collections_before_source_copies() {
    let specifier = format!("./{}", "s".repeat(64 * 1024 - 2));
    let source = format!("export {{{}}} from '{specifier}';", names(0, LIMIT));
    let module = parse_module(&source).unwrap();
    assert_eq!(module.imports.len(), LIMIT);
    assert_eq!(module.exports.len(), LIMIT);
    assert_eq!(
        crate::module_requests(&source).unwrap(),
        std::slice::from_ref(&specifier)
    );
    for count in [LIMIT + 1, 4096] {
        assert_rejected(&format!(
            "export {{{}}} from '{specifier}';",
            names(0, count)
        ));
    }
    let prefix = named_imports(0, LIMIT / 2, "./dep.js");
    let source = format!(
        "{prefix}export {{{}}} from './other.js';",
        names(0, LIMIT / 2)
    );
    let module = assert_registered(&source);
    assert_eq!(module.imports.len(), LIMIT);
    assert_eq!(module.exports.len(), LIMIT / 2);
    assert_rejected(&format!(
        "{prefix}export {{{}}} from '{specifier}';",
        names(0, LIMIT / 2 + 1)
    ));
}

#[test]
fn local_export_lists_use_only_the_export_budget() {
    let prefix = format!("{}let local;", named_imports(0, LIMIT, "./dep.js"));
    let source = format!("{prefix}{}", local_exports(0, LIMIT));
    let module = assert_registered(&source);
    assert_eq!(module.imports.len(), LIMIT);
    assert_eq!(module.exports.len(), LIMIT);
    assert_rejected(&format!("{prefix}{}", local_exports(0, LIMIT + 1)));
    let source = format!(
        "let local;{}{}",
        local_exports(0, LIMIT / 2),
        local_exports(LIMIT / 2, LIMIT / 2)
    );
    assert_eq!(assert_registered(&source).exports.len(), LIMIT);
    assert_rejected(&(source + &local_exports(LIMIT, 1)));
}

#[test]
fn empty_lists_and_duplicate_requests_do_not_consume_extra_entries() {
    let source = format!(
        "{}let local;{}import {{}} from './dep.js';export {{}};export {{}} from './dep.js';import './dep.js';",
        named_imports(0, LIMIT, "./dep.js"),
        local_exports(0, LIMIT)
    );
    let module = assert_registered(&source);
    assert_eq!(module.imports.len(), LIMIT);
    assert_eq!(module.exports.len(), LIMIT);
    assert_eq!(module.requests, ["./dep.js"]);
}

#[test]
fn request_limit_counts_unique_sources_across_declaration_forms() {
    let mut source = (0..LIMIT - 2)
        .map(|index| format!("import './dep{index}.js';"))
        .collect::<String>();
    source.push_str("export * from './star.js';export {} from './empty.js';");
    let module = assert_registered(&source);
    assert_eq!(module.requests.len(), LIMIT);
    source.push_str("import './dep0.js';export * from './dep1.js';");
    assert_eq!(assert_registered(&source).requests.len(), LIMIT);
    for suffix in [
        "import './overflow.js';",
        "import x from './overflow.js';",
        "export {} from './overflow.js';",
        "export * from './overflow.js';",
        "export * as ns from './overflow.js';",
    ] {
        assert_rejected(&format!("{source}{suffix}"));
    }
}

#[test]
fn star_export_limit_is_independent_of_request_deduplication() {
    let source = "export * from './dep.js';".repeat(LIMIT);
    let module = assert_registered(&source);
    assert_eq!(module.star_exports.len(), LIMIT);
    assert_eq!(module.requests, ["./dep.js"]);
    assert_rejected(&(source + "export * from './dep.js';"));
}

#[test]
fn namespace_reexports_consume_import_and_export_entries() {
    let source = (0..LIMIT)
        .map(|index| format!("export * as ns{index} from './dep.js';"))
        .collect::<String>();
    let module = assert_registered(&source);
    assert_eq!(module.imports.len(), LIMIT);
    assert_eq!(module.exports.len(), LIMIT);
    assert_rejected(&(source + "export * as overflow from './dep.js';"));
    let imports = named_imports(0, LIMIT, "./dep.js");
    assert_rejected(&(imports + "export * as ns from './dep.js';"));
    let exports = format!("let local;{}", local_exports(0, LIMIT));
    assert_rejected(&(exports + "export * as ns from './dep.js';"));
}

#[test]
fn exported_declarations_check_each_binding_and_default() {
    for count in [LIMIT, LIMIT + 1] {
        let mut declarations = vec![format!("export let {};", names(0, count))];
        for (open, close) in [("{", "}"), ("[", "]")] {
            // Each binding pattern already has its own width limit of 128.
            let groups = (0..count)
                .step_by(128)
                .map(|start| {
                    format!(
                        "{open}{}{close} = value",
                        names(start, (count - start).min(128))
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            declarations.push(format!("export const {groups};"));
        }
        for declaration in declarations {
            if count == LIMIT {
                assert_eq!(assert_registered(&declaration).exports.len(), LIMIT);
            } else {
                assert_rejected(&declaration);
            }
        }
    }
    for suffix in [
        "export default 1;",
        "export default function () {}",
        "export default class {}",
        "export function extra() {}",
        "export class Extra {}",
        "export const {a: [extra = 1, ...rest]} = value;",
        "export const {a: extra, ...rest} = value;",
    ] {
        let extra = if suffix.contains("...rest") { 2 } else { 1 };
        let accepted = format!("export let {};{suffix}", names(0, LIMIT - extra));
        assert_eq!(assert_registered(&accepted).exports.len(), LIMIT);
        let rejected = format!("export let {};{suffix}", names(0, LIMIT - extra + 1));
        assert_rejected(&rejected);
    }
}

#[test]
fn entry_limit_stops_before_invalid_tail_and_semantic_validation() {
    // Once a count is exhausted, the resource error intentionally takes
    // precedence over later grammar or duplicate-binding errors.
    assert_rejected(&format!("import {{{}}} from 0;", names(0, LIMIT + 1)));
    assert_rejected(&format!("export {{{}}} from 0;", names(0, LIMIT + 1)));
    assert_rejected(&format!(
        "import {{{}}} from './dep.js';",
        "x,".repeat(LIMIT + 1)
    ));
    for source in [
        "import {x, x} from './dep.js';",
        "export {'*' as x} from './dep.js';",
        "import {'*' as x} from './dep.js';",
        "export {missing};",
    ] {
        assert_eq!(parse_module(source).unwrap_err().kind, ErrorKind::Syntax);
    }
}
