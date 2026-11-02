use crate::cli::ExportFormat;
use crate::output::{OwcError, Result};
use crate::store::Message;
use std::fs;
use std::path::{Path, PathBuf};

const EXPORT_CSS: &str = r#":root { color-scheme: light dark; --fg:#1f2328; --muted:#6a737d; --bg:#f6f7f9;
        --card:#fff; --line:#e3e6ea; --accent:#2f6feb; --quote:#f3f5f8; }
@media (prefers-color-scheme: dark) {
  :root { --fg:#e6edf3; --muted:#9aa4af; --bg:#0f1115; --card:#171a1f;
          --line:#272b31; --accent:#6ea8ff; --quote:#1d2128; }
}
* { box-sizing: border-box; }
body { margin:0; padding:32px 20px 80px; background:var(--bg); color:var(--fg);
       font:15px/1.65 -apple-system, "Segoe UI", "PingFang SC", "Microsoft YaHei", sans-serif; }
main { max-width: 900px; margin: 0 auto; }
h1 { font-size: 22px; margin: 0 0 6px; }
.meta { color: var(--muted); font-size: 13px; margin-bottom: 24px; }
.meta span { margin-right: 14px; white-space: nowrap; }
.day { margin: 28px 0 10px; font-size: 13px; color: var(--muted); letter-spacing: .08em;
       text-transform: uppercase; border-bottom: 1px solid var(--line); padding-bottom: 6px; }
.msg { display:flex; gap:12px; padding:10px 12px; border-radius:10px; background:var(--card);
       border:1px solid var(--line); margin:10px 0; }
.avatar { flex:0 0 34px; height:34px; border-radius:50%; background:var(--accent); color:#fff;
          font-size:13px; display:flex; align-items:center; justify-content:center; }
.body { min-width:0; flex:1; }
.who { font-size:13px; color:var(--muted); margin-bottom:2px; }
.who b { color:var(--fg); font-size:14px; }
.chip { display:inline-block; font-size:11px; padding:1px 6px; border-radius:999px;
        border:1px solid var(--line); color:var(--muted); margin-left:6px; vertical-align:1px; }
.text { white-space:pre-wrap; word-break:break-word; }
.ref { border-left:3px solid var(--line); padding:2px 0 2px 10px; margin:6px 0;
       color:var(--muted); font-size:14px; white-space:pre-wrap; }
figure { margin:10px 0 4px; }
img { max-width:100%; border-radius:8px; border:1px solid var(--line); display:block; }
figcaption, .cap { font-size:12px; color:var(--muted); margin-top:4px; }
a { color:var(--accent); text-decoration:none; }
a:hover { text-decoration:underline; }
.card { border:1px solid var(--line); border-radius:10px; padding:8px 10px; margin:8px 0;
        background:var(--quote); }
.card .t { font-weight:600; }
.card .d { font-size:13px; color:var(--muted); }
.file { display:inline-flex; align-items:center; gap:6px; border:1px solid var(--line);
        border-radius:8px; padding:4px 8px; margin:6px 8px 0 0; font-size:13px; }
.fwd { border-left:3px solid var(--accent); background:var(--quote); border-radius:0 10px 10px 0;
       padding:8px 12px; margin:8px 0; }
.fwd .h { font-weight:600; font-size:13px; margin-bottom:6px; }
.fwd .item { padding:4px 0; border-top:1px dashed var(--line); }
.fwd .item:first-of-type { border-top:none; }
.fwd .fwd { background:transparent; border-left:2px dashed var(--line); margin:6px 0; padding:6px 10px; }
.fwd .who2 { color:var(--muted); font-size:12px; }
.muted { color:var(--muted); font-size:13px; }
footer { max-width:900px; margin:36px auto 0; color:var(--muted); font-size:12px; }"#;

pub fn write_export(
    format: ExportFormat,
    messages: &[Message],
    out: &Path,
    copy_media: bool,
    media_out: Option<&Path>,
    decode_voice: bool,
    decoder: Option<&str>,
) -> Result<PathBuf> {
    fs::create_dir_all(out).map_err(|error| OwcError::data(error.to_string()))?;
    let media_path = media_out
        .map(Path::to_path_buf)
        .unwrap_or_else(|| out.join("media"));
    let prepared = if copy_media || decode_voice {
        crate::media::copy_message_media(messages, &media_path, decode_voice, decoder)?
    } else {
        messages.to_vec()
    };
    let path = match format {
        ExportFormat::Jsonl => {
            let path = out.join("messages.jsonl");
            let content = prepared
                .iter()
                .map(|message| serde_json::to_string(&message.to_value()))
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|error| OwcError::data(error.to_string()))?
                .join("\n");
            fs::write(&path, content).map_err(|error| OwcError::data(error.to_string()))?;
            path
        }
        ExportFormat::Txt => {
            write_txt(&prepared, out)?;
            out.to_path_buf()
        }
        ExportFormat::Md => {
            let path = out.join("messages.md");
            fs::write(&path, markdown(&prepared))
                .map_err(|error| OwcError::data(error.to_string()))?;
            path
        }
        ExportFormat::Html => {
            let path = out.join("messages.html");
            fs::write(&path, html(&prepared)).map_err(|error| OwcError::data(error.to_string()))?;
            path
        }
    };
    Ok(path)
}

fn write_txt(messages: &[Message], out: &Path) -> Result<()> {
    let mut conversations = std::collections::BTreeMap::<String, Vec<&Message>>::new();
    for message in messages {
        conversations
            .entry(message.conv_id.clone())
            .or_default()
            .push(message);
    }
    for (conv_id, items) in conversations {
        let filename = format!("{}.txt", safe_name(&conv_id));
        let content = items
            .iter()
            .map(|message| {
                format!(
                    "[{}] {}: {}",
                    message.send_time, message.sender_name, message.text
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(out.join(filename), content)
            .map_err(|error| OwcError::data(error.to_string()))?;
    }
    Ok(())
}

fn markdown(messages: &[Message]) -> String {
    let mut output = String::from("# open-wechat-cli export\n\n");
    let mut current_day = String::new();
    for message in messages {
        let day = message.send_time.get(..10).unwrap_or("Unknown date");
        if day != current_day {
            current_day = day.to_owned();
            output.push_str(&format!(
                "\n<div class=\"day\">{}</div>\n\n",
                md_escape(day)
            ));
            output.push_str(&format!("## {}\n\n", md_escape(day)));
        }
        output.push_str(&format!(
            "**{}** · {} · `{}`\n\n{}\n\n",
            md_escape(&message.sender_name),
            md_escape(&message.send_time),
            md_escape(&message.type_label),
            md_escape(&message.text)
        ));
        if let Some(link) = &message.extra.link {
            if let Some(url) = &link.url {
                output.push_str(&format!(
                    "[{}]({})\n\n",
                    md_escape(link.title.as_deref().unwrap_or(url)),
                    md_escape(url)
                ));
            }
        }
        if let Some(quote) = &message.extra.quote {
            output.push_str(&format!(
                "<div class=\"ref\">{}: {}</div>\n\n",
                md_escape(quote.sender_name.as_deref().unwrap_or("")),
                md_escape(quote.text.as_deref().unwrap_or(""))
            ));
        }
        for media in &message.extra.media {
            output.push_str(&format!(
                "<span class=\"file\">{} {}</span>\n\n",
                md_escape(&media.kind),
                md_escape(media.name.as_deref().unwrap_or("attachment"))
            ));
        }
    }
    output.push_str("\n---\n\nopen-wechat-cli\n");
    output
}

fn html(messages: &[Message]) -> String {
    let mut output = format!(
        "<!doctype html>\n<html lang=\"zh-CN\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>open-wechat-cli export</title>\n<style>\n{EXPORT_CSS}\n</style>\n</head>\n<body>\n<main>\n<h1>open-wechat-cli export</h1>\n<div class=\"meta\"><span>{} messages</span></div>\n",
        messages.len()
    );
    let mut current_day = String::new();
    for message in messages {
        let day = message.send_time.get(..10).unwrap_or("Unknown date");
        if day != current_day {
            current_day = day.to_owned();
            output.push_str(&format!("<div class=\"day\">{}</div>\n", html_escape(day)));
        }
        let avatar = message.sender_name.chars().next().unwrap_or('?');
        output.push_str(&format!(
            "<div class=\"msg\"><div class=\"avatar\">{}</div><div class=\"body\"><div class=\"who\"><b>{}</b><span class=\"chip\">{}</span><span class=\"muted\">{}</span></div><div class=\"text\">{}</div>",
            html_escape(&avatar.to_string()),
            html_escape(&message.sender_name),
            html_escape(&message.type_label),
            html_escape(&message.send_time),
            html_escape(&message.text)
        ));
        if let Some(link) = &message.extra.link {
            if let Some(url) = &link.url {
                output.push_str(&format!(
                    "<div class=\"card\"><div class=\"t\"><a href=\"{}\">{}</a></div><div class=\"d\">{}</div></div>",
                    html_escape(url),
                    html_escape(link.title.as_deref().unwrap_or(url)),
                    html_escape(link.desc.as_deref().unwrap_or(""))
                ));
            }
        }
        if let Some(quote) = &message.extra.quote {
            output.push_str(&format!(
                "<div class=\"ref\"><b>{}</b> {}</div>",
                html_escape(quote.sender_name.as_deref().unwrap_or("")),
                html_escape(quote.text.as_deref().unwrap_or(""))
            ));
        }
        if let Some(forward) = &message.extra.forward {
            output.push_str(&format!(
                "<div class=\"fwd\"><div class=\"h\">{}</div>{}</div>",
                html_escape(forward.title.as_deref().unwrap_or("Forwarded chat")),
                forward
                    .items
                    .iter()
                    .take(100)
                    .map(|item| {
                        format!(
                            "<div class=\"item\">{}</div>",
                            html_escape(&item.to_string())
                        )
                    })
                    .collect::<String>()
            ));
        }
        for media in &message.extra.media {
            output.push_str(&format!(
                "<span class=\"file\">{} {}</span>",
                html_escape(&media.kind),
                html_escape(media.name.as_deref().unwrap_or("attachment"))
            ));
        }
        output.push_str("</div></div>\n");
    }
    output.push_str("</main>\n<footer>\n open-wechat-cli \n silk \n</footer>\n</body>\n</html>\n");
    output
}

fn safe_name(value: &str) -> String {
    let name: String = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
                character
            } else {
                '_'
            }
        })
        .take(80)
        .collect();
    if name.is_empty() {
        "conversation".to_owned()
    } else {
        name
    }
}

fn md_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('|', "\\|")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
