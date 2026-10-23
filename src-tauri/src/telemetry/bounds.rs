//! Curated, byte-bounded text and metadata limits shared by every DTO boundary.

use serde::{Deserialize, Serialize};

pub const MESSAGE_MAX_BYTES: usize = 160;
pub const LABEL_MAX_BYTES: usize = 128;
pub const PROVIDER_MAX_BYTES: usize = 32;
pub const METRIC_ID_MAX_BYTES: usize = 48;
pub const MAX_SCOPES: usize = 16;
pub const MAX_CAPABILITIES: usize = 64;
pub const MAX_NETWORK_MEMBERS: usize = 32;
pub const MAX_NOTICES: usize = 8;

pub fn utf8_len(text: &str) -> usize {
    text.len()
}

pub fn is_within(text: &str, max_bytes: usize) -> bool {
    text.len() <= max_bytes
}

/// Removes control characters (including line breaks) without trimming visible spaces.
pub fn strip_control(text: &str) -> String {
    text.chars()
        .filter(|character| !character.is_control())
        .collect()
}

/// Truncates to at most `max_bytes`, never splitting a UTF-8 scalar value.
pub fn truncate_utf8(text: &str, max_bytes: usize) -> &str {
    if text.len() <= max_bytes {
        return text;
    }
    let mut end = 0;
    for (index, character) in text.char_indices() {
        let next = index + character.len_utf8();
        if next > max_bytes {
            break;
        }
        end = next;
    }
    &text[..end]
}

pub fn bounded_label(text: &str, max_bytes: usize) -> String {
    truncate_utf8(&strip_control(text), max_bytes).to_string()
}

pub fn bounded_message(text: &str) -> String {
    bounded_label(text, MESSAGE_MAX_BYTES)
}

/// Parses the canonical unsigned decimal string form used for every byte amount.
pub fn parse_decimal_u64(text: &str) -> Option<u64> {
    if text.is_empty() || (text.len() > 1 && text.starts_with('0')) {
        return None;
    }
    if !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse::<u64>().ok()
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NetworkMemberMetadata {
    pub member_ids: Vec<String>,
    pub total_member_count: u32,
    pub omitted_member_count: u32,
}

/// Shortens only descriptive network metadata; aggregate collection still covers every interface.
pub fn network_member_metadata(member_ids: &[String]) -> NetworkMemberMetadata {
    let total_member_count = member_ids.len();
    let kept: Vec<String> = member_ids
        .iter()
        .take(MAX_NETWORK_MEMBERS)
        .map(|id| bounded_label(id, LABEL_MAX_BYTES))
        .collect();
    NetworkMemberMetadata {
        total_member_count: total_member_count as u32,
        omitted_member_count: (total_member_count - kept.len()) as u32,
        member_ids: kept,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncation_never_splits_characters() {
        let text = "é".repeat(200);
        assert_eq!(truncate_utf8(&text, MESSAGE_MAX_BYTES).len(), 160);
        assert_eq!(truncate_utf8("日本語", 5), "日");
        assert_eq!(bounded_message("ok\u{0}\nvalue"), "okvalue");
    }

    #[test]
    fn network_metadata_discloses_omissions_without_shrinking_collection() {
        let members: Vec<String> = (0..40).map(|index| format!("eth{index}")).collect();
        let metadata = network_member_metadata(&members);
        assert_eq!(metadata.member_ids.len(), MAX_NETWORK_MEMBERS);
        assert_eq!(metadata.total_member_count, 40);
        assert_eq!(metadata.omitted_member_count, 8);
    }

    #[test]
    fn decimal_parsing_rejects_noncanonical_forms() {
        assert_eq!(parse_decimal_u64("0"), Some(0));
        assert_eq!(parse_decimal_u64("18446744073709551615"), Some(u64::MAX));
        for invalid in ["", "00", "01", "-1", "+1", "1.0", " 1", "1 ", "0x1"] {
            assert_eq!(parse_decimal_u64(invalid), None, "{invalid:?}");
        }
    }
}
