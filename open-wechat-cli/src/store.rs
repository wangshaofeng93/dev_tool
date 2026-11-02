use crate::cache;
use crate::cli::{Cli, ListArgs};
use crate::config::Config;
use crate::output::{OwcError, Result};
use crate::schema::{self, SchemaMap, TableMapping};
use crate::state::State;
use base64::Engine;
use rusqlite::{types::ValueRef, Connection};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Message {
    pub message_id: String,
    pub conv_id: String,
    pub conv_name: String,
    pub sender_id: String,
    pub sender_name: String,
    pub send_time: String,
    pub send_time_epoch: i64,
    pub content_type: i64,
    pub type_label: String,
    pub text: String,
    pub extra: crate::codec::DecodedContent,
}

impl Message {
    pub fn to_value(&self) -> Value {
        let mut value = json!({
            "message_id": self.message_id,
            "conv_id": self.conv_id,
            "conv_name": self.conv_name,
            "sender_id": self.sender_id,
            "sender_name": self.sender_name,
            "send_time": self.send_time,
            "send_time_epoch": self.send_time_epoch,
            "content_type": self.content_type,
            "type_label": self.type_label,
            "text": self.text,
        });
        if let Some(object) = value.as_object_mut() {
            if !self.extra.media.is_empty() {
                object.insert("media".to_owned(), json!(self.extra.media));
            }
            if let Some(link) = &self.extra.link {
                object.insert("link".to_owned(), json!(link));
            }
            if let Some(forward) = &self.extra.forward {
                object.insert("forward".to_owned(), json!(forward));
            }
            if let Some(quote) = &self.extra.quote {
                object.insert("quote".to_owned(), json!(quote));
            }
        }
        value
    }
}

pub fn sessions(
    command: ListArgs,
    cli: &Cli,
    _config: &Config,
    state: &State,
) -> Result<(String, Value, String)> {
    list_entities("sessions list", command, cli, state, false)
}

pub fn contacts(
    command: ListArgs,
    cli: &Cli,
    _config: &Config,
    state: &State,
) -> Result<(String, Value, String)> {
    list_entities("contacts list", command, cli, state, true)
}

fn list_entities(
    label: &str,
    args: ListArgs,
    cli: &Cli,
    state: &State,
    contacts: bool,
) -> Result<(String, Value, String)> {
    if args.limit == 0 {
        return Err(OwcError::usage("--limit must be greater than zero."));
    }
    let dir = cache::resolve_db_dir(args.db_dir.as_deref(), cli.account.as_deref(), state)?;
    let connection = open_directory(&dir, contacts)?;
    let schema_map = schema::introspect(&connection)?;
    let mapping = if contacts {
        schema_map.contact
    } else {
        schema_map.session
    }
    .ok_or_else(|| {
        OwcError::data(if contacts {
            "Could not identify a contacts table."
        } else {
            "Could not identify a sessions table."
        })
    })?;
    let mut rows = select_rows(&connection, &mapping)?;
    if args.with_counts && !contacts {
        let message_mapping = schema::introspect(&connection)?.message;
        if let Some(message_mapping) = message_mapping {
            if let Some(message_conv) = message_mapping.conv {
                let sql = format!(
                    "SELECT {}, COUNT(*) FROM {} GROUP BY {}",
                    quote_id(&message_conv),
                    quote_id(&message_mapping.table),
                    quote_id(&message_conv)
                );
                let mut statement = connection
                    .prepare(&sql)
                    .map_err(|error| OwcError::data(error.to_string()))?;
                let counts = statement
                    .query_map([], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                    })
                    .map_err(|error| OwcError::data(error.to_string()))?
                    .collect::<std::result::Result<std::collections::HashMap<_, _>, _>>()
                    .map_err(|error| OwcError::data(error.to_string()))?;
                for row in &mut rows {
                    if let Some(object) = row.as_object_mut() {
                        let conversation =
                            object.get("id").and_then(Value::as_str).unwrap_or_default();
                        object.insert(
                            "message_count".to_owned(),
                            json!(counts.get(conversation).copied().unwrap_or_default()),
                        );
                    }
                }
            }
        }
    }
    let needle = args.query.unwrap_or_default().to_lowercase();
    let mut rows: Vec<Value> = rows
        .into_iter()
        .filter(|value| needle.is_empty() || value.to_string().to_lowercase().contains(&needle))
        .collect();
    let offset = decode_offset(args.cursor.as_deref())?;
    let total = rows.len();
    let next = offset.saturating_add(args.limit);
    let page = rows
        .drain(..)
        .skip(offset)
        .take(args.limit)
        .collect::<Vec<_>>();
    let next_cursor = if next < total {
        Some(encode_offset(next))
    } else {
        None
    };
    Ok(crate::output::result(
        label,
        json!({"items": page, "next_cursor": next_cursor}),
    ))
}

fn select_rows(connection: &Connection, mapping: &TableMapping) -> Result<Vec<Value>> {
    let columns = table_columns(connection, &mapping.table)?;
    let id = mapping
        .id
        .as_deref()
        .or_else(|| columns.first().map(String::as_str));
    let name = mapping
        .conv_name
        .as_deref()
        .or(mapping.alias.as_deref())
        .or_else(|| columns.get(1).map(String::as_str));
    let Some(id) = id else {
        return Ok(Vec::new());
    };
    let Some(name) = name else {
        return Ok(Vec::new());
    };
    let mut selected_names = vec![id.to_owned(), name.to_owned()];
    for candidate in ["last_activity", "last_time", "remark", "alias", "nickname"] {
        if let Some(column) = columns
            .iter()
            .find(|column| column.eq_ignore_ascii_case(candidate))
        {
            if !selected_names
                .iter()
                .any(|name| name.eq_ignore_ascii_case(column))
            {
                selected_names.push(column.clone());
            }
        }
    }
    let sql = format!(
        "SELECT {} FROM {}",
        selected_names
            .iter()
            .map(|column| quote_id(column))
            .collect::<Vec<_>>()
            .join(", "),
        quote_id(&mapping.table)
    );
    let mut statement = connection
        .prepare(&sql)
        .map_err(|error| OwcError::data(error.to_string()))?;
    let rows = statement
        .query_map([], |row| {
            let mut object = Map::new();
            for (index, column) in selected_names.iter().enumerate() {
                let key = match index {
                    0 => "id",
                    1 => "name",
                    _ => column.as_str(),
                };
                object.insert(key.to_owned(), value_from_ref(row.get_ref(index)?));
            }
            Ok(Value::Object(object))
        })
        .map_err(|error| OwcError::data(error.to_string()))?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|error| OwcError::data(error.to_string()))
}

pub fn read_messages(
    dir: &Path,
    since: Option<i64>,
    until: Option<i64>,
) -> Result<(Vec<Message>, SchemaMap)> {
    let connection = open_directory(dir, false)?;
    let map = schema::introspect(&connection)?;
    let mapping = map
        .message
        .as_ref()
        .ok_or_else(|| OwcError::data("Could not identify a messages table."))?;
    let messages = query_message_rows(&connection, mapping, since, until)?;
    Ok((messages, map))
}

fn query_message_rows(
    connection: &Connection,
    mapping: &TableMapping,
    since: Option<i64>,
    until: Option<i64>,
) -> Result<Vec<Message>> {
    let columns = table_columns(connection, &mapping.table)?;
    let pick = |column: Option<&str>, candidates: &[&str]| -> Option<String> {
        column.map(str::to_owned).or_else(|| {
            candidates.iter().find_map(|candidate| {
                columns
                    .iter()
                    .find(|column| column.eq_ignore_ascii_case(candidate))
                    .cloned()
            })
        })
    };
    let expressions = [
        pick(
            mapping.id.as_deref(),
            &["message_id", "msg_id", "id", "local_id"],
        ),
        pick(
            mapping.conv.as_deref(),
            &["conv_id", "conversation_id", "chat_id", "session_id"],
        ),
        pick(
            mapping.conv_name.as_deref(),
            &["conv_name", "conversation_name", "chat_name"],
        ),
        pick(
            mapping.sender.as_deref(),
            &["sender_id", "from_user", "from_id"],
        ),
        pick(
            mapping.sender_name.as_deref(),
            &["sender_name", "from_name"],
        ),
        pick(
            mapping.time.as_deref(),
            &["send_time", "msg_time", "create_time", "timestamp", "time"],
        ),
        pick(
            mapping.content_type.as_deref(),
            &["content_type", "msg_type", "type", "message_type"],
        ),
        pick(
            mapping.content.as_deref(),
            &["content", "message_content", "body", "text"],
        ),
    ];
    let required = expressions[5].is_some() && expressions[7].is_some();
    if !required {
        return Err(OwcError::data(
            "Message table is missing time or content columns.",
        ));
    }
    let sql = format!(
        "SELECT {} FROM {}",
        expressions
            .iter()
            .map(|expression| expression
                .as_deref()
                .map(quote_id)
                .unwrap_or_else(|| "NULL".to_owned()))
            .collect::<Vec<_>>()
            .join(", "),
        quote_id(&mapping.table)
    );
    let mut statement = connection
        .prepare(&sql)
        .map_err(|error| OwcError::data(error.to_string()))?;
    let rows = statement
        .query_map([], |row| {
            let time_value = value_from_ref(row.get_ref(5)?);
            let epoch = parse_epoch(&time_value);
            let raw_content_type = value_from_ref(row.get_ref(6)?);
            let content_type = raw_content_type.as_i64().unwrap_or(0);
            let raw = value_from_ref(row.get_ref(7)?)
                .as_str()
                .unwrap_or_default()
                .to_owned();
            let decoded = crate::codec::decode_content(content_type, &raw);
            Ok(Message {
                message_id: value_from_ref(row.get_ref(0)?).to_string_value(),
                conv_id: value_from_ref(row.get_ref(1)?).to_string_value(),
                conv_name: value_from_ref(row.get_ref(2)?).to_string_value(),
                sender_id: value_from_ref(row.get_ref(3)?).to_string_value(),
                sender_name: value_from_ref(row.get_ref(4)?).to_string_value(),
                send_time: format_epoch(epoch),
                send_time_epoch: epoch,
                content_type,
                type_label: decoded.type_label.clone(),
                text: decoded.text.clone(),
                extra: decoded,
            })
        })
        .map_err(|error| OwcError::data(error.to_string()))?;
    let mut messages = rows
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|error| OwcError::data(error.to_string()))?;
    messages.retain(|message| {
        since.is_none_or(|start| message.send_time_epoch >= start)
            && until.is_none_or(|end| message.send_time_epoch <= end)
    });
    Ok(messages)
}

fn open_directory(dir: &Path, contacts: bool) -> Result<Connection> {
    let path = if dir.is_file() {
        dir.to_path_buf()
    } else {
        database_path(dir, contacts)?
    };
    cache::open_read_only(&path)
}

fn database_path(dir: &Path, contacts: bool) -> Result<PathBuf> {
    let names = if contacts {
        ["user.db", "session.db", "message.db"]
    } else {
        ["session.db", "message.db", "user.db"]
    };
    for name in names {
        let path = dir.join(name);
        if path.is_file() {
            return Ok(path);
        }
    }
    Err(OwcError::data(format!(
        "No recognized SQLite database exists in {}.",
        dir.display()
    )))
}

fn table_columns(connection: &Connection, table: &str) -> Result<Vec<String>> {
    let sql = format!("PRAGMA table_info({})", quote_id(table));
    let mut statement = connection
        .prepare(&sql)
        .map_err(|error| OwcError::data(error.to_string()))?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| OwcError::data(error.to_string()))?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|error| OwcError::data(error.to_string()))
}

pub fn quote_id(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

fn value_from_ref(value: ValueRef<'_>) -> Value {
    match value {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(value) => json!(value),
        ValueRef::Real(value) => json!(value),
        ValueRef::Text(value) => json!(String::from_utf8_lossy(value).to_string()),
        ValueRef::Blob(value) => json!(base64::engine::general_purpose::STANDARD.encode(value)),
    }
}

trait ValueString {
    fn to_string_value(&self) -> String;
}

impl ValueString for Value {
    fn to_string_value(&self) -> String {
        match self {
            Value::String(value) => value.clone(),
            Value::Null => String::new(),
            _ => self.to_string(),
        }
    }
}

fn parse_epoch(value: &Value) -> i64 {
    if let Some(number) = value.as_i64() {
        if number.abs() >= 1_000_000_000_000 {
            number / 1000
        } else {
            number
        }
    } else if let Some(text) = value.as_str() {
        chrono::DateTime::parse_from_rfc3339(text)
            .map(|date| date.timestamp())
            .or_else(|_| {
                chrono::NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S")
                    .map(|date| date.and_utc().timestamp())
            })
            .unwrap_or(0)
    } else {
        0
    }
}

fn format_epoch(epoch: i64) -> String {
    chrono::DateTime::from_timestamp(epoch, 0)
        .map(|date| date.format("%Y-%m-%dT%H:%M:%S").to_string())
        .unwrap_or_default()
}

fn decode_offset(cursor: Option<&str>) -> Result<usize> {
    cursor
        .map(|cursor| {
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(cursor)
                .map_err(|_| OwcError::usage("Invalid cursor."))
                .and_then(|bytes| {
                    String::from_utf8(bytes)
                        .map_err(|_| OwcError::usage("Invalid cursor."))
                        .and_then(|value| {
                            value
                                .parse::<usize>()
                                .map_err(|_| OwcError::usage("Invalid cursor."))
                        })
                })
        })
        .unwrap_or(Ok(0))
}

fn encode_offset(offset: usize) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(offset.to_string())
}
