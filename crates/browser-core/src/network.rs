//! Privileged HTTP broker. The web engine only receives bounded bytes; URLs,
//! DNS, redirects and subordinate resource authority are checked here.
use super::http_metadata::HttpMetadata;
use browser_ipc::{DocumentInput, MAX_HTML_BYTES, MAX_IMAGE_BYTES, MAX_IMAGE_TOTAL_BYTES};
use reqwest::{
    blocking::Client,
    header::{CONTENT_TYPE, LOCATION},
    redirect::Policy,
};
use std::{
    io::Read,
    net::{IpAddr, ToSocketAddrs},
    time::{Duration, Instant},
};
use url::Url;

const REDIRECT_LIMIT: usize = 5;
const DOCUMENT_DEADLINE: Duration = Duration::from_secs(25);

pub(super) fn validate_url(url: &Url) -> Result<(), String> {
    if !matches!(url.scheme(), "https" | "http")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.as_str().len() > 8192
    {
        return Err("Somente URLs HTTP/HTTPS públicas sem credenciais são permitidas".into());
    }
    if url.port().is_some_and(|p| p != 80 && p != 443) {
        return Err("Esta versão permite somente portas Web 80 e 443".into());
    }
    if let Some(host) = url.host_str() {
        let host = host.trim_matches(['[', ']']);
        if host.eq_ignore_ascii_case("localhost")
            || host.ends_with(".localhost")
            || host.ends_with(".local")
            || host.ends_with(".internal")
        {
            return Err("Navegação não pode acessar serviços locais ou redes privadas".into());
        }
        if let Ok(ip) = host.parse::<IpAddr>()
            && !public_ip(ip)
        {
            return Err("Endereço de rede privado ou reservado não permitido".into());
        }
    }
    Ok(())
}

/// Cross-origin page subresources are public, credential-free data. Each
/// request/redirect receives the same DNS pinning and private-network checks.
fn validate_subresource(document: &Url, resource: &Url) -> Result<(), String> {
    validate_url(document)?;
    validate_url(resource)?;
    if document.scheme() == "https" && resource.scheme() != "https" {
        return Err("Recurso de uma página HTTPS não pode usar HTTP".into());
    }
    Ok(())
}

fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, _, _] = ip.octets();
            !ip.is_private()
                && !ip.is_loopback()
                && !ip.is_link_local()
                && !ip.is_unspecified()
                && !ip.is_broadcast()
                && !ip.is_documentation()
                && a != 0
                && a < 224
                && !(a == 100 && (64..=127).contains(&b))
                && !(a == 192 && b == 0)
                && !(a == 198 && (18..=19).contains(&b))
        }
        IpAddr::V6(ip) => {
            if let Some(mapped) = ip.to_ipv4_mapped() {
                return public_ip(IpAddr::V4(mapped));
            }
            let s = ip.segments();
            (s[0] & 0xe000) == 0x2000
                && s[0] != 0x2002 // 6to4 embeds IPv4 destinations; do not tunnel around filtering.
                && !(s[0] == 0x2001 && s[1] == 0xdb8)
                && !(s[0] == 0x2001 && s[1] < 0x200)
        }
    }
}

/// Pin the already-validated DNS result in the transport. Rebinding cannot
/// change a validated public address into a private connection inside reqwest.
fn client_for(url: &Url, remaining: Duration) -> Result<Client, String> {
    let lookup_started = Instant::now();
    validate_url(url)?;
    let host = url
        .host_str()
        .ok_or("URL sem host")?
        .trim_matches(['[', ']']);
    let port = url.port_or_known_default().ok_or("Porta inválida")?;
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let dns_host = host.to_owned();
    std::thread::spawn(move || {
        let result = (dns_host.as_str(), port)
            .to_socket_addrs()
            .map(|addresses| addresses.take(32).collect::<Vec<_>>())
            .map_err(|e| format!("DNS: {e}"));
        let _ = sender.send(result);
    });
    let addresses = receiver
        .recv_timeout(remaining.min(Duration::from_secs(4)))
        .map_err(|_| "DNS excedeu o prazo de 4 segundos")??;
    if addresses.is_empty() || addresses.iter().any(|a| !public_ip(a.ip())) {
        return Err("DNS resolveu para uma rede privada, reservada ou vazia".into());
    }
    Client::builder()
        .redirect(Policy::none())
        .no_proxy()
        .referer(false)
        .user_agent(concat!(
            "Noir/",
            env!("CARGO_PKG_VERSION"),
            " (Independent Web Engine)"
        ))
        .connect_timeout(Duration::from_secs(5))
        .timeout(
            remaining
                .checked_sub(lookup_started.elapsed())
                .ok_or("DNS consumiu o prazo da requisição")?
                .min(Duration::from_secs(12)),
        )
        .resolve_to_addrs(host, &addresses)
        .build()
        .map_err(|e| format!("HTTP: {e}"))
}

struct Resource {
    url: Url,
    bytes: Vec<u8>,
    metadata: HttpMetadata,
    redirected: bool,
}
fn fetch(
    url: Url,
    limit: usize,
    started: Instant,
    document_url: Option<&Url>,
) -> Result<Resource, String> {
    fetch_with_policy(url, limit, started, document_url, false)
}

fn fetch_with_policy(
    mut url: Url,
    limit: usize,
    started: Instant,
    document_url: Option<&Url>,
    module: bool,
) -> Result<Resource, String> {
    for redirect in 0..=REDIRECT_LIMIT {
        let remaining = DOCUMENT_DEADLINE
            .checked_sub(started.elapsed())
            .ok_or("Documento excedeu o prazo de rede")?;
        if let Some(document_url) = document_url {
            validate_subresource(document_url, &url)?;
            if module && document_url.origin() != url.origin() {
                return Err("Modulos exigem a mesma origem do documento".into());
            }
        }
        let response = client_for(&url, remaining)?
            .get(url.clone())
            .header("Accept-Language", "pt-BR,pt;q=0.9,en;q=0.6")
            .send()
            .map_err(|e| {
                format!(
                    "Falha ao carregar {}: {e}",
                    url.host_str().unwrap_or("site")
                )
            })?;
        if response.status().is_redirection() {
            if redirect == REDIRECT_LIMIT {
                return Err("Limite de redirecionamentos excedido".into());
            }
            let location = response
                .headers()
                .get(LOCATION)
                .ok_or("Redirect sem destino")?
                .to_str()
                .map_err(|_| "Redirect inválido")?;
            let next = url.join(location).map_err(|_| "Redirect inválido")?;
            validate_url(&next)?;
            if module && next != url {
                return Err("Redirecionamento de modulo nao suportado".into());
            }
            if url.scheme() == "https" && next.scheme() != "https" {
                return Err("Downgrade HTTPS não permitido".into());
            }
            url = next;
            continue;
        }
        if !response.status().is_success() {
            return Err(format!(
                "O site respondeu HTTP {}",
                response.status().as_u16()
            ));
        }
        if response.content_length().is_some_and(|n| n > limit as u64) {
            return Err(format!("Recurso excede o limite de {limit} bytes"));
        }
        let metadata = HttpMetadata::from_fields(
            response
                .headers()
                .get_all(CONTENT_TYPE)
                .iter()
                .map(|value| value.as_bytes()),
        )?;
        let mut bytes = Vec::new();
        response
            .take(limit as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("Leitura HTTP: {e}"))?;
        if bytes.len() > limit {
            return Err(format!("Recurso excede o limite de {limit} bytes"));
        }
        return Ok(Resource {
            url,
            bytes,
            metadata,
            redirected: redirect != 0,
        });
    }
    Err("Redirect inválido".into())
}

fn decode_text(resource: &Resource) -> String {
    let encoding = resource.metadata.encoding();
    let (text, _, _) = encoding.decode(&resource.bytes);
    text.into_owned()
}

pub(super) fn load(input: &str) -> Result<DocumentInput, String> {
    security_sandbox::ensure_verified(&std::env::current_exe().map_err(|e| e.to_string())?)?;
    let started = Instant::now();
    let resource = fetch(
        Url::parse(input).map_err(|e| e.to_string())?,
        MAX_HTML_BYTES,
        started,
        None,
    )?;
    if !resource.metadata.is_html() {
        return Err("O recurso não é um documento HTML".into());
    }
    let html = decode_text(&resource);
    if html.len() > MAX_HTML_BYTES {
        return Err("HTML decodificado excede 1 MiB".into());
    }
    let dom = web_html::parse(&html)?;

    let mut images = Vec::new();
    let mut scripts = super::module_assets::Scripts::default();
    let mut image_bytes = 0;
    let mut image_attempts = 0;
    let mut stylesheets = Vec::new();
    let mut style_bytes = 0usize;
    let mut style_seen = std::collections::HashSet::new();
    let mut style_attempts = 0;
    let mut seen = std::collections::HashSet::new();
    for (id, element) in dom.elements() {
        if dom.is_inactive_subtree(id) {
            continue;
        }
        if started.elapsed() >= DOCUMENT_DEADLINE {
            break;
        }
        scripts.element(
            &dom,
            id,
            element,
            &resource.url,
            &mut |_src, url, module| {
                let script = fetch_with_policy(
                    url.clone(),
                    super::module_assets::SCRIPT_LIMIT,
                    started,
                    Some(&resource.url),
                    module,
                )
                .ok()?;
                if !script.metadata.is_javascript() {
                    return None;
                }
                Some(super::module_assets::ScriptSource {
                    source: decode_text(&script),
                    module_allowed: !script.redirected
                        && script.url == *url
                        && resource.url.origin() == url.origin(),
                })
            },
        );
        if element.tag == "img" && image_attempts < 12 {
            let Some(src) = element.attr("src") else {
                continue;
            };
            if !seen.insert(src.to_string()) {
                continue;
            }
            image_attempts += 1;
            if src
                .get(..5)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("data:"))
            {
                if let Some(image) = super::image_decoder::decode_data(src)
                    && image_bytes + image.rgba.len() <= MAX_IMAGE_TOTAL_BYTES
                {
                    image_bytes += image.rgba.len();
                    images.push(image);
                }
                continue;
            }
            let Ok(url) = resource.url.join(src) else {
                continue;
            };
            let Ok(image) = fetch(url, MAX_IMAGE_BYTES, started, Some(&resource.url)) else {
                continue;
            };
            if !image.metadata.is_image() {
                continue;
            }
            if let Some(image) = super::decode_image(src, image.bytes) {
                if image_bytes + image.rgba.len() > MAX_IMAGE_TOTAL_BYTES {
                    continue;
                }
                image_bytes += image.rgba.len();
                images.push(image);
            }
        }
        if super::is_stylesheet_link(element) && style_attempts < browser_ipc::MAX_STYLESHEETS {
            style_attempts += 1;
            let Some(href) = element.attr("href") else {
                continue;
            };
            let Ok(url) = resource.url.join(href) else {
                continue;
            };
            if !style_seen.insert(href.to_owned()) {
                continue;
            }
            let Ok(style) = fetch(
                url,
                browser_ipc::MAX_STYLESHEET_BYTES,
                started,
                Some(&resource.url),
            ) else {
                continue;
            };
            if !style.metadata.is_css() {
                continue;
            }
            let css = decode_text(&style);
            if css.len() > browser_ipc::MAX_STYLESHEET_BYTES
                || style_bytes.saturating_add(css.len()) > browser_ipc::MAX_STYLESHEET_TOTAL_BYTES
            {
                continue;
            }
            style_bytes += css.len();
            stylesheets.push(browser_ipc::StylesheetAsset {
                href: href.into(),
                source: css,
            });
        }
    }
    Ok(DocumentInput {
        url: resource.url.into(),
        html,
        images,
        audio_metadata: vec![],
        scripts: scripts.assets,
        stylesheets,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn text_resource(content_type: &str, bytes: Vec<u8>) -> Resource {
        Resource {
            url: Url::parse("https://example.com/").unwrap(),
            bytes,
            metadata: HttpMetadata::from_fields([content_type.as_bytes()]).unwrap(),
            redirected: false,
        }
    }

    #[test]
    fn structured_charset_decoding_preserves_bom_and_utf8_expansion_checks() {
        let windows = text_resource("text/html; charset = \"windows-1252\"", vec![0x80, 0xe9]);
        assert_eq!(decode_text(&windows), "\u{20ac}\u{e9}");
        let unknown = text_resource(
            "text/html; charset=unknown; charset=windows-1252",
            vec![0xc3, 0xa9],
        );
        assert_eq!(decode_text(&unknown), "\u{e9}");
        let bom = text_resource(
            "text/html; charset=windows-1252",
            vec![0xef, 0xbb, 0xbf, 0xc3, 0xa9],
        );
        assert_eq!(decode_text(&bom), "\u{e9}");
        for (mime, limit) in [
            ("text/html", MAX_HTML_BYTES),
            ("text/css", browser_ipc::MAX_STYLESHEET_BYTES),
            ("text/javascript", super::super::module_assets::SCRIPT_LIMIT),
        ] {
            let resource =
                text_resource(&format!("{mime}; charset=windows-1252"), vec![0x80; limit]);
            assert_eq!(resource.bytes.len(), limit);
            // The existing consumers check decoded length after decode; this
            // does not claim a bound on the decoder's transient allocation.
            assert_eq!(decode_text(&resource).len(), limit * 3);
        }
    }

    #[test]
    fn public_cdn_resources_keep_mixed_content_and_private_network_guards() {
        let document = Url::parse("https://example.com/index.html").unwrap();
        assert!(
            validate_subresource(
                &document,
                &Url::parse("https://cdn.example.org/app.css").unwrap()
            )
            .is_ok()
        );
        for destination in [
            "http://cdn.example.org/app.css",
            "https://127.0.0.1/private",
            "https://169.254.169.254/latest",
            "https://[::1]/",
            "https://user:secret@cdn.example.org/x",
            "file:///C:/private",
            "https://cdn.example.org:8080/x",
        ] {
            assert!(
                validate_subresource(&document, &Url::parse(destination).unwrap()).is_err(),
                "{destination}"
            );
        }
    }
    #[test]
    fn private_reserved_and_metadata_addresses_are_denied() {
        for address in [
            "127.0.0.1",
            "10.0.0.1",
            "169.254.169.254",
            "100.64.1.2",
            "192.168.1.1",
            "198.18.0.1",
            "224.0.0.1",
            "::1",
            "fc00::1",
            "fe80::1",
            "2001:db8::1",
            "::ffff:127.0.0.1",
        ] {
            assert!(!public_ip(address.parse().unwrap()), "{address}");
        }
        for address in ["1.1.1.1", "8.8.8.8", "2606:4700:4700::1111"] {
            assert!(public_ip(address.parse().unwrap()), "{address}");
        }
        for address in [
            "file:///C:/private.html",
            "https://127.0.0.1/",
            "http://localhost:80/",
            "https://user:secret@example.com/",
            "https://example.com:8080/",
        ] {
            assert!(
                validate_url(&Url::parse(address).unwrap()).is_err(),
                "{address}"
            );
        }
    }
    #[test]
    fn address_bar_uses_google_and_preserves_explicit_paths() {
        assert_eq!(
            crate::normalize_address("google.com.br").unwrap(),
            "https://google.com.br/"
        );
        let query = Url::parse(&crate::normalize_address("motor próprio Rust").unwrap()).unwrap();
        assert_eq!(query.host_str(), Some("www.google.com.br"));
        assert!(
            query
                .query_pairs()
                .any(|(k, v)| k == "q" && v == "motor próprio Rust")
        );
        assert_eq!(
            crate::normalize_address("fixtures/specimen.html").unwrap(),
            "fixtures/specimen.html"
        );
        assert!(crate::normalize_address("javascript:alert(1)").is_err());
    }
}
