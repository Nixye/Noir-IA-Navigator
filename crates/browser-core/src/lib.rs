//! Privileged document broker. Renderers receive bounded bytes, never host file
//! handles or network authority. HTTP/HTTPS requires a verified OS sandbox;
//! local page-generated navigation remains confined to its authorized directory.
use browser_ipc::{
    DocumentInput, ImageAsset, MAX_HTML_BYTES, MAX_IMAGE_BYTES, MAX_IMAGE_TOTAL_BYTES,
};
use std::collections::HashSet;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use url::Url;
use web_dom::NodeKind;

mod http_metadata;
mod image_decoder;
mod media_metadata;
mod module_assets;
mod network;

/// Address-bar normalization. Search terms use Google; explicit local paths
/// remain capabilities granted by the user rather than page-controlled links.
pub fn normalize_address(input: &str) -> Result<String, String> {
    let input = input.trim();
    check_input(input)?;
    if builtin(input).is_some() {
        return Ok(input.into());
    }
    if input.starts_with("noir:") {
        return Err("Página interna desconhecida".into());
    }
    if Path::new(input).is_absolute()
        || input.starts_with("file:")
        || input.starts_with("./")
        || input.starts_with("../")
        || input.ends_with(".html")
        || input.ends_with(".htm")
    {
        local_path(input)?;
        return Ok(input.into());
    }
    if let Ok(url) = Url::parse(input) {
        network::validate_url(&url)?;
        return Ok(url.to_string());
    }
    if !input.chars().any(char::is_whitespace)
        && (input.contains('.') || input.starts_with("localhost"))
    {
        let url = Url::parse(&format!("https://{input}")).map_err(|e| e.to_string())?;
        network::validate_url(&url)?;
        return Ok(url.to_string());
    }
    let mut url = Url::parse("https://www.google.com.br/search").map_err(|e| e.to_string())?;
    url.query_pairs_mut()
        .append_pair("q", input)
        .append_pair("hl", "pt-BR");
    Ok(url.to_string())
}

const MAX_IMAGE_COUNT: usize = 32;
const MAX_INPUT_LENGTH: usize = 8192;

pub fn load_document(input: &str) -> Result<DocumentInput, String> {
    let normalized = normalize_address(input)?;
    let input = normalized.as_str();
    if input.starts_with("https://") || input.starts_with("http://") {
        return network::load(input);
    }
    if let Some(html) = builtin(input) {
        let images = if input == "noir:specimen" {
            decode_image(
                "palette.png",
                include_bytes!("../../../fixtures/palette.png").to_vec(),
            )
            .into_iter()
            .collect()
        } else {
            vec![]
        };
        return Ok(DocumentInput {
            url: input.to_owned(),
            html: html.to_owned(),
            images,
            audio_metadata: vec![],
            scripts: vec![],
            stylesheets: vec![],
        });
    }
    let path = local_path(input)?;
    let path = path
        .canonicalize()
        .map_err(|e| format!("Não foi possível abrir o documento: {e}"))?;
    require_html(&path)?;
    let bytes = read_bounded(&path, MAX_HTML_BYTES)?;
    let html = String::from_utf8(bytes)
        .map_err(|_| "O documento deve usar codificação UTF-8.".to_owned())?;
    let html = html.strip_prefix('\u{feff}').unwrap_or(&html).to_owned();
    let base = path.parent().ok_or("Documento sem diretório pai.")?;
    let url = Url::from_file_path(&path)
        .map_err(|_| "Caminho local inválido.")?
        .to_string();
    let images = load_images(&html, base, &url);
    let audio_metadata = media_metadata::load(&html, base, &url);
    let (scripts, stylesheets) = load_text_assets(&html, base, &url);
    Ok(DocumentInput {
        url,
        html,
        images,
        audio_metadata,
        scripts,
        stylesheets,
    })
}

/// Resolves a page-controlled link. Unlike an address typed by the user, the link
/// cannot grant itself authority to a directory outside the document's directory.
pub fn resolve_navigation(base: &str, target: &str) -> Result<String, String> {
    let target = target.trim();
    check_input(target)?;
    if builtin(target).is_some() {
        return Ok(target.to_owned());
    }
    let base_url = Url::parse(base).map_err(|_| "Origem de navegação inválida.")?;
    if matches!(base_url.scheme(), "http" | "https") {
        let next = base_url
            .join(target)
            .map_err(|_| "Destino de navegação inválido")?;
        network::validate_url(&next)?;
        if base_url.scheme() == "https" && next.scheme() != "https" {
            return Err("Navegação HTTPS não pode fazer downgrade para HTTP".into());
        }
        return Ok(next.into());
    }
    if base_url.scheme() != "file" {
        return Err("Esta página só pode abrir documentos internos do Noir.".to_owned());
    }
    let next = base_url
        .join(target)
        .map_err(|_| "Destino de navegação inválido.")?;
    let next_path = local_path(next.as_str())?;
    let base_path = local_path(base)?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let root = base_path.parent().ok_or("Origem local inválida.")?;
    let canonical = next_path
        .canonicalize()
        .map_err(|e| format!("Documento não encontrado: {e}"))?;
    if !canonical.starts_with(root) {
        return Err("O link tenta sair do diretório autorizado para esta página.".to_owned());
    }
    require_html(&canonical)?;
    Ok(Url::from_file_path(canonical)
        .map_err(|_| "Caminho inválido.")?
        .to_string())
}

fn builtin(input: &str) -> Option<&'static str> {
    match input {
        "noir:welcome" => Some(include_str!("../../../fixtures/welcome.html")),
        "noir:specimen" => Some(include_str!("../../../fixtures/specimen.html")),
        _ => None,
    }
}

fn check_input(input: &str) -> Result<(), String> {
    if input.is_empty() || input.len() > MAX_INPUT_LENGTH || input.chars().any(char::is_control) {
        return Err("Endereço vazio, muito longo ou com caracteres de controle.".to_owned());
    }
    // UNC, Win32 device paths and network shares must not trigger filesystem I/O.
    if input.starts_with("\\\\") || input.starts_with("//") {
        return Err(
            "Compartilhamentos de rede e caminhos de dispositivo não são permitidos.".to_owned(),
        );
    }
    Ok(())
}

fn local_path(input: &str) -> Result<PathBuf, String> {
    check_input(input)?;
    let path = Path::new(input);
    if path.is_absolute() && !input.starts_with("file:") {
        return Ok(path.to_path_buf());
    }
    match Url::parse(input) {
        Ok(url) => {
            if url.scheme() != "file" {
                return Err("A navegação de rede está desativada. Abra um arquivo HTML local ou noir:welcome.".to_owned());
            }
            if url.host_str().is_some()
                || url.query().is_some()
                || !url.username().is_empty()
                || url.password().is_some()
            {
                return Err(
                    "Somente URLs file locais, sem host ou consulta, são permitidas.".to_owned(),
                );
            }
            let result = url.to_file_path().map_err(|_| "URL file inválida.")?;
            check_input(&result.to_string_lossy())?;
            Ok(result)
        }
        Err(_) => Ok(path.to_path_buf()),
    }
}

fn require_html(path: &Path) -> Result<(), String> {
    if !path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("html") || e.eq_ignore_ascii_case("htm"))
    {
        return Err("Selecione um documento .html ou .htm.".to_owned());
    }
    if !path.is_file() {
        return Err("O endereço deve indicar um arquivo HTML regular.".to_owned());
    }
    Ok(())
}

fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let file = File::open(path).map_err(|e| format!("Falha ao ler arquivo: {e}"))?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("O recurso não é um arquivo regular.".to_owned());
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > limit {
        return Err(format!("Arquivo excede o limite de {limit} bytes."));
    }
    Ok(bytes)
}

fn load_images(html: &str, root: &Path, base: &str) -> Vec<ImageAsset> {
    let Ok(document) = web_html::parse(html) else {
        return vec![];
    };
    let Ok(base) = Url::parse(base) else {
        return vec![];
    };
    let mut seen = HashSet::new();
    let mut assets = Vec::new();
    let mut total = 0;
    for (id, node) in document.nodes.iter().enumerate() {
        if document.is_inactive_subtree(id) {
            continue;
        }
        let NodeKind::Element(element) = &node.kind else {
            continue;
        };
        if element.tag != "img" {
            continue;
        }
        let Some(src) = element.attributes.get("src") else {
            continue;
        };
        if !seen.insert(src.clone()) {
            continue;
        }
        // Bound attempted opens as well as successful images, so thousands of
        // missing resources cannot turn one small document into an I/O storm.
        if seen.len() > MAX_IMAGE_COUNT {
            break;
        }
        let Some(asset) = load_image(src, root, &base) else {
            continue;
        };
        if total + asset.rgba.len() > MAX_IMAGE_TOTAL_BYTES {
            continue;
        }
        total += asset.rgba.len();
        assets.push(asset);
    }
    assets
}

fn is_stylesheet_link(element: &web_dom::Element) -> bool {
    element.tag == "link"
        && element.attr("rel").is_some_and(|rel| {
            rel.split_ascii_whitespace()
                .any(|token| token.eq_ignore_ascii_case("stylesheet"))
                && !rel
                    .split_ascii_whitespace()
                    .any(|token| token.eq_ignore_ascii_case("alternate"))
        })
        && !element.attributes.contains_key("disabled")
        && element.attr("type").is_none_or(|kind| {
            kind.trim().is_empty() || kind.trim().eq_ignore_ascii_case("text/css")
        })
}
fn is_classic_script(element: &web_dom::Element) -> bool {
    element.tag == "script"
        && matches!(
            element
                .attr("type")
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase()
                .as_str(),
            "" | "text/javascript"
                | "application/javascript"
                | "application/x-javascript"
                | "text/ecmascript"
                | "application/ecmascript"
        )
}
fn local_resource(
    src: &str,
    root: &Path,
    base: &Url,
    limit: usize,
    extension: &str,
) -> Option<Vec<u8>> {
    check_input(src).ok()?;
    if Path::new(src).is_absolute() || Url::parse(src).is_ok() {
        return None;
    }
    let resolved = base.join(src).ok()?;
    let path = local_path(resolved.as_str()).ok()?.canonicalize().ok()?;
    if !path.starts_with(root)
        || !path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case(extension))
    {
        return None;
    }
    read_bounded(&path, limit).ok()
}
fn load_text_assets(
    html: &str,
    root: &Path,
    base: &str,
) -> (
    Vec<browser_ipc::ScriptAsset>,
    Vec<browser_ipc::StylesheetAsset>,
) {
    let mut scripts = module_assets::Scripts::default();
    let mut stylesheets = Vec::new();
    let (Ok(document), Ok(base)) = (web_html::parse(html), Url::parse(base)) else {
        return (scripts.assets, stylesheets);
    };
    let mut style_attempts = 0;
    let mut style_bytes = 0usize;
    let mut seen_styles = HashSet::new();
    for (id, element) in document.elements() {
        if document.is_inactive_subtree(id) {
            continue;
        }
        scripts.element(&document, id, element, &base, &mut |src, url, module| {
            let bytes = if module {
                check_input(url.as_str()).ok()?;
                let path = local_path(url.as_str()).ok()?.canonicalize().ok()?;
                if !path.starts_with(root)
                    || !path.extension().is_some_and(|ext| {
                        ext.eq_ignore_ascii_case("js") || ext.eq_ignore_ascii_case("mjs")
                    })
                {
                    return None;
                }
                read_bounded(&path, module_assets::SCRIPT_LIMIT).ok()?
            } else {
                local_resource(src, root, &base, module_assets::SCRIPT_LIMIT, "js")?
            };
            let source = String::from_utf8(bytes).ok()?;
            Some(module_assets::ScriptSource {
                source: source
                    .strip_prefix('\u{feff}')
                    .unwrap_or(&source)
                    .to_owned(),
                module_allowed: true,
            })
        });
        if is_stylesheet_link(element)
            && style_attempts < browser_ipc::MAX_STYLESHEETS
            && let Some(href) = element.attr("href")
            && seen_styles.insert(href.to_owned())
        {
            style_attempts += 1;
            let Some(bytes) =
                local_resource(href, root, &base, browser_ipc::MAX_STYLESHEET_BYTES, "css")
            else {
                continue;
            };
            let Ok(source) = String::from_utf8(bytes) else {
                continue;
            };
            let source = source
                .strip_prefix('\u{feff}')
                .unwrap_or(&source)
                .to_owned();
            if style_bytes.saturating_add(source.len()) > browser_ipc::MAX_STYLESHEET_TOTAL_BYTES {
                continue;
            }
            style_bytes += source.len();
            stylesheets.push(browser_ipc::StylesheetAsset {
                href: href.into(),
                source,
            });
        }
    }
    (scripts.assets, stylesheets)
}

fn load_image(src: &str, root: &Path, base: &Url) -> Option<ImageAsset> {
    if src
        .get(..5)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("data:"))
    {
        return image_decoder::decode_data(src);
    }
    check_input(src).ok()?;
    if Path::new(src).is_absolute() || Url::parse(src).is_ok() {
        return None;
    }
    let resolved = base.join(src).ok()?;
    let path = local_path(resolved.as_str()).ok()?.canonicalize().ok()?;
    if !path.starts_with(root) {
        return None;
    }
    let encoded = read_bounded(&path, MAX_IMAGE_BYTES).ok()?;
    decode_image(src, encoded)
}

pub(crate) fn decode_image(src: &str, encoded: Vec<u8>) -> Option<ImageAsset> {
    image_decoder::decode(src, encoded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[test]
    fn builtin_documents_do_not_acquire_audio_metadata() {
        for input in ["noir:welcome", "noir:specimen"] {
            assert!(load_document(input).unwrap().audio_metadata.is_empty());
        }
    }
    #[test]
    fn inactive_local_assets_never_become_renderer_capabilities() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["active", "inactive"] {
            fs::write(
                dir.path().join(format!("{name}.css")),
                format!("p{{color:{name}}}"),
            )
            .unwrap();
            fs::write(
                dir.path().join(format!("{name}.js")),
                format!("document.title='{name}';"),
            )
            .unwrap();
            image::RgbaImage::from_pixel(1, 1, image::Rgba([1u8, 2, 3, 255]))
                .save(dir.path().join(format!("{name}.png")))
                .unwrap();
        }
        let file = dir.path().join("page.html");
        for (prefix, suffix) in [
            ("<template>", "</template>"),
            ("<noscript>", "</noscript>"),
            ("<body><template></body></html>", "</template>"),
            ("<body><noscript></body></html>", "</noscript>"),
            (
                "<body><select><option><template><option>",
                "</template></select>",
            ),
            (
                "<body><select><optgroup><template><optgroup>",
                "</template></select>",
            ),
        ] {
            let html = format!(
                "{prefix}<link rel=stylesheet href=inactive.css><script src=inactive.js></script><img src=inactive.png>{suffix}<link rel=stylesheet href=active.css><script src=active.js></script><img src=active.png>"
            );
            fs::write(&file, html).unwrap();
            let input = load_document(file.to_str().unwrap()).unwrap();
            assert_eq!(input.images.len(), 1, "{prefix}");
            assert_eq!(input.images[0].id, "active.png", "{prefix}");
            assert_eq!(input.stylesheets.len(), 1, "{prefix}");
            assert_eq!(input.stylesheets[0].href, "active.css", "{prefix}");
            assert_eq!(input.scripts.len(), 1, "{prefix}");
            assert_eq!(input.scripts[0].src, "active.js", "{prefix}");
        }
    }

    #[test]
    fn rejects_unsupported_schemes_network_authority_and_non_html() {
        for input in [
            "https://127.0.0.1",
            "file://server/share/a.html",
            "\\\\server\\share\\a.html",
            "//server/a.html",
            "javascript:alert(1)",
            "data:text/html,test",
        ] {
            assert!(load_document(input).is_err(), "{input}");
        }
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("private.txt");
        fs::write(&file, "secret").unwrap();
        assert!(load_document(file.to_str().unwrap()).is_err());
    }

    #[test]
    fn navigation_is_confined_and_html_is_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let allowed = dir.path().join("allowed");
        fs::create_dir(&allowed).unwrap();
        let first = allowed.join("first.html");
        let second = allowed.join("second.html");
        fs::write(&first, "<p>Olá</p>").unwrap();
        fs::write(&second, "<p>Segundo</p>").unwrap();
        fs::write(dir.path().join("secret.html"), "secret").unwrap();
        let loaded = load_document(first.to_str().unwrap()).unwrap();
        assert!(resolve_navigation(&loaded.url, "second.html").is_ok());
        assert!(resolve_navigation(&loaded.url, "../secret.html").is_err());
        assert!(resolve_navigation(&loaded.url, "https://example.com").is_err());
        fs::write(&first, vec![b'x'; MAX_HTML_BYTES + 1]).unwrap();
        assert!(load_document(first.to_str().unwrap()).is_err());
    }

    #[test]
    fn image_broker_loads_only_confined_png_and_jpeg() {
        let dir = tempfile::tempdir().unwrap();
        let allowed = dir.path().join("allowed");
        fs::create_dir(&allowed).unwrap();
        let image = image::RgbaImage::from_pixel(2, 2, image::Rgba([1, 2, 3, 255]));
        image.save(allowed.join("inside.png")).unwrap();
        image.save(dir.path().join("outside.png")).unwrap();
        let file = allowed.join("image.html");
        fs::write(&file, "<img src='inside.png'><img src='../outside.png'><img src='https://example.com/a.png'><img src='inside.png'>").unwrap();
        let doc = load_document(file.to_str().unwrap()).unwrap();
        assert_eq!(doc.images.len(), 1);
        assert_eq!(doc.images[0].id, "inside.png");
        assert_eq!(doc.images[0].rgba.len(), 16);
    }

    #[test]
    fn builtin_pages_are_brokered_without_filesystem() {
        assert!(load_document("noir:welcome").unwrap().html.contains("Noir"));
        assert_eq!(load_document("noir:specimen").unwrap().images.len(), 1);
        assert!(load_document("noir:private").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn canonicalization_blocks_image_and_navigation_symlink_escape() {
        let dir = tempfile::tempdir().unwrap();
        let allowed = dir.path().join("allowed");
        fs::create_dir(&allowed).unwrap();
        fs::write(dir.path().join("secret.html"), "<p>Private</p>").unwrap();
        std::os::unix::fs::symlink(dir.path().join("secret.html"), allowed.join("escape.html"))
            .unwrap();
        let image = image::RgbaImage::from_pixel(2, 2, image::Rgba([1, 2, 3, 255]));
        image.save(dir.path().join("private.png")).unwrap();
        std::os::unix::fs::symlink(dir.path().join("private.png"), allowed.join("escape.png"))
            .unwrap();
        let file = allowed.join("first.html");
        fs::write(&file, "<img src='escape.png'>").unwrap();
        let document = load_document(file.to_str().unwrap()).unwrap();
        assert!(document.images.is_empty());
        assert!(resolve_navigation(&document.url, "escape.html").is_err());
    }
}
