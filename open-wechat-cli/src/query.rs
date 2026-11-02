use crate::cache;
use crate::cli::{
    Cli, ExportArgs, HistoryArgs, MessageFilterArgs, MessageListArgs, MessagesCommand, Order,
};
use crate::config::Config;
use crate::output::{OwcError, Result};
use crate::state::State;
use crate::store::{self, Message};
use base64::Engine;
use serde_json::{json, Value};

#[derive(Default)]
struct FetchOptions<'a> {
    query: Option<&'a str>,
    order: Option<Order>,
    cursor: Option<&'a str>,
    page_limit: Option<usize>,
    per_conv_limit: Option<usize>,
}

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
    let timezone = crate::timezone::resolve(cli, config)?;
    let since = crate::timezone::parse_bound(args.since.as_deref(), timezone, false)?;
    let until = crate::timezone::parse_bound(args.until.as_deref(), timezone, true)?;
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
    let timezone = crate::timezone::resolve(cli, config)?;
    let (messages, resolved, _) =
        fetch_messages(&args, FetchOptions::default(), cli, config, state)?;
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
        "first_time": first.map(|epoch| crate::timezone::format_epoch(epoch, timezone)),
        "last_time": last.map(|epoch| crate::timezone::format_epoch(epoch, timezone)),
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
    if args.limit_per_conv == Some(0) {
        return Err(OwcError::usage(
            "--limit-per-conv must be greater than zero.",
        ));
    }
    let (messages, resolved, total) = fetch_messages(
        &args.filter,
        FetchOptions {
            query: args.query.as_deref(),
            order: Some(args.order),
            cursor: args.cursor.as_deref(),
            page_limit: Some(args.limit),
            per_conv_limit: args.limit_per_conv,
        },
        cli,
        config,
        state,
    )?;
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
    let (mut messages, resolved, _) = fetch_messages(
        &args.list.filter,
        FetchOptions {
            query: args.list.query.as_deref(),
            order: Some(args.list.order),
            cursor: args.list.cursor.as_deref(),
            page_limit: Some(args.list.limit),
            per_conv_limit: args.list.limit_per_conv,
        },
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
    options: FetchOptions<'_>,
    cli: &Cli,
    config: &Config,
    state: &State,
) -> Result<(Vec<Message>, Option<String>, usize)> {
    let timezone = crate::timezone::resolve(cli, config)?;
    let since = crate::timezone::parse_bound(filter.since.as_deref(), timezone, false)?;
    let until = crate::timezone::parse_bound(filter.until.as_deref(), timezone, true)?;
    if let (Some(since), Some(until)) = (since, until) {
        if since > until {
            return Err(OwcError::usage("--since must not be later than --until."));
        }
    }
    let account = selected_account(cli, config);
    let dir = cache::resolve_db_dir(filter.db_dir.as_deref(), account, state)?;
    let resolved = filter
        .conv
        .as_deref()
        .map(|needle| store::resolve_conversation(&dir, since, until, needle))
        .transpose()?
        .flatten();
    if filter.conv.is_some() && resolved.is_none() {
        return Ok((Vec::new(), None, 0));
    }
    let cursor_key = options
        .cursor
        .map(decode_cursor)
        .transpose()?
        .filter(|_| options.order.is_some());
    let query_lower = options.query.unwrap_or_default().to_lowercase();
    let Some(page_limit) = options.page_limit else {
        let query = store::MessageQuery {
            since,
            until,
            conv_id: resolved.clone(),
            order: None,
            cursor: None,
            limit: None,
            timezone,
        };
        let messages = store::read_messages(&dir, &query)?;
        let messages = filter_text(messages, &query_lower);
        let total = messages.len();
        return Ok((messages, resolved, total));
    };
    let sql_limit = page_limit.saturating_add(1);
    let mut query = store::MessageQuery {
        since,
        until,
        conv_id: resolved.clone(),
        order: options.order,
        cursor: cursor_key,
        limit: Some(sql_limit),
        timezone,
    };
    if query_lower.is_empty() && options.per_conv_limit.is_none() {
        let total = store::count_messages(&dir, &query)?;
        let messages = store::read_messages(&dir, &query)?;
        return Ok((messages, resolved, total));
    }
    query.order = Some(options.order.ok_or_else(|| {
        OwcError::new(
            crate::output::ErrorCode::Internal,
            "Paged message queries require an order.",
            "Report this unexpected query state.",
        )
    })?);
    let mut conv_counts = std::collections::HashMap::<String, usize>::new();
    let (messages, total) = store::scan_messages(&dir, &query, sql_limit, sql_limit, |message| {
        if !query_matches(message, &query_lower) {
            return false;
        }
        if let Some(per_conv) = options.per_conv_limit {
            let count = conv_counts.entry(message.conv_id.clone()).or_default();
            if *count >= per_conv {
                return false;
            }
            *count += 1;
        }
        true
    })?;
    Ok((messages, resolved, total))
}

fn filter_text(messages: Vec<Message>, query: &str) -> Vec<Message> {
    if query.is_empty() {
        messages
    } else {
        messages
            .into_iter()
            .filter(|message| query_matches(message, query))
            .collect()
    }
}

fn query_matches(message: &Message, query: &str) -> bool {
    query.is_empty()
        || message
            .to_value()
            .to_string()
            .to_lowercase()
            .contains(query)
}

fn selected_account<'a>(cli: &'a Cli, config: &'a Config) -> Option<&'a str> {
    cli.account
        .as_deref()
        .or_else(|| (!config.default_account.is_empty()).then_some(config.default_account.as_str()))
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
