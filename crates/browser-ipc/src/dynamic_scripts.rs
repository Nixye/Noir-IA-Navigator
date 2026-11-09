//! Data-only handoff for classic scripts inserted by a live document.
use serde::{Deserialize, Serialize};

pub const MAX_DYNAMIC_SCRIPTS: usize = 32;
pub const MAX_DYNAMIC_SCRIPT_SRC_BYTES: usize = 8192;
pub const MAX_DYNAMIC_SCRIPT_BYTES: usize = 128 * 1024;
pub const MAX_DYNAMIC_SCRIPT_TOTAL_BYTES: usize = 512 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DynamicScriptRequest {
    /// Document-local, nonzero, never reused, including failed attempts.
    pub id: u64,
    /// A source reference, never an instruction to open files in the renderer.
    #[serde(deserialize_with = "bounded_string::<_, MAX_DYNAMIC_SCRIPT_SRC_BYTES>")]
    pub src: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DynamicScriptResponse {
    pub id: u64,
    /// None means unavailable, denied or over budget. No host path is exposed.
    #[serde(deserialize_with = "bounded_source")]
    pub source: Option<String>,
}

// Limit retained strings as they are decoded; the outer frame cap still
// bounds JSON escape scratch space and is not a replacement for these limits.
fn bounded_string<'de, D: serde::Deserializer<'de>, const LIMIT: usize>(
    deserializer: D,
) -> Result<String, D::Error> {
    struct StringVisitor<const LIMIT: usize>;
    impl<'de, const LIMIT: usize> serde::de::Visitor<'de> for StringVisitor<LIMIT> {
        type Value = String;
        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(formatter, "a script string of at most {LIMIT} bytes")
        }
        fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<String, E> {
            if value.len() > LIMIT {
                return Err(E::custom("Dynamic script string limit exceeded"));
            }
            Ok(value.to_owned())
        }
        fn visit_string<E: serde::de::Error>(self, value: String) -> Result<String, E> {
            if value.len() > LIMIT {
                return Err(E::custom("Dynamic script string limit exceeded"));
            }
            Ok(value)
        }
    }
    deserializer.deserialize_string(StringVisitor::<LIMIT>)
}

fn bounded_source<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    struct SourceVisitor;
    impl<'de> serde::de::Visitor<'de> for SourceVisitor {
        type Value = Option<String>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("an optional bounded script source")
        }
        fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_some<D: serde::Deserializer<'de>>(
            self,
            deserializer: D,
        ) -> Result<Self::Value, D::Error> {
            bounded_string::<D, MAX_DYNAMIC_SCRIPT_BYTES>(deserializer).map(Some)
        }
    }
    deserializer.deserialize_option(SourceVisitor)
}

pub fn validate_dynamic_script_requests(requests: &[DynamicScriptRequest]) -> Result<(), String> {
    if requests.len() > MAX_DYNAMIC_SCRIPTS {
        return Err("Dynamic script request count exceeded".into());
    }
    let mut ids = std::collections::HashSet::new();
    for request in requests {
        if request.id == 0
            || !ids.insert(request.id)
            || request.src.is_empty()
            || request.src.len() > MAX_DYNAMIC_SCRIPT_SRC_BYTES
            || request.src.chars().any(char::is_control)
        {
            return Err("Invalid dynamic script request".into());
        }
    }
    Ok(())
}

pub fn validate_dynamic_script_responses(
    responses: &[DynamicScriptResponse],
) -> Result<(), String> {
    if responses.len() > MAX_DYNAMIC_SCRIPTS {
        return Err("Dynamic script response count exceeded".into());
    }
    let mut ids = std::collections::HashSet::new();
    let mut bytes = 0usize;
    for response in responses {
        if response.id == 0 || !ids.insert(response.id) {
            return Err("Invalid dynamic script response identity".into());
        }
        if let Some(source) = &response.source {
            bytes = bytes
                .checked_add(source.len())
                .ok_or("Script size overflow")?;
            if source.len() > MAX_DYNAMIC_SCRIPT_BYTES || bytes > MAX_DYNAMIC_SCRIPT_TOTAL_BYTES {
                return Err("Dynamic script source budget exceeded".into());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        PROTOCOL_VERSION, RenderResponse, SessionRequest, Viewport, read_message, write_message,
    };

    #[test]
    fn request_and_response_semantics_are_bounded() {
        let valid = DynamicScriptRequest {
            id: 1,
            src: "app.js".into(),
        };
        assert!(validate_dynamic_script_requests(std::slice::from_ref(&valid)).is_ok());
        assert!(validate_dynamic_script_requests(&[valid.clone(), valid.clone()]).is_err());
        for src in [
            String::new(),
            "a\n.js".into(),
            "a".repeat(MAX_DYNAMIC_SCRIPT_SRC_BYTES + 1),
        ] {
            assert!(
                validate_dynamic_script_requests(&[DynamicScriptRequest { id: 1, src }]).is_err()
            );
        }
        assert!(
            validate_dynamic_script_requests(&[DynamicScriptRequest { id: 0, ..valid }]).is_err()
        );
        let valid = DynamicScriptResponse {
            id: 1,
            source: Some("x".repeat(MAX_DYNAMIC_SCRIPT_BYTES)),
        };
        assert!(validate_dynamic_script_responses(std::slice::from_ref(&valid)).is_ok());
        assert!(validate_dynamic_script_responses(&[valid.clone(), valid.clone()]).is_err());
        let responses: Vec<_> = (1..=5)
            .map(|id| DynamicScriptResponse {
                id,
                ..valid.clone()
            })
            .collect();
        assert!(validate_dynamic_script_responses(&responses).is_err());
        assert!(
            validate_dynamic_script_responses(&[DynamicScriptResponse {
                id: 1,
                source: Some("x".repeat(MAX_DYNAMIC_SCRIPT_BYTES + 1))
            }])
            .is_err()
        );
    }

    #[test]
    fn both_script_collection_bounds_apply_during_frame_decode() {
        let response = RenderResponse {
            pending_scripts: vec![
                DynamicScriptRequest {
                    id: 1,
                    src: "app.js".into()
                };
                MAX_DYNAMIC_SCRIPTS + 1
            ],
            protocol_version: PROTOCOL_VERSION,
            id: 1,
            session_storage: vec![],
            result: Err("failure".into()),
        };
        let mut frame = Vec::new();
        write_message(&mut frame, &response).unwrap();
        assert!(
            read_message::<_, RenderResponse>(&mut frame.as_slice())
                .unwrap_err()
                .to_string()
                .contains("collection limit")
        );
        let request = SessionRequest::Scripts {
            id: 2,
            viewport: Viewport::default(),
            scripts: vec![
                DynamicScriptResponse {
                    id: 1,
                    source: None
                };
                MAX_DYNAMIC_SCRIPTS + 1
            ],
        };
        let mut frame = Vec::new();
        write_message(&mut frame, &request).unwrap();
        assert!(
            read_message::<_, SessionRequest>(&mut frame.as_slice())
                .unwrap_err()
                .to_string()
                .contains("collection limit")
        );
    }

    #[test]
    fn script_strings_are_bounded_during_frame_deserialization() {
        for length in [
            MAX_DYNAMIC_SCRIPT_SRC_BYTES,
            MAX_DYNAMIC_SCRIPT_SRC_BYTES + 1,
        ] {
            let request = DynamicScriptRequest {
                id: 1,
                src: "a".repeat(length),
            };
            let mut bytes = Vec::new();
            write_message(&mut bytes, &request).unwrap();
            assert_eq!(
                read_message::<_, DynamicScriptRequest>(&mut bytes.as_slice()).is_ok(),
                length == MAX_DYNAMIC_SCRIPT_SRC_BYTES
            );
        }
        for length in [MAX_DYNAMIC_SCRIPT_BYTES, MAX_DYNAMIC_SCRIPT_BYTES + 1] {
            let response = DynamicScriptResponse {
                id: 1,
                source: Some("x".repeat(length)),
            };
            let mut bytes = Vec::new();
            write_message(&mut bytes, &response).unwrap();
            assert_eq!(
                read_message::<_, DynamicScriptResponse>(&mut bytes.as_slice()).is_ok(),
                length == MAX_DYNAMIC_SCRIPT_BYTES
            );
        }
    }

    #[test]
    fn pending_script_field_is_required_by_new_protocol() {
        assert!(
            serde_json::from_str::<RenderResponse>(
                r#"{"protocol_version":9,"id":1,"session_storage":[],"result":{"Err":"failed"}}"#
            )
            .is_err()
        );
    }
}
