use crate::cache;
use crate::cli::{
    Cli, ExportArgs, HistoryArgs, MessageFilterArgs, MessageListArgs, MessagesCommand, Order,
};
use crate::config::Config;
use crate::output::{OwcError, Result};
use crate::state::State;
use crate::store::{self, Message};
use base64::Engine;
use chrono::{Local, NaiveDate, NaiveDateTime, TimeZone};
use chrono_tz::Tz;
use serde_json::{json, Value};
use std::path::PathBuf;

pub fn messages(
    command: MessagesCommand,
    cli: &Cli,
    config: &Config,
    state: &State,
) -> Result<(String, Value, String)> {
    match command {
        MessagesCommand::Stats(args) => stats(args, cli, config, state),
        MessagesCommand::List(args) => list(args, cli, config, state, "messages list"),
        MessagesCommand::Export(args) => export(args, cli, config, state),
    }
}

pub fn history(
    args: HistoryArgs,
    cli: &Cli,
    config: &Config,
    state: &State,
) -> Result<(String, Value, String)> {
    let since = parse_bound(args.since.as_deref(), &cli.tz, false)?;
    let until = parse_bound(args.until.as_deref(), &cli.tz, true)?;
    crate::host::require_local_data()?;
    let account = selected_account(cli, config);
    let account = account.ok_or_else(|| OwcError::precondition("No account selected."))?;
    let db_dir = state.cache_path(account).join("decrypted").join("Data");
    if !db_dir.exists() {
        return Err(OwcError::precondition(
            "No decrypted database cache exists; automatic key/snapshot/decrypt is not available in this reconstruction.",
        ));
    }
    let args_list = MessageListArgs {
        filter: MessageFilterArgs {
            db_dir: Some(db_dir),
            conv: args.conv,
            since: args.since,
            until: args.until,
        },
        query: args.query,
        limit: args.limit,
        cursor: args.cursor,
        order: args.order,
        limit_per_conv: None,
        index: args.index,
        with_index: false,
        time_budget: None,
        max_hits: None,
        step: None,
    };
    let _ = (since, until, args.refresh, args.max_age, args.no_persist);
    list(args_list, cli, config, state, "history")
}

fn stats(
    args: MessageFilterArgs,
    cli: &Cli,
    config: &Config,
    state: &State,
) -> Result<(String, Value, String)> {
    let (messages, resolved) = fetch_messages(&args, None, None, None, cli, config, state)?;
    let mut by_type = serde_json::Map::new();
    let mut senders = std::collections::HashSet::new();
    for message in &messages {
        senders.insert(message.sender_id.clone());
        let count = by_type
            .entry(message.type_label.clone())
            .or_insert_with(|| json!(0));
        if let Some(number) = count.as_i64() {
            *count = json!(number + 1);
        }
    }
    let first = messages.iter().map(|message| message.send_time_epoch).min();
    let last = messages.iter().map(|message| message.send_time_epoch).max();
    let mut data = json!({
        "total": messages.len(),
        "senders": senders.len(),
        "first_time": first.map(format_time),
        "last_time": last.map(format_time),
        "by_type": by_type,
    });
    if let Some(conv) = resolved {
        data["resolved_conv"] = json!(conv);
        let conv_total = messages
            .iter()
            .filter(|message| message.conv_id == conv)
            .count();
        data["by_conv"] = json!([{"conv_id": conv, "total": conv_total}]);
    }
    Ok(crate::output::result("messages stats", data))
}

fn list(
    args: MessageListArgs,
    cli: &Cli,
    config: &Config,
    state: &State,
    command: &str,
) -> Result<(String, Value, String)> {
    if args.limit == 0 {
        return Err(OwcError::usage("--limit must be greater than zero."));
    }
    let (mut messages, resolved) = fetch_messages(
        &args.filter,
        args.query.as_deref(),
        Some(args.order),
        args.cursor.as_deref(),
        cli,
        config,
        state,
    )?;
    if let Some(per_conv) = args.limit_per_conv {
        if per_conv == 0 {
            return Err(OwcError::usage(
                "--limit-per-conv must be greater than zero.",
            ));
        }
        let mut counts = std::collections::HashMap::<String, usize>::new();
        messages.retain(|message| {
            let count = counts.entry(message.conv_id.clone()).or_default();
            if *count >= per_conv {
                false
            } else {
                *count += 1;
                true
            }
        });
    }
    let total = messages.len();
    let page: Vec<Message> = messages.into_iter().take(args.limit).collect();
    let next_cursor = if page.len() == args.limit && total > page.len() {
        page.last().map(encode_cursor)
    } else {
        None
    };
    let items: Vec<Value> = page.iter().map(Message::to_value).collect();
    let mut data = json!({
        "items": items,
        "next_cursor": next_cursor,
        "resolved_conv": resolved,
        "total": total,
    });
    if args.with_index {
        data["index"] = json!({"enabled": args.index, "available": false});
    }
    Ok(crate::output::result(command, data))
}

fn export(
    args: ExportArgs,
    cli: &Cli,
    config: &Config,
    state: &State,
) -> Result<(String, Value, String)> {
    let (mut messages, resolved) = fetch_messages(
        &args.list.filter,
        args.list.query.as_deref(),
        Some(args.list.order),
        args.list.cursor.as_deref(),
        cli,
        config,
        state,
    )?;
    messages.truncate(args.list.limit);
    let out = args.out.unwrap_or_else(|| state.export_path());
    if args.decode_voice {
        let decoder = args
            .silk_decoder
            .as_deref()
            .or_else(|| (!config.silk_decoder.is_empty()).then_some(config.silk_decoder.as_str()));
        if decoder.is_none() {
            return Err(OwcError::new(
                crate::output::ErrorCode::Dependency,
                "SILK decoding requires an external decoder.",
                "Remove --decode-voice to export .silk files, or configure --silk-decoder.",
            ));
        }
    }
    let media_out = args.media_out.clone();
    let path = crate::render::write_export(
        args.format,
        &messages,
        &out,
        args.copy_media,
        media_out.as_deref(),
        args.decode_voice,
        args.silk_decoder
            .as_deref()
            .or_else(|| (!config.silk_decoder.is_empty()).then_some(config.silk_decoder.as_str())),
    )?;
    Ok(crate::output::result(
        "messages export",
        json!({
            "path": path,
            "format": format!("{:?}", args.format).to_lowercase(),
            "count": messages.len(),
            "resolved_conv": resolved,
        }),
    ))
}

fn fetch_messages(
    filter: &MessageFilterArgs,
    query: Option<&str>,
    order: Option<Order>,
    cursor: Option<&str>,
    cli: &Cli,
    config: &Config,
    state: &State,
) -> Result<(Vec<Message>, Option<String>)> {
    let since = parse_bound(filter.since.as_deref(), &cli.tz, false)?;
    let until = parse_bound(filter.until.as_deref(), &cli.tz, true)?;
    if let (Some(since), Some(until)) = (since, until) {
        if since > until {
            return Err(OwcError::usage("--since must not be later than --until."));
        }
    }
    let account = selected_account(cli, config);
    let dir = cache::resolve_db_dir(filter.db_dir.as_deref(), account, state)?;
    let (mut messages, schema_map) = store::read_messages(&dir, since, until)?;
    let mut resolved = None;
    if let Some(conv) = filter.conv.as_deref() {
        if messages.iter().any(|message| message.conv_id == conv) {
            messages.retain(|message| message.conv_id == conv);
            resolved = Some(conv.to_owned());
        } else {
            let needle = conv.to_lowercase();
            let matches: Vec<&Message> = messages
                .iter()
                .filter(|message| message.conv_name.to_lowercase().contains(&needle))
                .collect();
            let mut active = std::collections::HashMap::<String, i64>::new();
            for message in &matches {
                active
                    .entry(message.conv_id.clone())
                    .and_modify(|last| *last = (*last).max(message.send_time_epoch))
                    .or_insert(message.send_time_epoch);
            }
            let selected = active
                .into_iter()
                .max_by_key(|entry| entry.1)
                .map(|entry| entry.0);
            if let Some(selected) = selected {
                messages.retain(|message| message.conv_id == selected);
                resolved = Some(selected);
            } else {
                messages.clear();
            }
        }
    }
    let query_lower = query.unwrap_or_default().to_lowercase();
    if !query_lower.is_empty() {
        messages.retain(|message| {
            let value = message.to_value().to_string().to_lowercase();
            value.contains(&query_lower)
        });
    }
    if let (Some(cursor), Some(order)) = (cursor, order) {
        let (epoch, message_id) = decode_cursor(cursor)?;
        messages.retain(|message| match order {
            Order::Asc => {
                (message.send_time_epoch, message.message_id.as_str())
                    > (epoch, message_id.as_str())
            }
            Order::Desc => {
                (message.send_time_epoch, message.message_id.as_str())
                    < (epoch, message_id.as_str())
            }
        });
    }
    if let Some(order) = order {
        messages.sort_by(|left, right| {
            let comparison = (left.send_time_epoch, &left.message_id)
                .cmp(&(right.send_time_epoch, &right.message_id));
            if order == Order::Asc {
                comparison
            } else {
                comparison.reverse()
            }
        });
    }
    let _ = schema_map;
    Ok((messages, resolved))
}

fn selected_account<'a>(cli: &'a Cli, config: &'a Config) -> Option<&'a str> {
    cli.account
        .as_deref()
        .or_else(|| (!config.default_account.is_empty()).then_some(config.default_account.as_str()))
}

fn parse_bound(value: Option<&str>, timezone: &str, end_of_day: bool) -> Result<Option<i64>> {
    let Some(value) = value else {
        return Ok(None);
    };
    let date_only = value.len() == 10;
    let naive = if date_only {
        NaiveDate::parse_from_str(value, "%Y-%m-%d")
            .map_err(|_| {
                OwcError::usage(format!(
                    "Invalid time `{value}`; use YYYY-MM-DD or YYYY-MM-DDTHH:MM:SS."
                ))
            })?
            .and_hms_opt(
                if end_of_day { 23 } else { 0 },
                if end_of_day { 59 } else { 0 },
                if end_of_day { 59 } else { 0 },
            )
            .ok_or_else(|| OwcError::usage(format!("Invalid time `{value}`.")))?
    } else {
        NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S")
            .or_else(|_| NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S"))
            .map_err(|_| {
                OwcError::usage(format!(
                    "Invalid time `{value}`; use YYYY-MM-DD or YYYY-MM-DDTHH:MM:SS."
                ))
            })?
    };
    let timestamp = if timezone == "local" {
        Local
            .from_local_datetime(&naive)
            .single()
            .or_else(|| Local.from_local_datetime(&naive).earliest())
            .ok_or_else(|| {
                OwcError::usage(format!("Time `{value}` is invalid in the local timezone."))
            })?
            .timestamp()
    } else {
        let tz: Tz = timezone
            .parse()
            .map_err(|_| OwcError::usage(format!("Unknown timezone `{timezone}`.")))?;
        tz.from_local_datetime(&naive)
            .single()
            .or_else(|| tz.from_local_datetime(&naive).earliest())
            .ok_or_else(|| {
                OwcError::usage(format!(
                    "Time `{value}` is invalid in timezone `{timezone}`."
                ))
            })?
            .timestamp()
    };
    Ok(Some(timestamp))
}

fn format_time(epoch: i64) -> String {
    chrono::DateTime::from_timestamp(epoch, 0)
        .map(|date| date.format("%Y-%m-%dT%H:%M:%S").to_string())
        .unwrap_or_default()
}

fn encode_cursor(message: &Message) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(format!(
        "{}:{}",
        message.send_time_epoch, message.message_id
    ))
}

fn decode_cursor(cursor: &str) -> Result<(i64, String)> {
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(cursor)
        .map_err(|_| OwcError::usage("Invalid message cursor."))?;
    let value =
        String::from_utf8(decoded).map_err(|_| OwcError::usage("Invalid message cursor."))?;
    let (time, id) = value
        .split_once(':')
        .ok_or_else(|| OwcError::usage("Invalid message cursor."))?;
    let epoch = time
        .parse::<i64>()
        .map_err(|_| OwcError::usage("Invalid message cursor."))?;
    Ok((epoch, id.to_owned()))
}

#[allow(dead_code)]
fn _unused_path_example() -> Option<PathBuf> {
    None
}
