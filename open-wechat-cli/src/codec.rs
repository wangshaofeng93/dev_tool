use crate::proto::{self, WireValue};
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Default)]
pub struct MediaInfo {
    pub kind: String,
    pub name: Option<String>,
    pub size: Option<u64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub md5: Option<String>,
    pub duration_s: Option<f64>,
    pub local_path: Option<String>,
    pub exists: Option<bool>,
    pub ocr: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct LinkInfo {
    pub url: Option<String>,
    pub title: Option<String>,
    pub desc: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct QuoteInfo {
    pub sender_name: Option<String>,
    pub text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ForwardInfo {
    pub title: Option<String>,
    pub total: Option<usize>,
    pub items: Vec<Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DecodedContent {
    pub type_label: String,
    pub text: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub media: Vec<MediaInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<LinkInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forward: Option<ForwardInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quote: Option<QuoteInfo>,
}

// S3: Numeric WXWork content_type values are not documented; familiar WeChat-style
// values are mapped conservatively and all unrecognized values degrade to `other`.
pub fn type_label(content_type: i64) -> &'static str {
    match content_type {
        1 => "text",
        3 => "image",
        34 => "voice",
        43 => "video",
        49 => "link",
        10000 | 10002 => "system",
        2000 => "file",
        2001 => "drive_file",
        2002 => "drive_notice",
        2003 => "doc_link",
        2004 => "article",
        2005 => "miniprogram",
        2006 => "chat_record",
        2007 => "article_card",
        2008 => "app_card",
        2009 => "rich_card",
        2010 => "approval",
        2011 => "stats_card",
        2012 => "moment_notice",
        _ => "other",
    }
}

pub fn decode_content(content_type: i64, raw: &str) -> DecodedContent {
    let label = type_label(content_type);
    let mut decoded = DecodedContent {
        type_label: label.to_owned(),
        text: String::new(),
        media: Vec::new(),
        link: None,
        forward: None,
        quote: None,
    };
    if label == "text" {
        decoded.text = raw.to_owned();
        return decoded;
    }
    if let Ok(value) = serde_json::from_str::<Value>(raw) {
        extract_json(&value, &mut decoded);
        if !decoded.text.is_empty() || decoded.link.is_some() || !decoded.media.is_empty() {
            return decoded;
        }
    }
    if raw.contains('<') && raw.contains('>') {
        extract_xml(raw, &mut decoded);
        if !decoded.text.is_empty() || decoded.link.is_some() || !decoded.media.is_empty() {
            return decoded;
        }
    }
    if let Ok(bytes) = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, raw) {
        if let Some(fields) = proto::decode(&bytes) {
            let strings: Vec<String> = fields
                .iter()
                .filter_map(|field| match &field.value {
                    WireValue::Bytes(value) => std::str::from_utf8(value).ok().map(str::to_owned),
                })
                .collect();
            if let Some(text) = strings.iter().find(|value| !value.trim().is_empty()) {
                decoded.text = text.clone();
            }
            if decoded.text.is_empty() {
                decoded.type_label = "other".to_owned();
            }
            return decoded;
        }
    }
    decoded.type_label = "other".to_owned();
    decoded
}

fn extract_json(value: &Value, decoded: &mut DecodedContent) {
    let Some(object) = value.as_object() else {
        return;
    };
    let get = |keys: &[&str]| {
        keys.iter()
            .find_map(|key| object.get(*key).and_then(Value::as_str))
            .map(str::to_owned)
    };
    decoded.text = get(&["text", "content", "body"]).unwrap_or_default();
    let url = get(&["url", "link", "href"]);
    let title = get(&["title", "name"]);
    let desc = get(&["desc", "description", "summary"]);
    if url.is_some() || title.is_some() || desc.is_some() {
        decoded.link = Some(LinkInfo { url, title, desc });
    }
    if let Some(sender_name) = get(&["quote_sender", "quoted_name"]) {
        decoded.quote = Some(QuoteInfo {
            sender_name: Some(sender_name),
            text: get(&["quote_text", "quoted_text"]),
        });
    }
    if let Some(items) = object.get("items").and_then(Value::as_array) {
        decoded.forward = Some(ForwardInfo {
            title: get(&["forward_title", "title"]),
            total: Some(items.len()),
            items: items.iter().take(100).cloned().collect(),
        });
    }
    if let Some(media) = object.get("media").and_then(Value::as_array) {
        decoded.media = media
            .iter()
            .filter_map(|entry| {
                let object = entry.as_object()?;
                Some(MediaInfo {
                    kind: object
                        .get("kind")
                        .and_then(Value::as_str)
                        .unwrap_or("file")
                        .to_owned(),
                    name: object
                        .get("name")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    size: object.get("size").and_then(Value::as_u64),
                    width: object
                        .get("width")
                        .and_then(Value::as_u64)
                        .and_then(|x| u32::try_from(x).ok()),
                    height: object
                        .get("height")
                        .and_then(Value::as_u64)
                        .and_then(|x| u32::try_from(x).ok()),
                    md5: object.get("md5").and_then(Value::as_str).map(str::to_owned),
                    duration_s: object.get("duration_s").and_then(Value::as_f64),
                    local_path: object
                        .get("local_path")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    exists: object.get("exists").and_then(Value::as_bool),
                    ocr: object.get("ocr").and_then(Value::as_str).map(str::to_owned),
                })
            })
            .collect();
    }
}

fn extract_xml(raw: &str, decoded: &mut DecodedContent) {
    let title = xml_tag(raw, &["title", "name"]);
    let url = xml_tag(raw, &["url", "weburl", "link"]);
    let desc = xml_tag(raw, &["des", "desc", "description"]);
    let text = xml_tag(raw, &["content", "text"]);
    if let Some(text) = text.filter(|value| !value.is_empty()) {
        decoded.text = text;
    }
    if url.is_some() || title.is_some() || desc.is_some() {
        decoded.link = Some(LinkInfo { url, title, desc });
    }
    if decoded.link.is_none() && decoded.text.is_empty() {
        decoded.type_label = "other".to_owned();
    }
}

fn xml_tag(input: &str, tags: &[&str]) -> Option<String> {
    for tag in tags {
        let open = format!("<{tag}>");
        let close = format!("</{tag}>");
        if let Some(start) = input.find(&open) {
            let value_start = start + open.len();
            if let Some(end) = input[value_start..].find(&close) {
                return Some(decode_entities(&input[value_start..value_start + end]));
            }
        }
    }
    None
}

fn decode_entities(input: &str) -> String {
    input
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}
