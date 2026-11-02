use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "owc",
    version,
    about = "Read-only local WXWork history reconstruction"
)]
pub struct Cli {
    #[arg(long, global = true)]
    pub json: bool,
    #[arg(short, long, global = true)]
    pub verbose: bool,
    #[arg(long, global = true)]
    pub account: Option<String>,
    #[arg(long, global = true)]
    pub tz: Option<String>,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Doctor,
    Accounts {
        #[arg(short, long)]
        verbose: bool,
    },
    Key {
        #[command(subcommand)]
        command: KeyCommand,
    },
    Snapshot(SnapshotArgs),
    Decrypt(DecryptArgs),
    Schema(SchemaArgs),
    Sessions {
        #[command(subcommand)]
        command: ListCommand,
    },
    Contacts {
        #[command(subcommand)]
        command: ListCommand,
    },
    Messages {
        #[command(subcommand)]
        command: MessagesCommand,
    },
    Media {
        #[command(subcommand)]
        command: MediaCommand,
    },
    History(HistoryArgs),
}

#[derive(Debug, Subcommand)]
pub enum KeyCommand {
    Scan(KeyScanArgs),
    Verify(KeyVerifyArgs),
}

#[derive(Debug, Args)]
pub struct KeyScanArgs {
    #[arg(long)]
    pub account: String,
    #[arg(long)]
    pub pid: Option<u32>,
    #[arg(long)]
    pub reveal_key: bool,
    #[arg(long)]
    pub key_file: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct KeyVerifyArgs {
    #[arg(long)]
    pub account: String,
    #[arg(long)]
    pub key_hex: Option<String>,
    #[arg(long)]
    pub key_file: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct SnapshotArgs {
    #[arg(long)]
    pub account: String,
    #[arg(long)]
    pub out: PathBuf,
    #[arg(long, value_delimiter = ',')]
    pub dbs: Vec<String>,
    #[arg(
        long,
        help = "Disable WAL handling (not implemented in this reconstruction)"
    )]
    pub no_wal: bool,
    #[arg(long)]
    pub reuse: bool,
    #[arg(long)]
    pub verify: bool,
}

#[derive(Debug, Args)]
pub struct DecryptArgs {
    #[arg(long)]
    pub account: String,
    #[arg(long)]
    pub snapshot: Option<PathBuf>,
    #[arg(long)]
    pub out: PathBuf,
    #[arg(long)]
    pub key_hex: Option<String>,
    #[arg(long)]
    pub key_file: Option<PathBuf>,
    #[arg(long, value_delimiter = ',')]
    pub dbs: Vec<String>,
    #[arg(long)]
    pub verify_only: bool,
}

#[derive(Debug, Args)]
pub struct SchemaArgs {
    #[arg(long)]
    pub db_dir: Option<PathBuf>,
    #[arg(long)]
    pub db: Option<String>,
}

#[derive(Debug, Args, Clone)]
pub struct ListArgs {
    #[arg(long)]
    pub db_dir: Option<PathBuf>,
    #[arg(long)]
    pub query: Option<String>,
    #[arg(long, default_value_t = 100)]
    pub limit: usize,
    #[arg(long)]
    pub cursor: Option<String>,
    #[arg(long)]
    pub with_counts: bool,
}

#[derive(Debug, Subcommand)]
pub enum ListCommand {
    List(ListArgs),
}

#[derive(Debug, Subcommand)]
pub enum MessagesCommand {
    Stats(MessageFilterArgs),
    List(MessageListArgs),
    Export(ExportArgs),
}

#[derive(Debug, Args, Clone, Default)]
pub struct MessageFilterArgs {
    #[arg(long)]
    pub db_dir: Option<PathBuf>,
    #[arg(long)]
    pub conv: Option<String>,
    #[arg(long)]
    pub since: Option<String>,
    #[arg(long)]
    pub until: Option<String>,
}

#[derive(Debug, Args, Clone)]
pub struct MessageListArgs {
    #[command(flatten)]
    pub filter: MessageFilterArgs,
    #[arg(long)]
    pub query: Option<String>,
    #[arg(long, default_value_t = 100)]
    pub limit: usize,
    #[arg(long)]
    pub cursor: Option<String>,
    #[arg(long, value_enum, default_value_t = Order::Asc)]
    pub order: Order,
    #[arg(long)]
    pub limit_per_conv: Option<usize>,
    #[arg(
        long,
        help = "Use the message index (not implemented in this reconstruction)"
    )]
    pub index: bool,
    #[arg(
        long,
        help = "Include index status (not implemented in this reconstruction)"
    )]
    pub with_index: bool,
    #[arg(
        long,
        help = "Set the search time budget (not implemented in this reconstruction)"
    )]
    pub time_budget: Option<u64>,
    #[arg(
        long,
        help = "Limit indexed search hits (not implemented in this reconstruction)"
    )]
    pub max_hits: Option<usize>,
    #[arg(
        long,
        help = "Set the index scan step (not implemented in this reconstruction)"
    )]
    pub step: Option<usize>,
}

#[derive(Debug, Args, Clone)]
pub struct ExportArgs {
    #[command(flatten)]
    pub list: MessageListArgs,
    #[arg(long, value_enum)]
    pub format: ExportFormat,
    #[arg(long)]
    pub out: Option<PathBuf>,
    #[arg(long)]
    pub copy_media: bool,
    #[arg(long)]
    pub decode_voice: bool,
    #[arg(long)]
    pub media_out: Option<PathBuf>,
    #[arg(long)]
    pub silk_decoder: Option<String>,
}

#[derive(Debug, Args)]
pub struct HistoryArgs {
    #[arg(long)]
    pub conv: Option<String>,
    #[arg(long)]
    pub since: Option<String>,
    #[arg(long)]
    pub until: Option<String>,
    #[arg(long)]
    pub query: Option<String>,
    #[arg(long, default_value_t = 100)]
    pub limit: usize,
    #[arg(long)]
    pub cursor: Option<String>,
    #[arg(long, value_enum, default_value_t = Order::Desc)]
    pub order: Order,
    #[arg(
        long,
        help = "Refresh the history cache (not implemented in this reconstruction)"
    )]
    pub refresh: bool,
    #[arg(
        long,
        default_value_t = 600,
        help = "Set cache age (not implemented in this reconstruction)"
    )]
    pub max_age: u64,
    #[arg(
        long,
        help = "Use the message index (not implemented in this reconstruction)"
    )]
    pub index: bool,
    #[arg(
        long,
        help = "Disable cache persistence (not implemented in this reconstruction)"
    )]
    pub no_persist: bool,
}

#[derive(Debug, Subcommand)]
pub enum MediaCommand {
    Decode(MediaDecodeArgs),
}

#[derive(Debug, Args)]
pub struct MediaDecodeArgs {
    #[arg(long = "in")]
    pub input: PathBuf,
    #[arg(long)]
    pub out: PathBuf,
    #[arg(long)]
    pub decoder: Option<String>,
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum Order {
    Asc,
    Desc,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ExportFormat {
    Jsonl,
    Txt,
    Md,
    Html,
}
