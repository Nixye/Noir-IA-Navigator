//! Versioned preferences, workspace sessions and user-selected favorites.
//! Page contents, AI context and credentials are never persisted here.
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs, io,
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub const MAX_TABS: usize = 24;
pub const MAX_WORKSPACES: usize = 8;
pub const MAX_TOTAL_TABS: usize = 128;
pub const MAX_FAVORITES: usize = 64;
const MAX_PROFILE_BYTES: usize = 1024 * 1024;
const MAX_URL_BYTES: usize = 4096;
const MAX_IDENTIFIER: u64 = 1_000_000_000_000;
const HOME: &str = "noir:welcome";
const COLORS: [[u8; 3]; 8] = [
    [118, 221, 230],
    [151, 158, 247],
    [238, 182, 112],
    [133, 212, 171],
    [229, 147, 181],
    [116, 182, 237],
    [196, 164, 233],
    [212, 204, 156],
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Workspace {
    pub id: u64,
    pub name: String,
    pub color: [u8; 3],
    pub tabs: Vec<String>,
    pub active_tab: usize,
    #[serde(default)]
    pub pinned: bool,
    /// Positions in `tabs`; independent of the transient runtime tab identifier.
    #[serde(default)]
    pub pinned_tabs: Vec<usize>,
}
impl Workspace {
    fn new(id: u64, name: &str, color: [u8; 3]) -> Self {
        Self {
            id,
            name: name.into(),
            color,
            tabs: vec![HOME.into()],
            active_tab: 0,
            pinned: false,
            pinned_tabs: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Favorite {
    pub id: u64,
    pub title: String,
    pub url: String,
    pub color: [u8; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub version: u32,
    pub restore_session: bool,
    pub workspaces: Vec<Workspace>,
    pub active_workspace: u64,
    pub favorites: Vec<Favorite>,
    pub show_metrics: bool,
    #[serde(default)]
    pub appearance_revision: u32,
    pub reduce_motion: bool,
    pub acrylic_enabled: bool,
    /// Window tint alpha (30..=255). The system compositor supplies the blur.
    pub acrylic_opacity: u8,
    next_id: u64,
}
impl Default for Profile {
    fn default() -> Self {
        Self {
            version: 2,
            restore_session: false,
            workspaces: vec![
                Workspace::new(1, "Pessoal", COLORS[0]),
                Workspace::new(2, "Trabalho", COLORS[1]),
            ],
            active_workspace: 1,
            favorites: [
                (10, "Google", "https://www.google.com.br/", [113, 173, 249]),
                (
                    11,
                    "Wikipedia",
                    "https://pt.wikipedia.org/",
                    [182, 195, 207],
                ),
                (12, "GitHub", "https://github.com/", [171, 155, 233]),
                (13, "Recursos do Noir", "noir:specimen", COLORS[0]),
            ]
            .into_iter()
            .map(|(id, title, url, color)| Favorite {
                id,
                title: title.into(),
                url: url.into(),
                color,
            })
            .collect(),
            show_metrics: false,
            appearance_revision: 3,
            reduce_motion: false,
            acrylic_enabled: true,
            acrylic_opacity: 185,
            next_id: 14,
        }
    }
}
impl Profile {
    /// Bounds deserialized state without granting authority to a stored URL.
    /// The navigation broker authorizes every destination independently.
    pub fn normalize(&mut self) {
        if self.appearance_revision < 3 {
            self.show_metrics = false;
            self.appearance_revision = 3;
        }
        self.version = 2;
        self.acrylic_opacity = self.acrylic_opacity.clamp(30, 255);
        let mut identifiers = HashSet::new();
        self.workspaces
            .retain(|w| w.id > 0 && w.id < MAX_IDENTIFIER && identifiers.insert(w.id));
        self.workspaces.truncate(MAX_WORKSPACES);
        if self.workspaces.is_empty() {
            self.workspaces
                .push(Workspace::new(1, "Pessoal", COLORS[0]));
            identifiers.insert(1);
        }
        let mut remaining = MAX_TOTAL_TABS;
        for workspace in &mut self.workspaces {
            workspace.name = clean_label(&workspace.name, 40);
            if workspace.name.is_empty() {
                workspace.name = "Espaço".into();
            }
            let old_tabs = std::mem::take(&mut workspace.tabs);
            let old_pins = std::mem::take(&mut workspace.pinned_tabs);
            let old_active = workspace.active_tab;
            workspace.active_tab = 0;
            for (index, tab) in old_tabs.into_iter().enumerate() {
                if workspace.tabs.len() >= MAX_TABS.min(remaining) {
                    break;
                }
                if let Ok(url) = normalized_url(&tab) {
                    let new_index = workspace.tabs.len();
                    if index == old_active {
                        workspace.active_tab = new_index;
                    }
                    if old_pins.contains(&index) {
                        workspace.pinned_tabs.push(new_index);
                    }
                    workspace.tabs.push(url);
                }
            }
            if workspace.tabs.is_empty() {
                workspace.tabs.push(HOME.into());
            }
            remaining = remaining.saturating_sub(workspace.tabs.len());
            workspace.active_tab = workspace.active_tab.min(workspace.tabs.len() - 1);
        }
        // Keep one usable tab per group, shortening earlier large groups if needed.
        while self.workspaces.iter().map(|w| w.tabs.len()).sum::<usize>() > MAX_TOTAL_TABS {
            if let Some(w) = self.workspaces.iter_mut().rev().find(|w| w.tabs.len() > 1) {
                w.tabs.pop();
                w.pinned_tabs.retain(|index| *index < w.tabs.len());
                w.active_tab = w.active_tab.min(w.tabs.len() - 1);
            } else {
                break;
            }
        }
        if !self
            .workspaces
            .iter()
            .any(|w| w.id == self.active_workspace)
        {
            self.active_workspace = self.workspaces[0].id;
        }
        let mut urls = HashSet::new();
        self.favorites.retain_mut(|favorite| {
            if favorite.id == 0 || favorite.id >= MAX_IDENTIFIER || !identifiers.insert(favorite.id)
            {
                return false;
            }
            let Ok(url) = normalized_url(&favorite.url) else {
                return false;
            };
            if !urls.insert(url.clone()) {
                return false;
            }
            favorite.url = url;
            favorite.title = clean_label(&favorite.title, 80);
            if favorite.title.is_empty() {
                favorite.title = "Favorito".into();
            }
            true
        });
        self.favorites.truncate(MAX_FAVORITES);
        let largest = self
            .workspaces
            .iter()
            .map(|w| w.id)
            .chain(self.favorites.iter().map(|f| f.id))
            .max()
            .unwrap_or(1);
        self.next_id = self.next_id.max(largest + 1).min(MAX_IDENTIFIER);
    }
    pub fn active_workspace(&self) -> &Workspace {
        self.workspaces
            .iter()
            .find(|w| w.id == self.active_workspace)
            .unwrap_or(&self.workspaces[0])
    }
    pub fn active_workspace_mut(&mut self) -> &mut Workspace {
        let index = self
            .workspaces
            .iter()
            .position(|w| w.id == self.active_workspace)
            .unwrap_or(0);
        &mut self.workspaces[index]
    }
    fn allocate_id(&mut self) -> Result<u64, String> {
        if self.next_id >= MAX_IDENTIFIER {
            return Err("Limite de identificadores do perfil atingido".into());
        }
        let id = self.next_id;
        self.next_id += 1;
        Ok(id)
    }
    pub fn create_workspace(&mut self, name: &str) -> Result<u64, String> {
        if self.workspaces.len() >= MAX_WORKSPACES {
            return Err(format!("Limite de {MAX_WORKSPACES} espaços de trabalho"));
        }
        let name = required_label(name, 40, "Nome do espaço")?;
        let id = self.allocate_id()?;
        self.workspaces.push(Workspace::new(
            id,
            &name,
            COLORS[self.workspaces.len() % COLORS.len()],
        ));
        Ok(id)
    }
    pub fn rename_workspace(&mut self, id: u64, name: &str) -> Result<(), String> {
        let name = required_label(name, 40, "Nome do espaço")?;
        self.workspaces
            .iter_mut()
            .find(|w| w.id == id)
            .ok_or("Espaço de trabalho não encontrado")?
            .name = name;
        Ok(())
    }
    pub fn select_workspace(&mut self, id: u64) -> Result<(), String> {
        if !self.workspaces.iter().any(|w| w.id == id) {
            return Err("Espaço de trabalho não encontrado".into());
        }
        self.active_workspace = id;
        Ok(())
    }
    pub fn remove_workspace(&mut self, id: u64) -> Result<(), String> {
        if self.workspaces.len() == 1 {
            return Err("Mantenha pelo menos um espaço de trabalho".into());
        }
        let index = self
            .workspaces
            .iter()
            .position(|w| w.id == id)
            .ok_or("Espaço de trabalho não encontrado")?;
        self.workspaces.remove(index);
        if self.active_workspace == id {
            self.active_workspace = self.workspaces[0].id;
        }
        Ok(())
    }
    pub fn add_favorite(&mut self, title: &str, url: &str) -> Result<u64, String> {
        let url = normalized_url(url)?;
        if let Some(existing) = self.favorites.iter().find(|f| f.url == url) {
            return Ok(existing.id);
        }
        if self.favorites.len() >= MAX_FAVORITES {
            return Err(format!("Limite de {MAX_FAVORITES} favoritos"));
        }
        let title = required_label(title, 80, "Título do favorito")?;
        let id = self.allocate_id()?;
        self.favorites.push(Favorite {
            id,
            title,
            url,
            color: COLORS[(id as usize) % COLORS.len()],
        });
        Ok(id)
    }
    pub fn edit_favorite(&mut self, id: u64, title: &str, url: &str) -> Result<(), String> {
        let title = required_label(title, 80, "Título do favorito")?;
        let url = normalized_url(url)?;
        if self.favorites.iter().any(|f| f.id != id && f.url == url) {
            return Err("Este endereço já está nos favoritos".into());
        }
        let favorite = self
            .favorites
            .iter_mut()
            .find(|f| f.id == id)
            .ok_or("Favorito não encontrado")?;
        favorite.title = title;
        favorite.url = url;
        Ok(())
    }
    pub fn remove_favorite(&mut self, id: u64) -> bool {
        let original = self.favorites.len();
        self.favorites.retain(|f| f.id != id);
        self.favorites.len() != original
    }
}

fn clean_label(value: &str, max: usize) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_control())
        .take(max)
        .collect::<String>()
        .trim()
        .to_owned()
}
fn required_label(value: &str, max: usize, kind: &str) -> Result<String, String> {
    let label = clean_label(value, max);
    if label.is_empty() {
        Err(format!("{kind} não pode ficar vazio"))
    } else {
        Ok(label)
    }
}
fn normalized_url(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() || value.len() > MAX_URL_BYTES || value.chars().any(char::is_control) {
        return Err("Endereço vazio, longo ou inválido".into());
    }
    if matches!(value, "noir:welcome" | "noir:specimen" | "noir:settings") {
        return Ok(value.into());
    }
    // Lexical only: retaining an explicit pre-navigation path never opens it.
    if Path::new(value).is_absolute()
        && !value.starts_with("file:")
        && !value.starts_with("\\\\")
        && !value.starts_with("//")
    {
        return Ok(value.into());
    }
    let url = url::Url::parse(value)
        .map_err(|_| "Use HTTP/HTTPS, arquivo local ou uma página interna do Noir")?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err("Não salve credenciais no endereço".into());
    }
    let allowed = match url.scheme() {
        "https" | "http" => url.host_str().is_some(),
        "file" => url.host_str().is_none() && url.to_file_path().is_ok(),
        _ => false,
    };
    let normalized = url.to_string();
    if !allowed || normalized.len() > MAX_URL_BYTES {
        return Err("Tipo de endereço não permitido no perfil".into());
    }
    Ok(normalized)
}

pub fn default_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("org", "Noir", "Noir")
        .map(|d| d.config_dir().join("profile.json"))
}
pub struct LoadReport {
    pub profile: Profile,
    pub recovered_from_backup: bool,
}
pub fn load(path: &Path) -> io::Result<Profile> {
    Ok(load_with_report(path)?.profile)
}
pub fn load_with_report(path: &Path) -> io::Result<LoadReport> {
    match read_profile(path) {
        Ok(profile) => Ok(LoadReport {
            profile,
            recovered_from_backup: false,
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            match read_profile(&backup_path(path)) {
                Ok(profile) => Ok(LoadReport {
                    profile,
                    recovered_from_backup: true,
                }),
                Err(backup) if backup.kind() == io::ErrorKind::NotFound => Ok(LoadReport {
                    profile: Profile::default(),
                    recovered_from_backup: false,
                }),
                Err(_) => Err(error),
            }
        }
        Err(error) if error.kind() == io::ErrorKind::InvalidData => {
            match read_profile(&backup_path(path)) {
                Ok(profile) => Ok(LoadReport {
                    profile,
                    recovered_from_backup: true,
                }),
                Err(_) => Err(error),
            }
        }
        Err(error) => Err(error),
    }
}
fn read_profile(path: &Path) -> io::Result<Profile> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(MAX_PROFILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_PROFILE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Perfil excede 1 MiB",
        ));
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let version = match value.get("version") {
        None => 1,
        Some(version) => version.as_u64().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "Versão de perfil inválida")
        })?,
    };
    let mut profile = match version {
        1 => {
            #[derive(Deserialize)]
            #[serde(default)]
            struct Legacy {
                restore_session: bool,
                tabs: Vec<String>,
                active_tab: usize,
                show_metrics: bool,
            }
            impl Default for Legacy {
                fn default() -> Self {
                    Self {
                        restore_session: false,
                        tabs: vec![HOME.into()],
                        active_tab: 0,
                        show_metrics: true,
                    }
                }
            }
            let legacy: Legacy = serde_json::from_value(value)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            let mut migrated = Profile {
                restore_session: legacy.restore_session,
                show_metrics: legacy.show_metrics,
                ..Profile::default()
            };
            migrated.workspaces[0].tabs = legacy.tabs;
            migrated.workspaces[0].active_tab = legacy.active_tab;
            migrated
        }
        2 => serde_json::from_value(value)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?,
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Versão de perfil incompatível; o arquivo foi preservado",
            ));
        }
    };
    profile.normalize();
    if !profile.restore_session {
        clear_sessions(&mut profile);
    }
    Ok(profile)
}
fn clear_sessions(profile: &mut Profile) {
    for workspace in &mut profile.workspaces {
        // Pinning is an explicit request to retain a destination, like a favorite.
        // Turning general session restoration off still removes every unpinned URL.
        let old_active = workspace.active_tab;
        let old_tabs = std::mem::take(&mut workspace.tabs);
        let old_pins = std::mem::take(&mut workspace.pinned_tabs);
        workspace.active_tab = 0;
        for (index, tab) in old_tabs.into_iter().enumerate() {
            if old_pins.contains(&index) {
                if index == old_active {
                    workspace.active_tab = workspace.tabs.len();
                }
                workspace.pinned_tabs.push(workspace.tabs.len());
                workspace.tabs.push(tab);
            }
        }
        if workspace.tabs.is_empty() {
            workspace.tabs.push(HOME.into());
        }
    }
}
fn storage_bytes(profile: &Profile) -> io::Result<Vec<u8>> {
    let mut p = profile.clone();
    p.normalize();
    if !p.restore_session {
        clear_sessions(&mut p);
    }
    let bytes = serde_json::to_vec_pretty(&p).map_err(io::Error::other)?;
    if bytes.len() > MAX_PROFILE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Perfil excede 1 MiB",
        ));
    }
    Ok(bytes)
}
fn scrub_previous_sessions(previous: &mut Profile, current: &Profile) {
    previous.restore_session = false;
    for workspace in &mut previous.workspaces {
        // A destination which has just been unpinned must not survive in the
        // backup when ordinary session restoration is disabled.
        let retained = current.workspaces.iter().find(|w| w.id == workspace.id);
        workspace.pinned_tabs.retain(|index| {
            let Some(url) = workspace.tabs.get(*index) else {
                return false;
            };
            retained.is_some_and(|current| {
                current
                    .pinned_tabs
                    .iter()
                    .any(|index| current.tabs.get(*index) == Some(url))
            })
        });
    }
    clear_sessions(previous);
}
fn backup_path(path: &Path) -> PathBuf {
    path.with_extension("json.bak")
}
fn atomic_write(path: &Path, data: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("Perfil sem diretório"))?;
    let mut temp = tempfile::Builder::new()
        .prefix(".noir-profile-")
        .tempfile_in(parent)?;
    temp.write_all(data)?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|e| e.error)?;
    #[cfg(unix)]
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}
pub fn save(path: &Path, profile: &Profile) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("Perfil sem diretório"))?;
    fs::create_dir_all(parent)?;
    let data = storage_bytes(profile)?;
    // Preserve a valid recovery state. Turning restoration off scrubs both files.
    let backup = match read_profile(path) {
        Ok(mut previous) => {
            if !profile.restore_session {
                scrub_previous_sessions(&mut previous, profile);
            }
            storage_bytes(&previous)?
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => data.clone(),
        Err(e) if e.kind() == io::ErrorKind::InvalidData => {
            if let Ok(mut previous) = read_profile(&backup_path(path)) {
                if !profile.restore_session {
                    scrub_previous_sessions(&mut previous, profile);
                }
                storage_bytes(&previous)?
            } else {
                data.clone()
            }
        }
        Err(e) => return Err(e),
    };
    atomic_write(&backup_path(path), &backup)?;
    atomic_write(path, &data)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn workspace_tabs_and_selection_are_independent() {
        let mut p = Profile::default();
        p.active_workspace_mut().tabs =
            vec!["https://www.google.com.br/".into(), "noir:specimen".into()];
        p.active_workspace_mut().active_tab = 1;
        p.select_workspace(2).unwrap();
        assert_eq!(p.active_workspace().tabs, [HOME]);
        p.active_workspace_mut().tabs = vec!["https://github.com/".into()];
        p.select_workspace(1).unwrap();
        assert_eq!(p.active_workspace().active_tab, 1);
        assert_eq!(p.active_workspace().tabs.len(), 2);
        let id = p.create_workspace(" Pesquisa ").unwrap();
        p.rename_workspace(id, "Pesquisa final").unwrap();
        assert!(p.workspaces.iter().any(|w| w.name == "Pesquisa final"));
        p.remove_workspace(id).unwrap();
        assert!(p.create_workspace("Novo").unwrap() > id);
    }
    #[test]
    fn legacy_profile_migrates_with_session_privacy() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profile.json");
        fs::write(&path,r#"{"version":1,"restore_session":true,"tabs":["noir:welcome","https://github.com/"],"active_tab":1,"show_metrics":false}"#).unwrap();
        let p = load(&path).unwrap();
        assert_eq!(p.version, 2);
        assert_eq!(p.active_workspace().active_tab, 1);
        assert_eq!(p.active_workspace().tabs.len(), 2);
        assert!(!p.show_metrics);
        fs::write(
            &path,
            r#"{"version":1,"restore_session":false,"tabs":["file:///sensitive.html"]}"#,
        )
        .unwrap();
        assert_eq!(load(&path).unwrap().active_workspace().tabs, [HOME]);
    }
    #[test]
    fn preferences_persist_and_disabling_sessions_scrubs_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profile.json");
        let mut p = Profile {
            restore_session: true,
            ..Profile::default()
        };
        p.active_workspace_mut().tabs = vec!["file:///C:/private/sensitive.html".into()];
        p.rename_workspace(2, "Design").unwrap();
        p.reduce_motion = true;
        save(&path, &p).unwrap();
        assert_eq!(
            load(&path).unwrap().active_workspace().tabs,
            p.active_workspace().tabs
        );
        p.restore_session = false;
        save(&path, &p).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.active_workspace().tabs, [HOME]);
        assert_eq!(loaded.workspaces[1].name, "Design");
        assert!(loaded.reduce_motion);
        assert!(!fs::read_to_string(&path).unwrap().contains("sensitive"));
        assert!(
            !fs::read_to_string(backup_path(&path))
                .unwrap()
                .contains("sensitive")
        );
    }
    #[test]
    fn backup_recovers_corruption_but_unknown_versions_are_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profile.json");
        let p = Profile::default();
        save(&path, &p).unwrap();
        fs::write(&path, "{broken").unwrap();
        let report = load_with_report(&path).unwrap();
        assert!(report.recovered_from_backup);
        assert_eq!(report.profile.workspaces.len(), 2);
        fs::write(&path, "{\"version\":99}").unwrap();
        assert_eq!(load(&path).unwrap_err().kind(), io::ErrorKind::Unsupported);
        assert!(save(&path, &p).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{\"version\":99}");
    }
    #[test]
    fn favorites_are_deduplicated_editable_and_reject_active_schemes() {
        let mut p = Profile::default();
        let id = p.add_favorite("Docs", "https://example.com").unwrap();
        assert_eq!(
            id,
            p.add_favorite("Duplicate", "https://example.com/").unwrap()
        );
        p.edit_favorite(id, "Documentação", "https://example.com/docs")
            .unwrap();
        assert_eq!(
            p.favorites.iter().find(|f| f.id == id).unwrap().title,
            "Documentação"
        );
        for bad in [
            "javascript:alert(1)",
            "data:text/html,x",
            "file://server/private.html",
            "https://user:secret@example.com/",
            "",
        ] {
            assert!(p.add_favorite("Bad", bad).is_err(), "{bad}");
        }
        assert!(p.remove_favorite(id));
        assert!(!p.remove_favorite(id));
    }
    #[test]
    fn corrupted_and_excessive_profile_state_is_bounded() {
        let workspaces = (1..=20)
            .map(|id| Workspace {
                id,
                name: "A\0B".repeat(100),
                color: COLORS[0],
                tabs: vec![HOME.into(); 100],
                active_tab: 900,
                pinned: false,
                pinned_tabs: vec![],
            })
            .collect();
        let favorites = (100..300)
            .map(|id| Favorite {
                id,
                title: "x".repeat(1000),
                url: format!("https://example.com/{id}"),
                color: COLORS[0],
            })
            .collect();
        let mut p = Profile {
            workspaces,
            active_workspace: 900,
            favorites,
            ..Profile::default()
        };
        p.normalize();
        assert_eq!(p.workspaces.len(), MAX_WORKSPACES);
        assert!(p.workspaces.iter().map(|w| w.tabs.len()).sum::<usize>() <= MAX_TOTAL_TABS);
        assert!(p.workspaces.iter().all(|w| w.tabs.len() <= MAX_TABS
            && w.active_tab < w.tabs.len()
            && w.name.chars().count() <= 40
            && !w.name.contains('\0')));
        assert_eq!(p.favorites.len(), MAX_FAVORITES);
        assert_eq!(p.active_workspace, p.workspaces[0].id);
        for id in p
            .workspaces
            .iter()
            .map(|w| w.id)
            .collect::<Vec<_>>()
            .into_iter()
            .skip(1)
        {
            p.remove_workspace(id).unwrap();
        }
        assert!(p.remove_workspace(p.workspaces[0].id).is_err());
    }
}
