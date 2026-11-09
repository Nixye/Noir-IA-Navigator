#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod ai_panel;
mod backdrop;
mod find;
mod home;
mod icons;
mod page_gpu;
mod page_scene;
mod shell;
mod theme;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "--sandbox-probe") {
        if browser_process::run_sandbox_probe(args.get(1).map(String::as_str).unwrap_or(""))
            .is_err()
        {
            std::process::exit(1);
        }
        return;
    }
    if args.first().is_some_and(|a| a == "--renderer") {
        if browser_process::run_worker().is_err() {
            std::process::exit(1);
        }
        return;
    }
    if args.first().is_some_and(|a| a == "--renderer-session") {
        if browser_process::run_session_worker().is_err() {
            std::process::exit(1);
        }
        return;
    }
    let startup = match Startup::parse(args) {
        Ok(options) => options,
        Err(error) => {
            rfd::MessageDialog::new()
                .set_title("Noir — argumentos inválidos")
                .set_description(error)
                .set_level(rfd::MessageLevel::Error)
                .show();
            return;
        }
    };
    if startup.version {
        println!(
            "Noir {} ({})",
            env!("CARGO_PKG_VERSION"),
            option_env!("NOIR_BUILD_ID").unwrap_or("development")
        );
        return;
    }
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size(startup.size)
            .with_min_inner_size([800.0, 560.0])
            .with_active(startup.capture.is_none())
            .with_transparent(true)
            .with_decorations(false)
            .with_icon(application_icon())
            .with_app_id("org.noir.browser"),
        ..Default::default()
    };
    if let Err(e) = eframe::run_native(
        "Noir",
        options,
        Box::new(move |cc| {
            let mut app = shell::Noir::new(
                cc,
                startup.initial,
                startup.capture,
                startup.temporary_profile,
            );
            if startup.assistant {
                app.open_assistant();
            }
            Ok(Box::new(app))
        }),
    ) {
        rfd::MessageDialog::new()
            .set_title("Noir — falha ao iniciar")
            .set_description(e.to_string())
            .set_level(rfd::MessageLevel::Error)
            .show();
    }
}

fn application_icon() -> std::sync::Arc<eframe::egui::IconData> {
    let icon = image::load_from_memory(include_bytes!("../assets/noir-256.png"))
        .expect("bundled app icon must be a valid PNG")
        .to_rgba8();
    std::sync::Arc::new(eframe::egui::IconData {
        width: icon.width(),
        height: icon.height(),
        rgba: icon.into_raw(),
    })
}

struct Startup {
    initial: Option<String>,
    capture: Option<std::path::PathBuf>,
    size: [f32; 2],
    assistant: bool,
    version: bool,
    temporary_profile: bool,
}

impl Startup {
    fn parse(args: Vec<String>) -> Result<Self, String> {
        let mut options = Self {
            initial: None,
            capture: None,
            size: [1280., 860.],
            assistant: false,
            version: false,
            temporary_profile: false,
        };
        let mut args = args.into_iter();
        while let Some(argument) = args.next() {
            match argument.as_str() {
                "--assistant" => options.assistant = true,
                "--version" | "-V" => options.version = true,
                "--temporary-profile" => options.temporary_profile = true,
                "--capture" => {
                    let path = std::path::PathBuf::from(
                        args.next().ok_or("Informe o caminho PNG após --capture")?,
                    );
                    if !path
                        .extension()
                        .is_some_and(|e| e.eq_ignore_ascii_case("png"))
                    {
                        return Err("A captura deve usar extensão .png".into());
                    }
                    options.capture = Some(path);
                }
                "--window-size" => {
                    let value = args
                        .next()
                        .ok_or("Informe largura x altura, por exemplo 900x700")?;
                    let (width, height) =
                        value.split_once('x').ok_or("Use --window-size 900x700")?;
                    let width: f32 = width.parse().map_err(|_| "Largura inválida")?;
                    let height: f32 = height.parse().map_err(|_| "Altura inválida")?;
                    if !(800.0..=4096.0).contains(&width) || !(560.0..=4096.0).contains(&height) {
                        return Err("Janela deve ter entre 800x560 e 4096x4096 pixels".into());
                    }
                    options.size = [width, height];
                }
                option if option.starts_with("--") => {
                    return Err(format!("Opção desconhecida: {option}"));
                }
                _ => {
                    if options.initial.replace(argument).is_some() {
                        return Err("Informe um único documento inicial".into());
                    }
                }
            }
        }
        Ok(options)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }
    #[test]
    fn diagnostics_parse_without_confusing_document_paths() {
        let options = Startup::parse(args(&[
            "C:/My Documents/page.html",
            "--window-size",
            "900x700",
            "--capture",
            "page.png",
        ]))
        .unwrap();
        assert_eq!(options.size, [900., 700.]);
        assert_eq!(
            options.initial.as_deref(),
            Some("C:/My Documents/page.html")
        );
        assert!(options.capture.is_some());
    }
    #[test]
    fn invalid_or_ambiguous_arguments_are_observable_errors() {
        for invalid in [
            args(&["--capture"]),
            args(&["--window-size", "NaNx700"]),
            args(&["--window-size", "400x400"]),
            args(&["one.html", "two.html"]),
            args(&["--unknown"]),
        ] {
            assert!(Startup::parse(invalid).is_err());
        }
    }
}
