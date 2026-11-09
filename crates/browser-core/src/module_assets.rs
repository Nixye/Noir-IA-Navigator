//! Bounded static-module discovery. Parsing is pure; only broker callbacks do I/O.
use browser_ipc::ScriptAsset;
use std::collections::{HashMap, HashSet};
use url::Url;

pub(super) const SCRIPT_LIMIT: usize = 128 * 1024;
const TOTAL_LIMIT: usize = 256 * 1024;
const ATTEMPT_LIMIT: usize = 4;
const DISCOVERY_LIMIT: usize = 64;
const ASSET_SRC_LIMIT: usize = 8192;
const MODULE_PREFIX: &str = "noir-module:";

pub(super) struct ScriptSource {
    pub source: String,
    // A classic response may only be reused as a module if its URL and origin
    // satisfy the stricter module policy too.
    pub module_allowed: bool,
}

#[derive(Default)]
pub(super) struct Scripts {
    pub assets: Vec<ScriptAsset>,
    attempts: usize,
    bytes: usize,
    cache: HashMap<String, Option<ScriptSource>>,
    modules: HashSet<String>,
    classics: HashSet<String>,
}

impl Scripts {
    pub fn element(
        &mut self,
        dom: &web_dom::Document,
        id: web_dom::NodeId,
        element: &web_dom::Element,
        base: &Url,
        fetch: &mut impl FnMut(&str, &Url, bool) -> Option<ScriptSource>,
    ) {
        let module = element.tag == "script"
            && element
                .attr("type")
                .is_some_and(|kind| kind.trim().eq_ignore_ascii_case("module"));
        if !module && !super::is_classic_script(element) {
            return;
        }
        if let Some(src) = element.attr("src") {
            let Ok(url) = base.join(src) else {
                return;
            };
            if module {
                self.module(&url, src, fetch);
            } else if self.classics.len() < DISCOVERY_LIMIT
                && self.classics.insert(src.to_owned())
                && let Some(source) = self.source(&url, src, false, fetch)
            {
                self.emit(src.to_owned(), source);
            }
        } else if module {
            let source = dom.text_content(id);
            if source.len() <= SCRIPT_LIMIT {
                self.dependencies(base, &source, fetch);
            }
        }
    }

    fn source(
        &mut self,
        url: &Url,
        src: &str,
        module: bool,
        fetch: &mut impl FnMut(&str, &Url, bool) -> Option<ScriptSource>,
    ) -> Option<String> {
        let key = url.as_str();
        if !self.cache.contains_key(key) {
            if self.attempts >= ATTEMPT_LIMIT {
                return None;
            }
            self.attempts += 1;
            let value = fetch(src, url, module).filter(|value| value.source.len() <= SCRIPT_LIMIT);
            self.cache.insert(key.to_owned(), value);
        }
        let value = self.cache.get(key)?.as_ref()?;
        if module && !value.module_allowed {
            return None;
        }
        Some(value.source.clone())
    }

    fn emit(&mut self, src: String, source: String) -> bool {
        if src.len() > ASSET_SRC_LIMIT {
            return false;
        }
        if self.assets.len() >= ATTEMPT_LIMIT
            || self.bytes.saturating_add(source.len()) > TOTAL_LIMIT
        {
            return false;
        }
        self.bytes += source.len();
        self.assets.push(ScriptAsset { src, source });
        true
    }

    fn module(
        &mut self,
        url: &Url,
        src: &str,
        fetch: &mut impl FnMut(&str, &Url, bool) -> Option<ScriptSource>,
    ) {
        if url.as_str().len().saturating_add(MODULE_PREFIX.len()) > ASSET_SRC_LIMIT
            || self.modules.len() >= DISCOVERY_LIMIT
            || !self.modules.insert(url.to_string())
        {
            return;
        }
        let Some(source) = self.source(url, src, true, fetch) else {
            return;
        };
        if web_javascript::module_requests(&source).is_err() {
            return;
        }
        if self.emit(format!("{MODULE_PREFIX}{url}"), source.clone()) {
            self.dependencies(url, &source, fetch);
        }
    }

    fn dependencies(
        &mut self,
        base: &Url,
        source: &str,
        fetch: &mut impl FnMut(&str, &Url, bool) -> Option<ScriptSource>,
    ) {
        let Ok(requests) = web_javascript::module_requests(source) else {
            return;
        };
        for specifier in requests {
            let Ok(resolved) = web_javascript::resolve_module_specifier(base.as_str(), &specifier)
            else {
                continue;
            };
            let Ok(url) = Url::parse(&resolved) else {
                continue;
            };
            self.module(&url, &resolved, fetch);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_graph_shares_classic_budget_and_deduplicates_cycles() {
        let dom = web_html::parse(
            "<script src='classic.js'></script><script type='module' src='entry.mjs'></script>",
        )
        .unwrap();
        let base = Url::parse("https://example.com/index.html").unwrap();
        let mut scripts = Scripts::default();
        let mut calls = 0;
        for (id, element) in dom.elements() {
            scripts.element(&dom, id, element, &base, &mut |_, url, _| {
                calls += 1;
                let source = match url.path() {
                    "/classic.js" => "var classic = 1;",
                    "/entry.mjs" => "import './dep.mjs';",
                    "/dep.mjs" => "import './entry.mjs';",
                    _ => return None,
                };
                Some(ScriptSource {
                    source: source.into(),
                    module_allowed: true,
                })
            });
        }
        assert_eq!(calls, 3);
        assert_eq!(scripts.assets.len(), 3);
        assert_eq!(scripts.assets[0].src, "classic.js");
        assert_eq!(
            scripts.assets[1].src,
            "noir-module:https://example.com/entry.mjs"
        );
    }

    #[test]
    fn inline_modules_discover_imports_but_classics_do_not() {
        let dom = web_html::parse(
            "<script src='classic.js'></script><script type='module'>import './dep.mjs';</script>",
        )
        .unwrap();
        let base = Url::parse("https://example.com/index.html").unwrap();
        let mut scripts = Scripts::default();
        let mut requested = Vec::new();
        for (id, element) in dom.elements() {
            scripts.element(&dom, id, element, &base, &mut |_, url, _| {
                requested.push(url.path().to_owned());
                Some(ScriptSource {
                    source: "import './not-requested.mjs';".into(),
                    module_allowed: false,
                })
            });
        }
        assert_eq!(requested, ["/classic.js", "/dep.mjs"]);
        assert_eq!(scripts.assets.len(), 1);
    }

    #[test]
    fn missing_dependencies_consume_only_four_attempts() {
        let dom = web_html::parse("<script type='module'>import './a.mjs'; import './b.mjs'; import './c.mjs'; import './d.mjs'; import './e.mjs';</script>").unwrap();
        let base = Url::parse("https://example.com/index.html").unwrap();
        let mut scripts = Scripts::default();
        let mut calls = 0;
        for (id, element) in dom.elements() {
            scripts.element(&dom, id, element, &base, &mut |_, _, _| {
                calls += 1;
                None
            });
        }
        assert_eq!(calls, 4);
        assert!(scripts.assets.is_empty());
    }
    #[test]
    fn local_modules_stay_inside_authorized_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("allowed");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(dir.path().join("outside.mjs"), "export const secret = 1;").unwrap();
        std::fs::write(
            root.join("entry.mjs"),
            "import './dep.mjs'; import '../outside.mjs';",
        )
        .unwrap();
        std::fs::write(root.join("dep.mjs"), "export const value = 1;").unwrap();
        let page = root.join("index.html");
        std::fs::write(&page, "<script type='module' src='entry.mjs'></script>").unwrap();
        let loaded = crate::load_document(page.to_str().unwrap()).unwrap();
        assert_eq!(loaded.scripts.len(), 2);
        assert!(
            loaded
                .scripts
                .iter()
                .all(|asset| !asset.src.ends_with("outside.mjs"))
        );
        assert!(
            loaded
                .scripts
                .iter()
                .any(|asset| asset.src.ends_with("dep.mjs"))
        );
    }
    #[test]
    fn denied_module_keeps_classics_without_module_marker() {
        let dom = web_html::parse("<script src='https://cdn.example.com/a.js'></script><script type='module' src='https://cdn.example.com/a.js'></script><script src='//cdn.example.com/a.js'></script>").unwrap();
        let base = Url::parse("https://example.com/index.html").unwrap();
        let mut scripts = Scripts::default();
        let mut calls = 0;
        for (id, element) in dom.elements() {
            scripts.element(&dom, id, element, &base, &mut |_, _, _| {
                calls += 1;
                Some(ScriptSource {
                    source: "var value = 1;".into(),
                    module_allowed: false,
                })
            });
        }
        assert_eq!(calls, 1);
        assert_eq!(scripts.assets.len(), 2);
        assert!(
            scripts
                .assets
                .iter()
                .all(|asset| !asset.src.starts_with(MODULE_PREFIX))
        );
    }
    #[test]
    fn full_asset_budget_never_promotes_classic_root_to_module() {
        let dom = web_html::parse("<script src='a.js'></script><script src='b.js'></script><script src='c.js'></script><script src='entry.js'></script><script type='module' src='entry.js'></script>").unwrap();
        let base = Url::parse("https://example.com/index.html").unwrap();
        let mut scripts = Scripts::default();
        let mut calls = 0;
        for (id, element) in dom.elements() {
            scripts.element(&dom, id, element, &base, &mut |_, url, _| {
                calls += 1;
                Some(ScriptSource {
                    source: if url.path() == "/entry.js" {
                        "import './a.js';"
                    } else {
                        "var value = 1;"
                    }
                    .into(),
                    module_allowed: url.path() == "/entry.js",
                })
            });
        }
        assert_eq!(calls, 4);
        assert_eq!(scripts.assets.len(), 4);
        assert!(
            scripts
                .assets
                .iter()
                .all(|asset| !asset.src.starts_with(MODULE_PREFIX))
        );
    }

    #[test]
    fn module_discovery_is_capped_at_sixty_four_urls() {
        let mut scripts = Scripts::default();
        let mut calls = 0;
        for index in 0..100 {
            let url = Url::parse(&format!("https://example.com/{index}.mjs")).unwrap();
            scripts.module(&url, url.as_str(), &mut |_, _, _| {
                calls += 1;
                None
            });
        }
        assert_eq!(scripts.modules.len(), DISCOVERY_LIMIT);
        assert_eq!(calls, ATTEMPT_LIMIT);
    }

    #[test]
    fn module_marker_counts_towards_src_limit_before_fetch() {
        let prefix = "https://example.com/";
        let url = Url::parse(&format!(
            "{prefix}{}",
            "a".repeat(ASSET_SRC_LIMIT - prefix.len())
        ))
        .unwrap();
        let mut scripts = Scripts::default();
        let mut calls = 0;
        scripts.module(&url, url.as_str(), &mut |_, _, _| {
            calls += 1;
            Some(ScriptSource {
                source: "export const value = 1;".into(),
                module_allowed: true,
            })
        });
        assert_eq!(calls, 0);
        assert!(scripts.assets.is_empty());
    }
}
