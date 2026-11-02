# open-wechat-cli (`owc`)

`owc` is a Rust, spec-based reconstruction of the read-only command-line interface described by the open-wechat-cli documentation and reconstruction contract. The original CLI's source code is unrecoverable; this crate is a new implementation, not the original source and not a byte-for-byte reproduction.

The intended purpose is to inspect local WXWork database snapshots, query messages and contacts, and export results. Database queries accept an explicit `--db-dir`, which is useful for fixtures and already-decrypted copies. Native client discovery, process-memory key extraction, and automatic snapshot/decrypt are not complete. Linux does not support live WXWork data access.

## Build and run

From this directory:

```sh
cargo build --release
cargo test
./target/release/owc --help
./target/release/owc doctor --json
```

The crate is standalone and is not a workspace member. The default state directory is `~/.config/agent-skills/open-wechat-cli/`; set `OWC_STATE_DIR` to override it. Configuration is strict JSON in `config.json`. The CLI supports `--json`, account and timezone selection, read-only database queries, and JSONL/TXT/Markdown/HTML exports.

## Safety and current implementation limits

The implementation does not modify client databases, inject into a process, or make network requests. The update-check interface is an offline no-op. Explicit key inputs are accepted for decryption but are never included in ordinary output. Live process-memory key scanning is intentionally unavailable in this reconstruction; `key scan` returns a precondition error rather than inspecting another process.

### Accepted CLI flags not implemented

The upstream-compatible flags `snapshot --no-wal`, `history --refresh`, `history --max-age`, `history --index`, `history --no-persist`, `messages list/export --index`, `--with-index`, `--time-budget`, `--max-hits`, and `--step` are accepted for surface compatibility but currently have no effect.

## Unknowns / needs real-device verification

The following points remain uncertain because the original implementation is unavailable and no WXWork 5.x client/database was available for verification:

1. **Key scan strategy and metadata** — the upstream strategy is described as scanning readable heap regions for 32-byte candidates and checking each against page 1. Real memory signatures, candidate offsets, process selection, and `db_key_version` semantics are unknown. This build does not perform memory scanning.
2. **Cipher profile parameters** — `sha1-64000` and `sha512-256000` follow the supplied reconstruction spec and are selected by page-1 HMAC validation. Their real-world use has not been confirmed against a device database.
3. **WXWork schema** — runtime table/column introspection and candidate scoring are implemented, but real table names, aliases, data encodings, and relationship semantics still need validation against WXWork 5.x databases.
4. **`content_type` mappings** — values beyond the documented type-label set are unknown. The current mapping is explicitly heuristic; unknown values fall back to `other`.
5. **Rich-content storage** — protobuf/XML/JSON handling is best effort. Actual link, forward, quote, recalled-message and media payload layouts, recall-key aliases, recursion rules, OCR fields, and media cache paths need real samples.
6. **Snapshot/WAL behavior** — snapshots use SQLite's read-only backup API, but live-write contention, WAL edge cases, and upstream page-verification behavior need device-level testing.

Do not treat heuristic labels, platform-specific discovery, rich-content extraction, or key acquisition as verified upstream behavior.
