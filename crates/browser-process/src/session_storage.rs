//! Tab-owned volatile buckets. Never serialized into a profile or diagnostic.
use browser_ipc::{
    SESSION_STORAGE_MAX_ORIGINS, SessionStorageAccess, SessionStorageSeed, SessionStorageSnapshot,
    validate_session_storage_entries,
};
use std::collections::HashMap;

#[derive(Clone, Default)]
pub struct TabSessionStorage {
    buckets: HashMap<String, SessionStorageSnapshot>,
}

pub fn session_storage_origin(final_url: &str) -> Option<String> {
    let url = url::Url::parse(final_url).ok()?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return None;
    }
    match url.origin() {
        url::Origin::Tuple(..) => Some(url.origin().ascii_serialization()),
        url::Origin::Opaque(_) => None,
    }
}
impl TabSessionStorage {
    /// Call only after the broker returns the authorized final document URL.
    /// Read-only selection: an obsolete initial load must not consume a bucket.
    pub fn seed_for_url(&self, final_url: &str) -> (Option<String>, SessionStorageSeed) {
        let Some(origin) = session_storage_origin(final_url) else {
            return (None, SessionStorageSeed::default());
        };
        let seed = if let Some(entries) = self.buckets.get(&origin) {
            SessionStorageSeed {
                access: SessionStorageAccess::ReadWrite,
                entries: entries.clone(),
            }
        } else if self.buckets.len() < SESSION_STORAGE_MAX_ORIGINS {
            SessionStorageSeed {
                access: SessionStorageAccess::ReadWrite,
                entries: vec![],
            }
        } else {
            SessionStorageSeed {
                access: SessionStorageAccess::QuotaExceeded,
                entries: vec![],
            }
        };
        (Some(origin), seed)
    }
    /// Origin is supervisor-owned metadata attached to the exact committed
    /// session, never a renderer field or the address bar's navigation target.
    pub fn commit(&mut self, origin: &str, entries: &SessionStorageSnapshot) -> Result<(), String> {
        if session_storage_origin(origin).as_deref() != Some(origin) {
            return Err("Noncanonical session storage origin".into());
        }
        validate_session_storage_entries(entries)?;
        if !self.buckets.contains_key(origin) && self.buckets.len() >= SESSION_STORAGE_MAX_ORIGINS {
            if entries.is_empty() {
                return Ok(());
            }
            return Err("Tab session storage origin quota exceeded".into());
        }
        if entries.is_empty() && !self.buckets.contains_key(origin) {
            return Ok(());
        }
        self.buckets.insert(origin.to_owned(), entries.clone());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_origins_partition_schemes_ports_not_paths() {
        assert_eq!(
            session_storage_origin("https://EXAMPLE.test:443/a?q=1#f"),
            Some("https://example.test".into())
        );
        for url in ["file:///a", "data:text/html,x", "noir:home", "", "null"] {
            assert_eq!(session_storage_origin(url), None);
        }
        assert_ne!(
            session_storage_origin("http://example.test"),
            session_storage_origin("https://example.test")
        );
        assert_ne!(
            session_storage_origin("https://example.test:444"),
            session_storage_origin("https://example.test")
        );
    }
    #[test]
    fn navigation_reuses_only_final_origin_and_tabs_are_independent() {
        let mut tab = TabSessionStorage::default();
        let entries = vec![("key".into(), "value".into())];
        let (origin, seed) = tab.seed_for_url("https://a.test/first");
        assert!(seed.entries.is_empty());
        // Seed selection alone, including stale initial work, changes nothing.
        assert!(tab.buckets.is_empty());
        tab.commit(&origin.unwrap(), &entries).unwrap();
        assert!(
            tab.seed_for_url("https://b.test/redirect-final")
                .1
                .entries
                .is_empty()
        );
        assert_eq!(
            tab.seed_for_url("https://a.test/second?q=2#x").1.entries,
            entries
        );
        assert!(
            TabSessionStorage::default()
                .seed_for_url("https://a.test")
                .1
                .entries
                .is_empty()
        );
        drop(tab);
        assert!(TabSessionStorage::default().buckets.is_empty());
    }
    #[test]
    fn capacity_never_evicts_and_invalid_snapshots_are_atomic() {
        let mut tab = TabSessionStorage::default();
        for i in 0..SESSION_STORAGE_MAX_ORIGINS {
            tab.commit(
                &format!("https://host{i}.test"),
                &vec![("k".into(), "v".into())],
            )
            .unwrap();
        }
        assert_eq!(
            tab.seed_for_url("https://new.test").1.access,
            SessionStorageAccess::QuotaExceeded
        );
        assert!(
            tab.commit("https://new.test", &vec![("x".into(), "y".into())])
                .is_err()
        );
        assert!(
            tab.commit(
                "https://host0.test",
                &vec![("k".into(), "1".into()), ("k".into(), "2".into())]
            )
            .is_err()
        );
        assert_eq!(
            tab.seed_for_url("https://host0.test").1.entries,
            vec![("k".into(), "v".into())]
        );
    }
}
