//! Data-only boundary for inference observations, browser action proposals and
//! scoped capabilities. No privileged browser service is exposed by this crate.
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

mod snapshot;
pub use snapshot::{BoundedText, ObservationError, bounded_text, snapshot_limits};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Capability {
    ReadPage,
    ReadSelection,
    SearchTabs,
    Navigate,
    FillForm,
    SubmitForm,
    Delete,
    Share,
}

impl Capability {
    pub fn requires_confirmation(self) -> bool {
        matches!(
            self,
            Self::FillForm | Self::SubmitForm | Self::Delete | Self::Share
        )
    }
}

/// A principal is a particular document generation, not an untrusted URL string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DocumentScope {
    pub tab: u64,
    pub generation: u64,
}

/// The shell builds this bounded observation from its currently committed page.
/// The shell excludes password/hidden controls and raw HTML. Visible text, ordinary
/// fields and URLs may still contain secrets; this is not a credential scrubber.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserSnapshot {
    pub scope: DocumentScope,
    pub title: String,
    pub url: String,
    pub text: String,
    pub selection: String,
    pub tabs: Vec<BrowserTab>,
    pub elements: Vec<BrowserElement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserTab {
    pub id: u64,
    pub title: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserElement {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub value: String,
}

/// A closed action vocabulary. Model output can never evaluate script, open
/// files, run commands or access APIs outside the browser's existing broker.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "tool", rename_all = "snake_case", deny_unknown_fields)]
pub enum BrowserAction {
    ReadPage,
    Navigate { url: String },
    OpenTab { url: String },
    SwitchTab { tab: u64 },
    Select { element: String },
    Type { element: String, text: String },
    Click { element: String },
}

impl BrowserAction {
    pub fn tool_index(&self) -> usize {
        match self {
            Self::ReadPage => 0,
            Self::Navigate { .. } => 1,
            Self::OpenTab { .. } | Self::SwitchTab { .. } => 2,
            Self::Select { .. } => 3,
            Self::Type { .. } => 4,
            Self::Click { .. } => 5,
        }
    }

    pub fn description(&self) -> String {
        match self {
            Self::ReadPage => "Ler a página atual".into(),
            Self::Navigate { url } => format!("Navegar para {url}"),
            Self::OpenTab { url } => format!("Abrir uma aba em {url}"),
            Self::SwitchTab { tab } => format!("Alternar para a aba {tab}"),
            Self::Select { element } => format!("Selecionar {element}"),
            Self::Type { element, text } => format!("Digitar em {element}: {text}"),
            Self::Click { element } => format!("Clicar em {element}"),
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        match self {
            Self::Navigate { url } | Self::OpenTab { url } => {
                if url.is_empty() || url.len() > 4096 || url.chars().any(char::is_control) {
                    return Err("Endereço de ação inválido");
                }
            }
            Self::Select { element } | Self::Click { element } => valid_element(element)?,
            Self::Type { element, text } => {
                valid_element(element)?;
                if text.len() > 8192 || text.chars().count() > 2048 || text.contains('\0') {
                    return Err("Texto de ação excede o limite");
                }
            }
            Self::SwitchTab { tab } if *tab == 0 => return Err("Aba inválida"),
            _ => {}
        }
        Ok(())
    }
}

fn valid_element(element: &str) -> Result<(), &'static str> {
    if element.is_empty()
        || element.len() > 48
        || !element
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b':' | b'-' | b'_'))
    {
        Err("Identificador de elemento inválido")
    } else {
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ScopedAction {
    pub scope: DocumentScope,
    pub action: BrowserAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserTurn {
    #[serde(default)]
    pub message: String,
    pub action: Option<BrowserAction>,
}

#[derive(Debug, Default)]
pub struct PermissionGateway {
    grants: HashSet<(DocumentScope, Capability)>,
}

impl PermissionGateway {
    /// Called only from a trusted browser UI event. Page text cannot issue grants.
    pub fn grant(&mut self, scope: DocumentScope, capability: Capability) {
        self.grants.insert((scope, capability));
    }
    pub fn revoke_tab(&mut self, tab: u64) {
        self.grants.retain(|(s, _)| s.tab != tab);
    }
    pub fn authorize(
        &mut self,
        scope: DocumentScope,
        capability: Capability,
    ) -> Result<(), &'static str> {
        if capability.requires_confirmation() {
            if self.grants.remove(&(scope, capability)) {
                return Ok(());
            }
        } else if self.grants.contains(&(scope, capability)) {
            return Ok(());
        }
        Err("Ação sem permissão explícita para este documento")
    }
}

/// Text extracted from a page is explicitly data. A backend must preserve this boundary.
#[derive(Debug, Clone)]
pub struct UntrustedPageContext {
    pub scope: DocumentScope,
    pub text: String,
}

impl UntrustedPageContext {
    pub fn new(scope: DocumentScope, text: &str) -> Self {
        Self {
            scope,
            text: text.chars().take(16_384).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scoped_permissions_and_single_use_confirmations() {
        let mut p = PermissionGateway::default();
        let a = DocumentScope {
            tab: 1,
            generation: 1,
        };
        let b = DocumentScope {
            tab: 1,
            generation: 2,
        };
        assert!(p.authorize(a, Capability::ReadPage).is_err());
        p.grant(a, Capability::ReadPage);
        assert!(p.authorize(a, Capability::ReadPage).is_ok());
        assert!(p.authorize(b, Capability::ReadPage).is_err());
        p.grant(a, Capability::SubmitForm);
        assert!(p.authorize(a, Capability::SubmitForm).is_ok());
        assert!(p.authorize(a, Capability::SubmitForm).is_err());
        p.revoke_tab(1);
        assert!(p.authorize(a, Capability::ReadPage).is_err());
    }
}
