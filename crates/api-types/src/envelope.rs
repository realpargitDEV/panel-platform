//! The response envelope and pagination.
//!
//! Every response — success or failure — carries a request id, so a user
//! reporting "it said something went wrong" can be traced to an exact log line.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::errors::ApiError;

/// Metadata attached to every successful response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ResponseMeta {
    /// Echoed from the request, or generated when the client omitted it.
    pub request_id: String,
    /// Agent's clock, RFC 3339 UTC. Lets the client detect a skewed local clock
    /// rather than silently rendering nonsense timestamps.
    pub server_time: String,
}

/// Success or failure, discriminated by `ok` so a client can branch before
/// knowing the payload type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum ApiResponse<T> {
    Ok {
        ok: OkTrue,
        data: T,
        meta: ResponseMeta,
    },
    Err {
        ok: OkFalse,
        error: ApiError,
    },
}

impl<T> ApiResponse<T> {
    pub fn success(data: T, request_id: impl Into<String>, server_time: impl Into<String>) -> Self {
        ApiResponse::Ok {
            ok: OkTrue,
            data,
            meta: ResponseMeta {
                request_id: request_id.into(),
                server_time: server_time.into(),
            },
        }
    }

    pub fn failure(error: ApiError) -> Self {
        ApiResponse::Err { ok: OkFalse, error }
    }

    pub fn is_ok(&self) -> bool {
        matches!(self, ApiResponse::Ok { .. })
    }
}

/// Serialises as the literal `true`. Makes `ok` a discriminant the type system
/// enforces rather than a boolean somebody could set inconsistently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OkTrue;

/// Serialises as the literal `false`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OkFalse;

macro_rules! literal_bool {
    ($name:ident, $value:literal) => {
        impl Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_bool($value)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let observed = bool::deserialize(d)?;
                if observed == $value {
                    Ok($name)
                } else {
                    Err(serde::de::Error::invalid_value(
                        serde::de::Unexpected::Bool(observed),
                        &stringify!($value),
                    ))
                }
            }
        }

        impl JsonSchema for $name {
            fn schema_name() -> std::borrow::Cow<'static, str> {
                std::borrow::Cow::Borrowed(stringify!($name))
            }
            fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
                schemars::json_schema!({ "type": "boolean", "const": $value })
            }
        }
    };
}

literal_bool!(OkTrue, true);
literal_bool!(OkFalse, false);

/// A page of results. Cursor-based: see `docs/api-design.md` §1 for why offsets
/// are not offered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Page<T> {
    pub items: Vec<T>,
    /// Pass as `cursor` to fetch the next page. `None` at the end.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

impl<T> Page<T> {
    /// Build a page from one extra row: query `limit + 1`, and the presence of
    /// that extra row is what `has_more` means. Avoids a second COUNT query and
    /// cannot disagree with the rows actually returned.
    pub fn from_overfetch(
        mut rows: Vec<T>,
        limit: usize,
        cursor_of: impl Fn(&T) -> String,
    ) -> Self {
        let has_more = rows.len() > limit;
        if has_more {
            rows.truncate(limit);
        }
        let next_cursor = if has_more {
            rows.last().map(&cursor_of)
        } else {
            None
        };
        Page {
            items: rows,
            next_cursor,
            has_more,
        }
    }

    pub fn empty() -> Self {
        Page {
            items: Vec::new(),
            next_cursor: None,
            has_more: false,
        }
    }
}

/// Query parameters for a paginated endpoint.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PageRequest {
    /// Defaults to 50, clamped to `MAX_LIMIT`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    /// The `next_cursor` from the previous page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

impl PageRequest {
    pub const DEFAULT_LIMIT: u32 = 50;
    pub const MAX_LIMIT: u32 = 200;

    /// Clamp rather than reject: a client asking for 10,000 rows gets 200 and a
    /// cursor, which is more useful than an error it has to handle.
    pub fn effective_limit(&self) -> u32 {
        self.limit
            .unwrap_or(Self::DEFAULT_LIMIT)
            .clamp(1, Self::MAX_LIMIT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors::ErrorCode;

    #[test]
    fn success_serialises_with_ok_true() {
        let response = ApiResponse::success(42u32, "req_1", "2026-07-29T00:00:00Z");
        let json = serde_json::to_value(&response).unwrap();
        assert_eq!(json["ok"], serde_json::json!(true));
        assert_eq!(json["data"], serde_json::json!(42));
        assert_eq!(json["meta"]["request_id"], serde_json::json!("req_1"));
    }

    #[test]
    fn failure_serialises_with_ok_false_and_no_data_key() {
        let response: ApiResponse<u32> =
            ApiResponse::failure(ApiError::new(ErrorCode::NotFound, "Gone.", "req_2"));
        let json = serde_json::to_value(&response).unwrap();
        assert_eq!(json["ok"], serde_json::json!(false));
        assert!(json.get("data").is_none());
        assert_eq!(json["error"]["code"], serde_json::json!("NOT_FOUND"));
    }

    #[test]
    fn a_wrong_ok_discriminant_fails_to_deserialise() {
        // `ok: false` alongside a data payload is not a shape the type permits.
        let raw = r#"{"ok":false,"data":42,"meta":{"request_id":"r","server_time":"t"}}"#;
        assert!(serde_json::from_str::<ApiResponse<u32>>(raw).is_err());
    }

    #[test]
    fn overfetch_sets_has_more_and_trims_the_extra_row() {
        let rows = vec!["a", "b", "c", "d"];
        let page = Page::from_overfetch(rows, 3, |row| row.to_string());
        assert_eq!(page.items, vec!["a", "b", "c"]);
        assert!(page.has_more);
        assert_eq!(page.next_cursor.as_deref(), Some("c"));
    }

    #[test]
    fn a_short_result_is_the_last_page() {
        let page = Page::from_overfetch(vec!["a", "b"], 3, |row| row.to_string());
        assert_eq!(page.items.len(), 2);
        assert!(!page.has_more);
        assert_eq!(page.next_cursor, None);
    }

    #[test]
    fn an_exactly_full_result_is_still_the_last_page() {
        // Three rows for a limit of three means no overfetched row appeared,
        // so there is nothing after them.
        let page = Page::from_overfetch(vec!["a", "b", "c"], 3, |row| row.to_string());
        assert!(!page.has_more);
        assert_eq!(page.next_cursor, None);
    }

    #[test]
    fn limits_are_clamped_not_rejected() {
        assert_eq!(PageRequest::default().effective_limit(), 50);
        assert_eq!(
            PageRequest {
                limit: Some(10_000),
                cursor: None
            }
            .effective_limit(),
            200
        );
        assert_eq!(
            PageRequest {
                limit: Some(0),
                cursor: None
            }
            .effective_limit(),
            1
        );
    }
}
