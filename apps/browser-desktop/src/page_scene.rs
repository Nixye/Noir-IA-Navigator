//! One active page owns the retained CPU scene; inactive tabs do not each keep
//! an atlas and vertex allocation. Zoom/DPI and committed document generations
//! replace the scene without accepting results from stale navigations.
use browser_ipc::{ImageAsset, RenderedPage};
use std::sync::Arc;
use web_graphics::Scene;

#[derive(Default)]
pub struct Cache {
    key: Option<(u64, u64, u32)>,
    scene: Option<Result<Arc<Scene>, String>>,
}
impl Cache {
    pub fn clear(&mut self) {
        self.key = None;
        self.scene = None;
    }
    pub fn prepare(
        &mut self,
        tab: u64,
        generation: u64,
        scale: f32,
        page: &RenderedPage,
        images: &[ImageAsset],
    ) -> Result<Arc<Scene>, String> {
        let key = (tab, generation, scale.to_bits());
        if self.key != Some(key) {
            self.scene = Some(Scene::prepare(page, images, scale).map(Arc::new));
            self.key = Some(key);
        }
        self.scene
            .as_ref()
            .expect("scene is set with its key")
            .clone()
    }
}
