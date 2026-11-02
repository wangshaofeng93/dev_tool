use crate::cache;
use crate::cli::{Cli, SchemaArgs};
use crate::config::Config;
use crate::output::{OwcError, Result};
use crate::state::State;
use rusqlite::Connection;
use serde::Serialize;
use serde_json::{json, Value};
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct ColumnInfo {
    pub name: String,
    pub data_type: String,
    pub not_null: bool,
    pub primary_key: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TableInfo {
    pub name: String,
    pub kind: String,
    pub columns: Vec<ColumnInfo>,
}

#[derive(Debug, Clone, Default)]
pub struct TableMapping {
    pub table: String,
    pub id: Option<String>,
    pub conv: Option<String>,
    pub conv_name: Option<String>,
    pub sender: Option<String>,
    pub sender_name: Option<String>,
    pub time: Option<String>,
    pub content_type: Option<String>,
    pub content: Option<String>,
    pub alias: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct SchemaMap {
    pub tables: Vec<TableInfo>,
    pub message: Option<TableMapping>,
    pub session: Option<TableMapping>,
    pub contact: Option<TableMapping>,
}

pub fn introspect(connection: &Connection) -> Result<SchemaMap> {
    let mut tables = Vec::new();
    let mut statement = connection
        .prepare("PRAGMA table_list")
        .map_err(|error| OwcError::data(error.to_string()))?;
    let table_rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(1)?, row.get::<_, String>(2)?))
        })
        .map_err(|error| OwcError::data(error.to_string()))?;
    for row in table_rows {
        let (name, kind) = row.map_err(|error| OwcError::data(error.to_string()))?;
        if name.starts_with("sqlite_") {
            continue;
        }
        let escaped = name.replace('"', "\"\"");
        let mut columns = Vec::new();
        let pragma = format!("PRAGMA table_info(\"{escaped}\")");
        let mut column_statement = connection
            .prepare(&pragma)
            .map_err(|error| OwcError::data(error.to_string()))?;
        let column_rows = column_statement
            .query_map([], |row| {
                Ok(ColumnInfo {
                    name: row.get(1)?,
                    data_type: row.get(2)?,
                    not_null: row.get::<_, i64>(3)? != 0,
                    primary_key: row.get(5)?,
                })
            })
            .map_err(|error| OwcError::data(error.to_string()))?;
        for column in column_rows {
            columns.push(column.map_err(|error| OwcError::data(error.to_string()))?);
        }
        tables.push(TableInfo {
            name,
            kind,
            columns,
        });
    }
    let message = best_mapping(&tables, TableRole::Message);
    let session = best_mapping(&tables, TableRole::Session);
    let contact = best_mapping(&tables, TableRole::Contact);
    Ok(SchemaMap {
        tables,
        message,
        session,
        contact,
    })
}

#[derive(Clone, Copy)]
enum TableRole {
    Message,
    Session,
    Contact,
}

fn best_mapping(tables: &[TableInfo], role: TableRole) -> Option<TableMapping> {
    // S3: Candidate names and table-name bonuses are heuristic until checked against WXWork 5.x files.
    let mut scored: Vec<(i32, &TableInfo)> = tables
        .iter()
        .map(|table| {
            let columns: Vec<String> = table
                .columns
                .iter()
                .map(|column| column.name.to_ascii_lowercase())
                .collect();
            let has = |candidates: &[&str]| {
                candidates
                    .iter()
                    .any(|candidate| columns.iter().any(|name| name == candidate))
            };
            let (required, name_score) = match role {
                TableRole::Message => (
                    has(&["content", "message_content", "body"])
                        && has(&["send_time", "msg_time", "create_time", "timestamp"]),
                    if table.name.to_ascii_lowercase().contains("message") {
                        3
                    } else {
                        0
                    },
                ),
                TableRole::Session => (
                    has(&["conv_id", "conversation_id", "chat_id"])
                        && has(&["conv_name", "name", "conversation_name"]),
                    if table.name.to_ascii_lowercase().contains("session") {
                        3
                    } else {
                        0
                    },
                ),
                TableRole::Contact => (
                    has(&["name", "display_name", "nickname"])
                        && has(&["id", "user_id", "userid", "contact_id"]),
                    if table.name.to_ascii_lowercase().contains("contact")
                        || table.name.to_ascii_lowercase().contains("user")
                    {
                        3
                    } else {
                        0
                    },
                ),
            };
            let score = if required {
                10 + name_score
            } else {
                name_score
            };
            (score, table)
        })
        .filter(|(score, _)| *score >= 10)
        .collect();
    scored.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    let table = scored.first()?.1;
    let pick = |candidates: &[&str]| -> Option<String> {
        candidates.iter().find_map(|candidate| {
            table
                .columns
                .iter()
                .find(|column| column.name.eq_ignore_ascii_case(candidate))
                .map(|column| column.name.clone())
        })
    };
    Some(TableMapping {
        table: table.name.clone(),
        id: pick(&[
            "message_id",
            "msg_id",
            "id",
            "local_id",
            "user_id",
            "userid",
            "contact_id",
        ]),
        conv: pick(&["conv_id", "conversation_id", "chat_id", "session_id"]),
        conv_name: pick(&["conv_name", "conversation_name", "chat_name", "name"]),
        sender: pick(&["sender_id", "from_user", "from_id", "user_id"]),
        sender_name: pick(&["sender_name", "from_name", "nickname"]),
        time: pick(&["send_time", "msg_time", "create_time", "timestamp", "time"]),
        content_type: pick(&["content_type", "msg_type", "type", "message_type"]),
        content: pick(&["content", "message_content", "body", "text"]),
        alias: pick(&["alias", "remark", "nickname", "display_name"]),
    })
}

pub fn run(
    args: SchemaArgs,
    cli: &Cli,
    _config: &Config,
    state: &State,
) -> Result<(String, Value, String)> {
    let path = cache::resolve_db_dir(args.db_dir.as_deref(), cli.account.as_deref(), state)?;
    let db_path = if let Some(db) = args.db {
        path.join(db)
    } else {
        find_database(&path)?
    };
    let connection = cache::open_read_only(&db_path)?;
    let map = introspect(&connection)?;
    Ok(crate::output::result(
        "schema",
        json!({"database": db_path, "tables": map.tables}),
    ))
}

pub fn find_database(dir: &Path) -> Result<std::path::PathBuf> {
    for name in ["message.db", "session.db", "user.db"] {
        let path = dir.join(name);
        if path.is_file() {
            return Ok(path);
        }
    }
    Err(OwcError::data(format!(
        "No message.db, session.db, or user.db found in {}.",
        dir.display()
    )))
}
