//! Data-only, initial-document WAV metadata. No fetch, playback or media events.
use super::*;
use browser_ipc::{AudioMetadata, validate_audio_metadata};

#[derive(Debug, Clone, Default)]
pub(super) struct MediaState {
    assets: Vec<AudioMetadata>,
    pub(super) nodes: BTreeMap<NodeId, usize>,
}

// Bounded parent traversal, without building a full-tree list on each getter.
fn active(document: &Document, id: NodeId) -> bool {
    let mut current = id;
    for _ in 0..=MAX_DEPTH {
        let Some(node) = document.nodes.get(current) else {
            return false;
        };
        if current == 0 {
            return true;
        }
        if matches!(&node.kind, NodeKind::Element(e) if matches!(e.tag.as_str(), "template" | "noscript"))
        {
            return false;
        }
        let Some(parent) = node.parent else {
            return false;
        };
        current = parent;
    }
    false
}

impl DocumentSession {
    /// One seed before any execution; association belongs only to original nodes.
    /// Preload is a current-value gate; restoring it can expose unchanged metadata.
    /// Writing/removing src, even the same value, permanently revokes association.
    pub fn set_audio_metadata(&mut self, assets: Vec<AudioMetadata>) -> Result<(), JsError> {
        if self.media_started || self.initial_started {
            return Err(JsError::runtime("Audio metadata seed is already fixed"));
        }
        self.media_started = true;
        validate_audio_metadata(&assets).map_err(JsError::runtime)?;
        if !assets.is_empty()
            && !self
                .state
                .document_url
                .as_ref()
                .is_some_and(|url| url.scheme() == "file")
        {
            return Err(JsError::runtime("Audio metadata requires a file document"));
        }
        if self.document.nodes.len() > MAX_NODES {
            return Err(JsError::resource("Audio seed DOM limit exceeded"));
        }
        let mut nodes = BTreeMap::new();
        let mut work = 0usize;
        for id in 0..self.document.nodes.len() {
            let Some(element) = self.document.element(id) else {
                continue;
            };
            if element.tag != "audio"
                || !element
                    .attr("preload")
                    .is_some_and(|value| value.eq_ignore_ascii_case("metadata"))
            {
                continue;
            }
            work = work.saturating_add(MAX_DEPTH + 1);
            if work > 4_000_000 {
                return Err(JsError::resource("Audio seed work exceeded"));
            }
            if !active(&self.document, id) {
                continue;
            }
            let Some(src) = element.attr("src") else {
                continue;
            };
            if src.len() > browser_ipc::MAX_AUDIO_SRC_BYTES {
                continue;
            }
            for (index, asset) in assets.iter().enumerate() {
                work = work.saturating_add(src.len().saturating_add(1));
                if work > 4_000_000 {
                    return Err(JsError::resource("Audio seed work exceeded"));
                }
                if asset.src == src {
                    nodes.insert(id, index);
                    break;
                }
            }
        }
        self.state.media = MediaState { assets, nodes };
        Ok(())
    }
}

impl DomHost<'_> {
    pub(super) fn audio_duration(&mut self, id: NodeId) -> Result<Value, JsError> {
        // Charge before parent traversal or bounded source comparison.
        self.charge_tree_read(MAX_DEPTH + browser_ipc::MAX_AUDIO_SRC_BYTES + 32)?;
        let duration = (|| {
            if !self
                .state
                .document_url
                .as_ref()
                .is_some_and(|url| url.scheme() == "file")
            {
                return None;
            }
            if !active(self.document, id) {
                return None;
            }
            let element = self.document.element(id)?;
            if element.tag != "audio"
                || !element
                    .attr("preload")
                    .is_some_and(|value| value.eq_ignore_ascii_case("metadata"))
            {
                return None;
            }
            let index = *self.state.media.nodes.get(&id)?;
            let asset = self.state.media.assets.get(index)?;
            if element.attr("src") != Some(asset.src.as_str()) {
                return None;
            }
            Some(asset.frames as f64 / f64::from(asset.sample_rate))
        })()
        .unwrap_or(f64::NAN);
        Ok(Value::Number(duration))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn asset() -> AudioMetadata {
        AudioMetadata {
            src: "clip.wav".into(),
            sample_rate: 8000,
            channels: 1,
            bits_per_sample: 16,
            block_align: 2,
            frames: 12000,
        }
    }
    fn session() -> DocumentSession {
        let mut s = DocumentSession::new(
            web_html::parse("<body><audio id=a src=clip.wav preload=metadata></audio></body>")
                .unwrap(),
        );
        s.set_document_url("file:///audio.html").unwrap();
        s.set_audio_metadata(vec![asset()]).unwrap();
        s.execute("var a=document.getElementById('a');").unwrap();
        s
    }
    fn duration(s: &mut DocumentSession) -> f64 {
        s.execute("a.duration;").unwrap().as_number().unwrap()
    }
    #[test]
    fn duration_is_readonly_and_preload_is_only_a_current_gate() {
        let mut s = session();
        assert_eq!(
            s.execute("('duration' in a) && !('duration' in document.body);")
                .unwrap(),
            Value::Bool(true)
        );
        assert_eq!(duration(&mut s), 1.5);
        assert!(s.execute("a.duration=9;").is_err());
        assert_eq!(duration(&mut s), 1.5);
        s.execute("a.setAttribute('preload','none');").unwrap();
        assert!(duration(&mut s).is_nan());
        assert_eq!(s.execute("'duration' in a;").unwrap(), Value::Bool(true));
        s.execute("a.setAttribute('preload','metadata');").unwrap();
        assert_eq!(duration(&mut s), 1.5);
    }
    #[test]
    fn any_src_write_or_removal_permanently_revokes_metadata() {
        for source in [
            "a.setAttribute('src','clip.wav');",
            "a.setAttribute('src','other.wav');a.setAttribute('src','clip.wav');",
            "a.removeAttribute('SRC');a.removeAttribute('src');a.setAttribute('src','clip.wav');",
        ] {
            let mut s = session();
            s.execute(source).unwrap();
            assert!(duration(&mut s).is_nan());
        }
    }
    #[test]
    fn clones_fragments_new_and_detached_audio_have_no_available_metadata() {
        let mut s = session();
        s.execute(
            "var copy=a.cloneNode(true);document.body.appendChild(copy);var value=copy.duration;",
        )
        .unwrap();
        assert!(s.execute("value;").unwrap().as_number().unwrap().is_nan());
        s.execute("a.remove();").unwrap();
        assert!(duration(&mut s).is_nan());
        s.execute("document.body.innerHTML='<audio id=b src=clip.wav preload=metadata></audio>';a=document.getElementById('b');").unwrap();
        assert!(duration(&mut s).is_nan());
        s.execute("a=document.createElement('audio');a.setAttribute('src','clip.wav');a.setAttribute('preload','metadata');document.body.appendChild(a);").unwrap();
        assert!(duration(&mut s).is_nan());
    }
    #[test]
    fn resource_rollback_restores_association_but_ordinary_error_does_not() {
        let mut s = session();
        assert_eq!(
            s.execute("a.setAttribute('src','clip.wav');while(true){}")
                .unwrap_err()
                .kind,
            ErrorKind::Resource
        );
        assert_eq!(s.state.media.nodes.len(), 1);
        let mut s = session();
        assert!(
            s.execute("a.setAttribute('src','clip.wav');throw 'stop';")
                .is_err()
        );
        assert!(duration(&mut s).is_nan());
        assert!(s.set_audio_metadata(vec![asset()]).is_err());
    }
    #[test]
    fn seed_is_once_before_execution_and_file_only() {
        let mut s = DocumentSession::new(web_html::parse("<audio id=a src=clip.wav preload=metadata></audio><script>var d=document.getElementById('a').duration;</script>").unwrap());
        s.set_document_url("file:///audio.html").unwrap();
        s.set_audio_metadata(vec![asset()]).unwrap();
        s.run_initial(&[]);
        assert_eq!(s.execute("d;").unwrap(), Value::Number(1.5));
        assert!(s.set_audio_metadata(vec![]).is_err());
        let mut s = DocumentSession::new(Document::default());
        s.execute("1;").unwrap();
        assert!(s.set_audio_metadata(vec![]).is_err());
        let mut s = DocumentSession::new(Document::default());
        s.set_document_url("https://example.test/").unwrap();
        assert!(s.set_audio_metadata(vec![asset()]).is_err());
    }
    #[test]
    fn invalid_node_ids_are_unavailable_and_cannot_grow_the_map() {
        let mut s = session();
        let count = s.state.media.nodes.len();
        let mut host = DomHost {
            document: &mut s.document,
            state: &mut s.state,
        };
        assert!(
            host.audio_duration(usize::MAX)
                .unwrap()
                .as_number()
                .unwrap()
                .is_nan()
        );
        assert_eq!(host.state.media.nodes.len(), count);
    }
}
