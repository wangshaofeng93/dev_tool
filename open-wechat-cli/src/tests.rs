use crate::cipher::{self, CipherProfile};
use crate::cli::{
    Cli, ExportArgs, ExportFormat, ListArgs, MessageFilterArgs, MessageListArgs, MessagesCommand,
    Order,
};
use crate::config::Config;
use crate::output::{ErrorCode, OwcError};
use crate::state::State;
use chrono::NaiveDate;
use rusqlite::Connection;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

const KEY: [u8; 32] = [0x41; 32];

fn create_database_with_reserve(profile: CipherProfile) -> (TempDir, PathBuf, Vec<u8>) {
    let dir = tempfile::tempdir().expect("temporary directory creation should succeed");
    let path = dir.path().join("fixture.db");
    {
        let connection = Connection::open(&path).expect("fixture SQLite database should open");
        connection
            .execute_batch(
                "PRAGMA page_size=4096;
                 CREATE TABLE sample(value TEXT);
                 INSERT INTO sample VALUES ('round trip fixture');",
            )
            .expect("fixture data should be created");
    }
    let mut bytes = fs::read(&path).expect("fixture bytes should be readable");
    bytes[20] = profile.reserve() as u8;
    fs::write(&path, bytes).expect("reserve byte should be set");
    {
        let connection = Connection::open(&path).expect("reserved SQLite database should open");
        connection
            .execute_batch("VACUUM")
            .expect("SQLite should rebuild pages with reserved bytes");
    }
    let bytes = fs::read(&path).expect("rebuilt fixture bytes should be readable");
    (dir, path, bytes)
}

#[test]
fn decrypt_round_trip_supports_both_profiles() {
    for profile in CipherProfile::ALL {
        let (_dir, _path, plain) = create_database_with_reserve(profile);
        assert_eq!(plain.get(20).copied(), Some(profile.reserve() as u8));
        let encrypted = cipher::encrypt_db(&plain, &KEY, profile)
            .expect("test fixture encryption should succeed");
        let decrypted = cipher::decrypt_database(&encrypted, &KEY)
            .expect("test fixture decryption should succeed");
        assert_eq!(decrypted.profile, profile);
        assert_eq!(decrypted.bytes, plain);

        let dir = tempfile::tempdir().expect("temporary directory creation should succeed");
        let decrypted_path = dir.path().join("decrypted.db");
        fs::write(&decrypted_path, &decrypted.bytes).expect("decrypted fixture should be written");
        let connection =
            Connection::open(&decrypted_path).expect("rusqlite should open decrypted fixture");
        let check: String = connection
            .query_row("PRAGMA quick_check", [], |row| row.get(0))
            .expect("quick_check should execute");
        assert_eq!(check, "ok");
        let value: String = connection
            .query_row("SELECT value FROM sample", [], |row| row.get(0))
            .expect("decrypted table should be readable");
        assert_eq!(value, "round trip fixture");
    }
}

#[test]
fn wrong_key_and_plaintext_have_distinct_errors() {
    let (_dir, _path, plain) = create_database_with_reserve(CipherProfile::Sha1_64000);
    let encrypted = cipher::encrypt_db(&plain, &KEY, CipherProfile::Sha1_64000)
        .expect("test encryption should succeed");
    let error =
        cipher::decrypt_database(&encrypted, &[0x12; 32]).expect_err("wrong key must be rejected");
    assert!(matches!(error.code, ErrorCode::KeyNotFound));

    let error = cipher::decrypt_database(&plain, &KEY)
        .expect_err("plaintext must not be treated as ciphertext");
    assert!(matches!(error.code, ErrorCode::FormatMismatch));
}

fn create_query_fixture() -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("temporary directory creation should succeed");
    let path = dir.path().join("message.db");
    let connection = Connection::open(&path).expect("query fixture should open");
    connection
        .execute_batch(
            "CREATE TABLE messages(
                message_id TEXT PRIMARY KEY,
                conv_id TEXT,
                conv_name TEXT,
                sender_id TEXT,
                sender_name TEXT,
                send_time INTEGER,
                content_type INTEGER,
                content TEXT
            );
            CREATE TABLE sessions(conv_id TEXT, conv_name TEXT, last_activity INTEGER);
            CREATE TABLE contacts(id TEXT, name TEXT, alias TEXT);
            INSERT INTO sessions VALUES('R:team', 'Engineering', 1704193200);
            INSERT INTO contacts VALUES('u1', 'Alice', 'Al');
            INSERT INTO messages VALUES('m1','R:team','Engineering','u1','Alice',1704103200,1,'launch notes');
            INSERT INTO messages VALUES('m2','R:team','Engineering','u2','Bob',1704106800,1,'status update');
            INSERT INTO messages VALUES('m3','R:team','Engineering','u1','Alice',1704193200,1,'launch complete');",
        )
        .expect("query fixture schema/data should be created");
    drop(connection);
    (dir, path)
}

fn cli() -> Cli {
    Cli {
        json: true,
        verbose: false,
        account: None,
        tz: Some("UTC".to_owned()),
        command: crate::cli::Command::Doctor,
    }
}

fn state(dir: &Path) -> State {
    State {
        root: dir.join("state"),
    }
}

fn list_args(db_dir: &Path) -> MessageListArgs {
    MessageListArgs {
        filter: MessageFilterArgs {
            db_dir: Some(db_dir.to_path_buf()),
            conv: None,
            since: None,
            until: None,
        },
        query: None,
        limit: 100,
        cursor: None,
        order: Order::Asc,
        limit_per_conv: None,
        index: false,
        with_index: false,
        time_budget: None,
        max_hits: None,
        step: None,
    }
}

fn data(result: (String, Value, String)) -> Value {
    result.1
}

#[test]
fn fixture_queries_and_export_formats_cover_cli_contract() {
    let (dir, _db_path) = create_query_fixture();
    let state = state(dir.path());
    let config = Config::default();
    let cli = cli();

    let sessions = crate::store::sessions(
        ListArgs {
            db_dir: Some(dir.path().to_path_buf()),
            query: None,
            limit: 100,
            cursor: None,
            with_counts: true,
        },
        &cli,
        &config,
        &state,
    )
    .expect("sessions list should query fixture");
    let sessions = data(sessions);
    assert_eq!(sessions["items"].as_array().map(Vec::len), Some(1));
    assert_eq!(sessions["items"][0]["message_count"], 3);

    let contacts = crate::store::contacts(
        ListArgs {
            db_dir: Some(dir.path().to_path_buf()),
            query: Some("Alice".to_owned()),
            limit: 10,
            cursor: None,
            with_counts: false,
        },
        &cli,
        &config,
        &state,
    )
    .expect("contacts list should query fixture");
    assert_eq!(data(contacts)["items"].as_array().map(Vec::len), Some(1));

    let stats = crate::query::messages(
        MessagesCommand::Stats(MessageFilterArgs {
            db_dir: Some(dir.path().to_path_buf()),
            conv: None,
            since: None,
            until: None,
        }),
        &cli,
        &config,
        &state,
    )
    .expect("messages stats should query fixture");
    assert_eq!(data(stats)["total"], 3);

    let mut query_args = list_args(dir.path());
    query_args.query = Some("launch".to_owned());
    query_args.filter.since = Some("2024-01-01".to_owned());
    query_args.filter.until = Some("2024-01-02".to_owned());
    let searched = crate::query::messages(MessagesCommand::List(query_args), &cli, &config, &state)
        .expect("filtered messages list should query fixture");
    assert_eq!(data(searched)["items"].as_array().map(Vec::len), Some(2));

    let mut conversation = list_args(dir.path());
    conversation.filter.conv = Some("Engineering".to_owned());
    conversation.limit = 2;
    let conversation_page = crate::query::messages(
        MessagesCommand::List(conversation.clone()),
        &cli,
        &config,
        &state,
    )
    .expect("conversation-name resolution should query fixture");
    let conversation_page = data(conversation_page);
    assert_eq!(conversation_page["resolved_conv"], "R:team");
    assert_eq!(conversation_page["items"].as_array().map(Vec::len), Some(2));
    let cursor = conversation_page["next_cursor"]
        .as_str()
        .expect("conversation page should expose a cursor")
        .to_owned();
    conversation.cursor = Some(cursor);
    let conversation_next =
        crate::query::messages(MessagesCommand::List(conversation), &cli, &config, &state)
            .expect("conversation cursor should apply in SQL");
    assert_eq!(data(conversation_next)["items"][0]["message_id"], "m3");

    let mut descending = list_args(dir.path());
    descending.order = Order::Desc;
    descending.limit = 2;
    let first_page = crate::query::messages(
        MessagesCommand::List(descending.clone()),
        &cli,
        &config,
        &state,
    )
    .expect("first message page should be returned");
    assert_eq!(data(first_page.clone())["items"][0]["message_id"], "m3");
    let cursor = data(first_page)["next_cursor"]
        .as_str()
        .expect("first page should include a cursor")
        .to_owned();
    descending.cursor = Some(cursor);
    let second_page =
        crate::query::messages(MessagesCommand::List(descending), &cli, &config, &state)
            .expect("second message page should be returned");
    assert_eq!(data(second_page)["items"].as_array().map(Vec::len), Some(1));

    let since = NaiveDate::from_ymd_opt(2024, 1, 2)
        .expect("fixture date should be valid")
        .format("%Y-%m-%d")
        .to_string();
    let mut date_filter = list_args(dir.path());
    date_filter.filter.since = Some(since);
    date_filter.order = Order::Asc;
    let dated = crate::query::messages(MessagesCommand::List(date_filter), &cli, &config, &state)
        .expect("since filter should be accepted");
    assert_eq!(data(dated)["items"].as_array().map(Vec::len), Some(1));

    for format in [
        ExportFormat::Jsonl,
        ExportFormat::Txt,
        ExportFormat::Md,
        ExportFormat::Html,
    ] {
        let mut list = list_args(dir.path());
        list.limit = 10;
        let export = crate::query::messages(
            MessagesCommand::Export(ExportArgs {
                list,
                format,
                out: Some(dir.path().join(format!("export-{format:?}"))),
                copy_media: false,
                decode_voice: false,
                media_out: None,
                silk_decoder: None,
            }),
            &cli,
            &config,
            &state,
        )
        .expect("each export format should succeed");
        assert_eq!(data(export)["count"], 3);
        match format {
            ExportFormat::Jsonl => {
                assert!(dir.path().join("export-Jsonl/messages.jsonl").is_file())
            }
            ExportFormat::Txt => assert!(dir.path().join("export-Txt/R_team.txt").is_file()),
            ExportFormat::Md | ExportFormat::Html => {}
        }
    }
    let markdown = fs::read_to_string(dir.path().join("export-Md/messages.md"))
        .expect("markdown export should exist");
    assert!(markdown.contains("class=\"day\""));
    let html = fs::read_to_string(dir.path().join("export-Html/messages.html"))
        .expect("html export should exist");
    for class in [
        ".msg", ".avatar", ".body", ".who", ".chip", ".text", ".day", ".meta",
    ] {
        assert!(html.contains(class), "missing export CSS class {class}");
    }
    assert!(html.matches("class=\"day\"").count() >= 2);
}

#[test]
fn sql_time_filters_parse_text_timestamps() {
    let (dir, db_path) = create_query_fixture();
    let connection = Connection::open(db_path).expect("query fixture should open");
    connection
        .execute(
            "UPDATE messages SET send_time = ?1 WHERE message_id = ?2",
            ("2024-01-01 11:00:00", "m2"),
        )
        .expect("text timestamp should be stored");
    drop(connection);

    let mut args = list_args(dir.path());
    args.filter.since = Some("2024-01-01T10:30:00".to_owned());
    args.filter.until = Some("2024-01-01T11:30:00".to_owned());
    let result = crate::query::messages(
        MessagesCommand::List(args),
        &cli(),
        &Config::default(),
        &state(dir.path()),
    )
    .expect("text timestamps should be queryable");
    let result = data(result);
    assert_eq!(result["total"], 1);
    assert_eq!(result["items"][0]["message_id"], "m2");
    assert_eq!(result["items"][0]["send_time"], "2024-01-01T11:00:00");
}

#[test]
fn entity_query_matches_only_display_name_fields() {
    let (dir, _db_path) = create_query_fixture();
    let state = state(dir.path());
    let config = Config::default();
    let cli = cli();

    let contacts_by_id = crate::store::contacts(
        ListArgs {
            db_dir: Some(dir.path().to_path_buf()),
            query: Some("u1".to_owned()),
            limit: 10,
            cursor: None,
            with_counts: false,
        },
        &cli,
        &config,
        &state,
    )
    .expect("contact query should succeed");
    assert_eq!(
        data(contacts_by_id)["items"].as_array().map(Vec::len),
        Some(0)
    );

    let sessions_by_activity = crate::store::sessions(
        ListArgs {
            db_dir: Some(dir.path().to_path_buf()),
            query: Some("1704193200".to_owned()),
            limit: 10,
            cursor: None,
            with_counts: false,
        },
        &cli,
        &config,
        &state,
    )
    .expect("session query should succeed");
    assert_eq!(
        data(sessions_by_activity)["items"].as_array().map(Vec::len),
        Some(0)
    );

    let sessions_by_name = crate::store::sessions(
        ListArgs {
            db_dir: Some(dir.path().to_path_buf()),
            query: Some("Engineering".to_owned()),
            limit: 10,
            cursor: None,
            with_counts: false,
        },
        &cli,
        &config,
        &state,
    )
    .expect("display-name query should succeed");
    assert_eq!(
        data(sessions_by_name)["items"].as_array().map(Vec::len),
        Some(1)
    );
}

#[test]
fn message_and_stats_timestamps_follow_selected_timezone() {
    let (dir, _db_path) = create_query_fixture();
    let state = state(dir.path());
    let mut config = Config::default();
    let mut cli = cli();
    cli.tz = Some("Asia/Shanghai".to_owned());

    let listed = crate::query::messages(
        MessagesCommand::List(list_args(dir.path())),
        &cli,
        &config,
        &state,
    )
    .expect("messages list should format fixture timestamps");
    let stats = crate::query::messages(
        MessagesCommand::Stats(MessageFilterArgs {
            db_dir: Some(dir.path().to_path_buf()),
            conv: None,
            since: None,
            until: None,
        }),
        &cli,
        &config,
        &state,
    )
    .expect("messages stats should format fixture timestamps");
    assert_eq!(data(listed)["items"][0]["send_time"], "2024-01-01T18:00:00");
    assert_eq!(data(stats)["first_time"], "2024-01-01T18:00:00");

    cli.tz = Some("UTC".to_owned());
    let utc = crate::query::messages(
        MessagesCommand::List(list_args(dir.path())),
        &cli,
        &config,
        &state,
    )
    .expect("UTC message formatting should succeed");
    assert_eq!(data(utc)["items"][0]["send_time"], "2024-01-01T10:00:00");

    cli.tz = None;
    config.tz = "Asia/Tokyo".to_owned();
    let configured = crate::query::messages(
        MessagesCommand::List(list_args(dir.path())),
        &cli,
        &config,
        &state,
    )
    .expect("configured timezone should format timestamps");
    assert_eq!(
        data(configured)["items"][0]["send_time"],
        "2024-01-01T19:00:00"
    );

    cli.tz = Some("UTC".to_owned());
    let overridden = crate::query::messages(
        MessagesCommand::List(list_args(dir.path())),
        &cli,
        &config,
        &state,
    )
    .expect("CLI timezone should override configuration");
    assert_eq!(
        data(overridden)["items"][0]["send_time"],
        "2024-01-01T10:00:00"
    );
}

#[test]
fn extra_config_fields_are_rejected_as_usage() {
    let error = serde_json::from_str::<Config>(r#"{"default_account":"","unexpected":true}"#)
        .expect_err("unknown config fields must be rejected");
    let usage = OwcError::usage(error.to_string());
    assert!(matches!(usage.code, ErrorCode::Usage));
}

#[test]
fn recalled_content_is_extracted_from_json_and_xml_candidates() {
    let json = crate::codec::decode_content(49, r#"{"revoker_id":"u9","revoked_at":1704103200}"#);
    let recalled = json
        .recalled
        .expect("JSON recall fields should be extracted");
    assert_eq!(recalled.revoker_id.as_deref(), Some("u9"));
    assert_eq!(recalled.revoked_at.as_deref(), Some("1704103200"));

    let xml = crate::codec::decode_content(
        49,
        "<revoker_id>u8</revoker_id><revoked_at>2024-01-01T10:00:00Z</revoked_at>",
    );
    let recalled = xml.recalled.expect("XML recall fields should be extracted");
    assert_eq!(recalled.revoker_id.as_deref(), Some("u8"));
    assert_eq!(recalled.revoked_at.as_deref(), Some("2024-01-01T10:00:00Z"));
}
