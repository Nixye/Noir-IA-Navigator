use browser_cli::snapshot::{self, Snapshot};
use browser_ipc::Viewport;
use std::{env, fs, path::PathBuf, process::ExitCode};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "--renderer-session") {
        return match browser_process::run_session_worker() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("{e}");
                ExitCode::FAILURE
            }
        };
    }
    if args.first().is_some_and(|a| a == "--sandbox-probe") {
        return match browser_process::run_sandbox_probe(
            args.get(1).map(String::as_str).unwrap_or(""),
        ) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("{e}");
                ExitCode::FAILURE
            }
        };
    }
    let result = if args.first().is_some_and(|a| a == "--renderer") {
        browser_process::run_worker()
    } else {
        run(&args)
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Noir: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<(), String> {
    if args
        .first()
        .is_some_and(|arg| matches!(arg.as_str(), "--version" | "-V"))
    {
        println!(
            "Noir CLI {} ({})",
            env!("CARGO_PKG_VERSION"),
            option_env!("NOIR_BUILD_ID").unwrap_or("development")
        );
        return Ok(());
    }
    if args.is_empty() || args[0] == "--help" || args[0] == "-h" {
        println!(
            "Noir - motor Web próprio\n\n  browser-cli render <arquivo|URL|consulta|noir:welcome|noir:specimen>\n      [--width 1024] [--height 768] [--output page.json] [--ppm page.ppm]\n  browser-cli capture <arquivo|URL|noir:welcome|noir:specimen>\n      --snapshot captura.noir-snapshot [--width 1024] [--height 768]\n  browser-cli replay captura.noir-snapshot [--output page.json] [--ppm page.ppm]\n\nHTML local e HTTP/HTTPS com sandbox verificada no Windows. Consultas usam Google.\nSubconjunto JavaScript próprio; compatibilidade com aplicações Web modernas é parcial.\nCapture é explícito, usa somente o broker e não executa scripts. Snapshots podem conter dados privados.\nReplay não busca recursos, preserva URL/viewport e exige AppContainer para HTTP/HTTPS.\nStorage é indisponível/vazio; nenhum perfil ou cookie é exportado. Arquivos diagnósticos não são sobrescritos.\nO CLI executa uma renderização descartável; eventos e timers retidos são entregues pelo desktop."
        );
        return Ok(());
    }
    if !matches!(args[0].as_str(), "render" | "capture" | "replay") {
        return Err("Comando desconhecido; use --help".into());
    }
    let capture = args[0] == "capture";
    let replay = args[0] == "replay";
    let target = args.get(1).ok_or("Informe o documento; use --help")?;
    let mut viewport = Viewport::default();
    let mut output: Option<PathBuf> = None;
    let mut ppm: Option<PathBuf> = None;
    let mut snapshot_path: Option<PathBuf> = None;
    let mut i = 2;
    while i < args.len() {
        let value = args
            .get(i + 1)
            .ok_or_else(|| format!("Valor ausente para {}", args[i]))?;
        match args[i].as_str() {
            "--width" if !replay => {
                viewport.width = value.parse().map_err(|_| "Largura inválida")?
            }
            "--height" if !replay => {
                viewport.height = value.parse().map_err(|_| "Altura inválida")?
            }
            "--output" if !capture => output = Some(value.into()),
            "--ppm" if !capture => ppm = Some(value.into()),
            "--snapshot" if capture && snapshot_path.is_none() => {
                snapshot_path = Some(value.into())
            }
            x => return Err(format!("Opção desconhecida: {x}")),
        }
        i += 2;
    }
    if !viewport.width.is_finite()
        || !viewport.height.is_finite()
        || !(64.0..=8192.0).contains(&viewport.width)
        || !(64.0..=8192.0).contains(&viewport.height)
    {
        return Err("Viewport deve estar entre 64 e 8192 pixels".into());
    }
    if capture {
        let path = snapshot_path.ok_or("Capture exige --snapshot; use --help")?;
        // Avoid a broker request when a destination is already known to exist.
        // save_new still performs the atomic, race-safe existence check.
        if path.try_exists().map_err(|e| e.to_string())? {
            return Err("O arquivo de snapshot já existe; escolha outro nome".into());
        }
        let document = browser_core::load_document(target)?;
        snapshot::save_new(&path, &Snapshot { document, viewport })?;
        eprintln!("Snapshot salvo. Pode conter dados privados; revise antes de compartilhar.");
        return Ok(());
    }
    let document = if replay {
        let snapshot = snapshot::load(std::path::Path::new(target))?;
        viewport = snapshot.viewport;
        snapshot.document
    } else {
        browser_core::load_document(target)?
    };
    let images = document.images.clone();
    let page = browser_process::render_in_subprocess(
        &env::current_exe().map_err(|e| e.to_string())?,
        document,
        viewport,
    )?;
    if let Some(path) = ppm {
        let surface = web_paint::rasterize(
            &page,
            &images,
            viewport.width as u32,
            viewport.height as u32,
        )?;
        surface
            .write_ppm(&mut output_file(&path, replay)?)
            .map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(&page).map_err(|e| e.to_string())?;
    if let Some(path) = output {
        use std::io::Write;
        output_file(&path, replay)?
            .write_all(json.as_bytes())
            .map_err(|e| e.to_string())?;
    } else {
        println!("{json}");
    }
    Ok(())
}

fn output_file(path: &std::path::Path, exclusive: bool) -> Result<fs::File, String> {
    if exclusive {
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
    } else {
        fs::File::create(path)
    }
    .map_err(|e| format!("Arquivo de saída: {e}"))
}
