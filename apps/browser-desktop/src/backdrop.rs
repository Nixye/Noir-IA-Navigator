//! Native compositor effect. Page pixels never control window transparency.
#[derive(Default)]
pub struct Backdrop {
    applied: Option<(bool, u8)>,
}

impl Backdrop {
    /// Apply only after a preference changes; repeatedly calling the OS API while
    /// dragging or painting can cause expensive compositor reconfiguration.
    pub fn update(&mut self, frame: &eframe::Frame, enabled: bool, opacity: u8) -> Option<String> {
        let opacity = opacity.clamp(30, 255);
        if self.applied == Some((enabled, opacity)) {
            return None;
        }
        self.applied = Some((enabled, opacity));
        apply(frame, enabled, opacity).err()
    }
}

#[cfg(target_os = "windows")]
fn apply(frame: &eframe::Frame, enabled: bool, opacity: u8) -> Result<(), String> {
    let result = if enabled {
        window_vibrancy::apply_acrylic(frame, Some((17, 23, 24, opacity)))
    } else {
        window_vibrancy::clear_acrylic(frame)
    };
    result.map_err(|error| format!("O acrílico do sistema não está disponível: {error}"))
}

#[cfg(not(target_os = "windows"))]
fn apply(_frame: &eframe::Frame, enabled: bool, _opacity: u8) -> Result<(), String> {
    if enabled {
        Err("O acrílico nativo está disponível nesta versão apenas no Windows.".into())
    } else {
        Ok(())
    }
}
