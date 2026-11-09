//! Bounded, conservative Content-Type parsing for the privileged HTTP broker.
//! No sniffing, CORS or complete nosniff implementation is implied.
use std::borrow::Cow;

const MAX_CONTENT_TYPE_BYTES: usize = 4096;
const MAX_PARAMETERS: usize = 32;

#[derive(Debug)]
pub(super) struct HttpMetadata {
    essence: String,
    charset: Option<Vec<u8>>,
}

impl HttpMetadata {
    /// Reject all repeated fields, even identical ones: Content-Type is not a
    /// list. The limit precedes our parsing/copying, not HTTP-client allocation.
    pub(super) fn from_fields<'a>(
        fields: impl IntoIterator<Item = &'a [u8]>,
    ) -> Result<Self, &'static str> {
        let mut fields = fields.into_iter();
        let field = fields.next().ok_or("Content-Type ausente")?;
        if field.len() > MAX_CONTENT_TYPE_BYTES {
            return Err("Content-Type excede 4096 bytes");
        }
        if fields.next().is_some() {
            return Err("Content-Type repetido nao permitido");
        }
        Self::parse(field)
    }

    fn parse(field: &[u8]) -> Result<Self, &'static str> {
        let mut scanner = Scanner {
            bytes: field,
            pos: 0,
        };
        scanner.ows();
        let start = scanner.pos;
        scanner.token()?;
        scanner.expect(b'/')?;
        scanner.token()?;
        let essence = std::str::from_utf8(&field[start..scanner.pos])
            .map_err(|_| "Content-Type invalido")?
            .to_ascii_lowercase();
        let mut charset = None;
        let mut count = 0;
        scanner.ows();
        while scanner.pos < field.len() {
            scanner.expect(b';')?;
            count += 1;
            if count > MAX_PARAMETERS {
                return Err("Content-Type excede 32 parametros");
            }
            scanner.ows();
            let name = scanner.token()?;
            scanner.ows();
            scanner.expect(b'=')?;
            scanner.ows();
            let value = scanner.value()?;
            // First usable ASCII token label wins, even if encoding_rs does not
            // recognize it. Empty/non-token labels are ignored. Broken syntax
            // anywhere rejects the entire field, rather than recovering it.
            if charset.is_none() && name.eq_ignore_ascii_case(b"charset") {
                let label = trim_ows(&value);
                if !label.is_empty() && label.iter().copied().all(is_token) {
                    charset = Some(label.to_ascii_lowercase());
                }
            }
            scanner.ows();
        }
        Ok(Self { essence, charset })
    }

    pub(super) fn is_html(&self) -> bool {
        matches!(self.essence.as_str(), "text/html" | "application/xhtml+xml")
    }

    pub(super) fn is_css(&self) -> bool {
        self.essence == "text/css"
    }

    pub(super) fn is_javascript(&self) -> bool {
        matches!(
            self.essence.as_str(),
            "text/javascript"
                | "application/javascript"
                | "application/x-javascript"
                | "text/ecmascript"
                | "application/ecmascript"
        )
    }

    pub(super) fn is_image(&self) -> bool {
        self.essence.starts_with("image/")
    }

    pub(super) fn encoding(&self) -> &'static encoding_rs::Encoding {
        self.charset
            .as_deref()
            .and_then(encoding_rs::Encoding::for_label)
            .unwrap_or(encoding_rs::UTF_8)
    }
}

fn trim_ows(mut bytes: &[u8]) -> &[u8] {
    while bytes.first().is_some_and(|b| matches!(b, b' ' | b'\t')) {
        bytes = &bytes[1..];
    }
    while bytes.last().is_some_and(|b| matches!(b, b' ' | b'\t')) {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}

fn is_token(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

struct Scanner<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Scanner<'a> {
    fn ows(&mut self) {
        while self
            .bytes
            .get(self.pos)
            .is_some_and(|b| matches!(b, b' ' | b'\t'))
        {
            self.pos += 1;
        }
    }

    fn expect(&mut self, byte: u8) -> Result<(), &'static str> {
        if self.bytes.get(self.pos) != Some(&byte) {
            return Err("Sintaxe Content-Type invalida");
        }
        self.pos += 1;
        Ok(())
    }

    fn token(&mut self) -> Result<&'a [u8], &'static str> {
        let start = self.pos;
        while self.bytes.get(self.pos).is_some_and(|b| is_token(*b)) {
            self.pos += 1;
        }
        if self.pos == start {
            return Err("Token Content-Type invalido");
        }
        Ok(&self.bytes[start..self.pos])
    }

    fn value(&mut self) -> Result<Cow<'a, [u8]>, &'static str> {
        if self.bytes.get(self.pos) != Some(&b'"') {
            return self.token().map(Cow::Borrowed);
        }
        self.pos += 1;
        let mut value = Vec::new();
        while let Some(&byte) = self.bytes.get(self.pos) {
            self.pos += 1;
            match byte {
                b'"' => return Ok(Cow::Owned(value)),
                b'\\' => {
                    let escaped = *self
                        .bytes
                        .get(self.pos)
                        .ok_or("Escape Content-Type incompleto")?;
                    if !quoted_byte(escaped) {
                        return Err("Escape Content-Type invalido");
                    }
                    self.pos += 1;
                    value.push(escaped);
                }
                b if quoted_byte(b) => value.push(b),
                _ => return Err("Quoted-string Content-Type invalida"),
            }
        }
        Err("Quoted-string Content-Type incompleta")
    }
}

fn quoted_byte(byte: u8) -> bool {
    byte == b'\t' || byte == b' ' || (0x21..=0x7e).contains(&byte) || byte >= 0x80
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(value: &str) -> Result<HttpMetadata, &'static str> {
        HttpMetadata::from_fields([value.as_bytes()])
    }

    #[test]
    fn exact_mime_and_shared_consumers() {
        for value in [
            "text/html",
            " \tTeXt/HTmL \t; x = token\t",
            "application/xhtml+xml",
        ] {
            let metadata = parse(value).unwrap();
            assert!(metadata.is_html());
            assert!(!metadata.is_css() && !metadata.is_javascript() && !metadata.is_image());
        }
        assert!(parse(" TEXT/CSS ; charset=UTF-8").unwrap().is_css());
        for value in [
            "text/javascript",
            "application/javascript",
            "application/x-javascript",
            "text/ecmascript",
            "application/ecmascript",
        ] {
            assert!(
                parse(&format!("{} ; x=ok", value.to_ascii_uppercase()))
                    .unwrap()
                    .is_javascript()
            );
            assert!(
                !parse(&format!("{value}-malicioso"))
                    .unwrap()
                    .is_javascript()
            );
        }
        assert!(parse("ImAgE/svg+xml; x=ok").unwrap().is_image());
        assert!(!parse("text/html-malicioso").unwrap().is_html());
        assert!(!parse("application/xhtml+xml-malicioso").unwrap().is_html());
        assert!(!parse("text/css-malicioso").unwrap().is_css());
        assert!(!parse("images/png").unwrap().is_image());
    }

    #[test]
    fn absent_repeated_and_malformed_fields_are_rejected() {
        assert!(HttpMetadata::from_fields(std::iter::empty::<&[u8]>()).is_err());
        for second in [b"text/html".as_slice(), b"text/css".as_slice()] {
            assert!(HttpMetadata::from_fields([b"text/html".as_slice(), second]).is_err());
        }
        for value in [
            "",
            "text",
            "text/",
            "image/",
            "text /html",
            "text/h tml",
            "text/html, text/html",
            "text/html\r\n",
            "text/html;",
            "text/html; x",
            "text/html; =x",
            "text/html; x=",
            "text/html; x=\"open",
            "text/html; x=\"bad\\",
            "text/html; x=\"a\n\"",
            "text/html; x=\"ok\"junk",
            "téxt/html",
        ] {
            assert!(parse(value).is_err(), "{value:?}");
        }
    }

    #[test]
    fn charset_quotes_escapes_and_first_usable_label() {
        for value in [
            "text/html; charset = \"WINDOWS-1252\"",
            "text/html; x=\"semi;colon=ok\"; charset=windows-1252",
            "text/html; x=\"escaped\\\";still quoted\"; charset=windows-1252",
            "text/html; charset=\"windows\\-1252\"",
            "text/html; charset=\" \twindows-1252\t \"",
            "text/html; charset=\"\"; charset=\"not a label\"; charset=windows-1252",
        ] {
            assert_eq!(
                parse(value).unwrap().encoding(),
                encoding_rs::WINDOWS_1252,
                "{value}"
            );
        }
        for value in [
            "text/html",
            "text/html; charset=unknown; charset=windows-1252",
            "text/html; charset=utf-8; charset=windows-1252",
            "text/html; charset=\"windows;1252\"",
        ] {
            assert_eq!(
                parse(value).unwrap().encoding(),
                encoding_rs::UTF_8,
                "{value}"
            );
        }
        assert_eq!(
            parse("text/html; charset=windows-1252; charset=utf-8")
                .unwrap()
                .encoding(),
            encoding_rs::WINDOWS_1252
        );
    }

    #[test]
    fn quoted_bytes_cannot_inject_parameters_or_controls() {
        let metadata = HttpMetadata::from_fields([
            b"text/html; charset=\"\xff\"; charset=windows-1252".as_slice(),
        ])
        .unwrap();
        assert_eq!(metadata.encoding(), encoding_rs::WINDOWS_1252);
        assert_eq!(
            parse("text/html; x=\";,charset=windows-1252\"")
                .unwrap()
                .encoding(),
            encoding_rs::UTF_8
        );
        for control in [b'\r', b'\n', 0x7f, 0x00] {
            for escaped in [false, true] {
                let mut field = b"text/html; x=\"".to_vec();
                if escaped {
                    field.push(b'\\');
                }
                field.push(control);
                field.push(b'"');
                assert!(HttpMetadata::from_fields([field.as_slice()]).is_err());
            }
        }
    }
    #[test]
    fn header_and_parameter_limits_are_inclusive() {
        let exact = format!(
            "text/html{}",
            " ".repeat(MAX_CONTENT_TYPE_BYTES - "text/html".len())
        );
        assert!(parse(&exact).is_ok());
        assert_eq!(
            parse(&(exact + " ")).unwrap_err(),
            "Content-Type excede 4096 bytes"
        );
        let exact = format!("text/html{}", "; x=y".repeat(MAX_PARAMETERS));
        assert!(parse(&exact).is_ok());
        assert_eq!(
            parse(&(exact + "; x=y")).unwrap_err(),
            "Content-Type excede 32 parametros"
        );
        assert!(parse(&format!("text/html; x=\"{}\"", ";".repeat(100))).is_ok());
        assert!(
            parse(&format!(
                "text/html; x=\"{}\"",
                "x".repeat(MAX_CONTENT_TYPE_BYTES)
            ))
            .is_err()
        );
    }
}
