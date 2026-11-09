use browser_ipc::{DisplayItem, RenderedPage};
use std::collections::HashSet;

#[derive(Default)]
pub struct Matches {
    pub query: String,
    pub items: HashSet<usize>,
    pub count: usize,
}

impl Matches {
    pub fn update(&mut self, page: &RenderedPage, query: &str) {
        let query = query
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase();
        if self.query == query {
            return;
        }
        self.query = query;
        self.items.clear();
        self.count = 0;
        if self.query.is_empty() {
            return;
        }
        let mut text = String::new();
        let mut spans = Vec::new();
        for (i, item) in page.items.iter().enumerate() {
            if let DisplayItem::Text { text: fragment, .. } = item {
                if !text.is_empty() {
                    text.push(' ');
                }
                let start = text.len();
                text.push_str(&fragment.to_lowercase());
                spans.push((i, start, text.len()));
            }
        }
        let hits: Vec<_> = text
            .match_indices(&self.query)
            .map(|(start, _)| (start, start + self.query.len()))
            .collect();
        self.count = hits.len();
        let mut cursor = 0;
        for (i, start, end) in spans {
            while cursor < hits.len() && hits[cursor].1 <= start {
                cursor += 1;
            }
            if cursor < hits.len() && hits[cursor].0 < end {
                self.items.insert(i);
            }
        }
    }

    pub fn invalidate(&mut self) {
        self.query = "\0".into();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_phrases_match_across_layout_fragments() {
        let items = ["Uma", "AÇÃO", "local", "e", "ação", "local"]
            .into_iter()
            .map(|text| DisplayItem::Text {
                rect: Default::default(),
                text: text.into(),
                size: 16.,
                color: Default::default(),
                bold: false,
            })
            .collect();
        let page = RenderedPage {
            next_timer_ms: None,
            animation_frame_pending: false,
            title: String::new(),
            width: 100.,
            height: 100.,
            background: Default::default(),
            items,
            links: vec![],
            metrics: Default::default(),
            warnings: vec![],
        };
        let mut found = Matches::default();
        found.update(&page, "  ação   local ");
        assert_eq!(found.count, 2);
        assert_eq!(found.items, HashSet::from([1, 2, 4, 5]));
        found.update(&page, "");
        assert_eq!(found.count, 0);
        assert!(found.items.is_empty());
    }
}
