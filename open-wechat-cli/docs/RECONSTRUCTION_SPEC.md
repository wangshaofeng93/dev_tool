# owc 重建规格（Reconstruction Spec）

本文件是本仓库 `open-wechat-cli/` 下 Rust 实现的**权威契约**。实现必须遵循本文件；
与本文件冲突的猜测一律以本文件为准。

## 0. 来源与性质（Provenance）

原始 `owc` CLI 的 Rust 源码**不公开、未能找回**：
- 公开仓库 `briqt/open-wechat-cli` 只包含 Skill + 文档（`skills/open-wechat-cli/SKILL.md`、
  `config.json`、`config.schema.json`、`README.md`、`LICENSE`、`notice.json`），不含 CLI 源码。
- v0.1.0 Releases 只提供预编译二进制；二进制内**没有**业务源码路径，只有依赖/标准库路径。

因此本实现是**按规格重写（reconstruction）**，不是逐字还原。规格来源三类，按可信度排序：

1. **S1 文档（权威）** — 上游 `SKILL.md` / `README.md` / `config.schema.json`。见 `docs/UPSTREAM_README.md`
   与 `skills/open-wechat-cli/`。JSON 信封、错误码语义、字段清单、默认排序均以此为准。
2. **S2 二进制可观察事实** — 从 v0.1.0 两个平台二进制 `strings` 提取：依赖版本、模块名、
   子命令名、参数名、环境变量、类型标签、导出模板 CSS/HTML、常量（`DEFAULT_PAGE_SIZE=4096`、
   `AES-CBC`、`message.db`/`session.db`/`user.db`、`owc_send_time_index` 等）。
3. **S3 推断（必须标注）** — 无法从 S1/S2 确定的部分（见 §9 未知项）。实现中凡属 S3 的
   决策，必须在代码注释里写 `// S3:` 并说明依据与回退策略。

## 1. 不可逾越的行为边界（上游 README 承诺，必须保留）

- **严格只读**：绝不修改企业微信客户端原目录下任何文件（含不开启 WAL 写入、不执行
  `PRAGMA journal_mode` 之类会写活库的语句）。读取活库一律先做只读快照。
- **不注入目标进程**：仅允许读取进程内存（Windows `ReadProcessMemory`、macOS `vm_read`），
  禁止写内存、禁止注入代码/DLL、禁止 hook。
- **不外传**：除 §8 的版本/公告检查外，禁止任何网络请求；绝不把聊天内容或密钥发往外部。
- **密钥不泄露**：密钥只写入 `keys/<corp_id>.json`（权限 `0600`）。除显式 `--reveal-key`
  外，任何输出（含 `--json`、日志、错误信息）都不得包含 `key_hex`；默认输出 `key_fingerprint`
  （key 的 SHA256 前 8 字节 hex）。
- 所有快照/解密/导出产物只落在状态目录（§3）。

## 2. Crate 与依赖（版本对齐二进制观察值）

包名 `owc`，bin 名 `owc`，version `0.1.0`，edition 2021，license MIT。

必须使用的版本（S2，已验证可编译）：
`clap 4.6`(derive) · `rusqlite 0.32.1`(bundled) · `serde 1`(derive) · `serde_json 1` ·
`chrono 0.4` · `chrono-tz 0.10` · `aes 0.8.4` · `hex 0.4.3` · `base64 0.22.1` ·
`walkdir 2.5` · `indexmap 2`(serde)

补充（实现 §5 所需，S3）：`cbc 0.1` · `hmac 0.12` · `sha1 0.10` · `sha2 0.10` · `pbkdf2 0.12`

**平台相关依赖必须 cfg 门控**，Linux 上 `cargo build` 不得需要它们：
`[target.'cfg(windows)'.dependencies] windows-sys` / `[target.'cfg(target_os="macos")'.dependencies] libc, mach2`

> 注意：二进制内含 SQLite amalgamation 与 `aes-0.8.4`，但**未链接 SQLCipher**。
> 说明上游是**自己按页 AES-CBC 解密成明文 SQLite 文件**（见 §5），而不是用 SQLCipher 打开。
> 实现必须照此做：解密产物是可被普通 SQLite 打开的明文库。

## 3. 状态目录与配置

根目录 `~/.config/agent-skills/open-wechat-cli/`，可被 `OWC_STATE_DIR` 覆盖。
Windows 上 `~` 取 `USERPROFILE`。

| 路径 | 用途 |
|---|---|
| `config.json` | 用户配置 |
| `keys/<corp_id>.json` | 密钥缓存，`0600` |
| `cache/<corp_id>/snapshot/` | 只读快照 |
| `cache/<corp_id>/decrypted/Data/` | 解密后明文 SQLite |
| `cache/<corp_id>/message_lookup.db` | 索引库（`--index`，S2 文件名） |
| `export/` | 默认导出目录 |

`config.json` 字段严格等于 `skills/open-wechat-cli/config.schema.json`（`additionalProperties:false`）：
`default_account`(""), `tz`("local"), `data_root`(""), `silk_decoder`(""), `disable_update_check`(false)。
缺文件/缺字段用默认值；**多余字段必须报 `USAGE`**（schema 要求）。

环境变量（S2，优先级高于 config）：
`OWC_STATE_DIR` · `OWC_WECOM_DATA`(覆盖 data_root) · `OWC_SILK_DECODER` · `OWC_KEY_HEX` · `OWC_NO_UPDATE_CHECK`

## 4. CLI 表面

全局参数：`--json` · `-v/--verbose` · `--account <corp_id>` · `--tz <local|IANA>`

子命令（S2 完整清单，顺序即二进制内顺序）：

```
owc doctor
owc accounts [-v]
owc key scan   --account <corp_id> [--pid <n>] [--reveal-key] [--key-file <p>]
owc key verify --account <corp_id> [--key-hex <hex>] [--key-file <p>]
owc snapshot   --account <corp_id> --out <dir> [--dbs <list>] [--no-wal] [--reuse] [--verify]
owc decrypt    --account <corp_id> [--snapshot <dir>] --out <dir>
               [--key-hex <hex>|--key-file <p>] [--dbs <list>] [--verify-only]
owc schema     [--db-dir <dir>] [--db <name>]
owc sessions list  [--db-dir] [--query] [--limit] [--cursor] [--with-counts]
owc contacts list  [--db-dir] [--query] [--limit] [--cursor]
owc messages stats [--db-dir] [--conv] [--since] [--until]
owc messages list  [--db-dir] [--conv] [--since] [--until] [--query] [--limit]
                   [--cursor] [--order asc|desc] [--limit-per-conv] [--index] [--with-index]
                   [--time-budget <s>] [--max-hits <n>] [--step <n>]
owc messages export [同 messages list 的过滤参数] --format jsonl|txt|md|html
                   [--out <dir>] [--copy-media] [--decode-voice] [--media-out <dir>]
                   [--silk-decoder <cmd>]
owc media decode --in <file> --out <file> [--decoder <cmd>]
owc history    [--conv] [--since] [--until] [--query] [--limit] [--cursor]
               [--order asc|desc] [--refresh] [--max-age <s>] [--index] [--no-persist]
```

关键默认值与语义（S1 权威）：
- `history` 默认 `--order desc`；`messages list` 默认 `--order asc`。
- `history` 默认复用缓存，`--max-age` 默认 **600** 秒；`--refresh` 强制重拍快照+解密。
- `--db-dir` 对所有查询类子命令**可选**；省略时用 `cache/<corp_id>/decrypted/Data`，
  **缺缓存则自动执行 key→snapshot→decrypt→query 全链路**。
- `--conv` 既接受原始 `conv_id`（`R:...` 群 / `S:...` 单聊），也接受**群名/联系人姓名子串**，
  需自动解析匹配；多个匹配时取最近活跃者，并在 `data.resolved_conv` 回报解析结果。
- 时间格式 `YYYY-MM-DD` 或 `YYYY-MM-DDTHH:MM:SS`；按 `--tz`、配置 `tz`、默认
  `local` 的优先级解释（`--tz` 优先级最高）；格式或时区错误 → `USAGE`。
- `send_time`、`stats.first_time` 和 `stats.last_time` 都按同一有效时区格式化；
  `send_time_epoch` 始终为 UTC epoch 秒。
- `--query` 的匹配范围（S1）：正文 `text` + 链接 title/url + 附件文件名 + `media[].ocr` +
  合并转发子消息正文。
- `--cursor`：不透明字符串，实现为 `base64url(send_time_epoch:message_id)`；
  原样回传 `data.next_cursor` 即可翻页，须与相同 `--order`/过滤参数配合。

## 5. 数据库解密（核心，S2 事实 + S3 参数）

S2 事实：`AES-CBC`、`DEFAULT_PAGE_SIZE=4096`、`cipher`、`raw key`、页 1 校验
（`key` / `page1` / `magic` / `quick_check=` / `.bad` / `WAL` 字符串）。

实现为 SQLCipher 兼容的按页加密格式（微信系客户端通用格式）：

- 页大小 4096；文件**前 16 字节是 salt**。
- `enc_key = PBKDF2-HMAC(raw_key(32B), salt, iter, 32)`
- `mac_salt = salt XOR 0x3a`（逐字节）；`mac_key = PBKDF2-HMAC(enc_key, mac_salt, 2, 32)`
- 每页尾部 `reserve` 字节 = `IV(16) || HMAC(n) || padding`，`reserve` 向上取整到 16 的倍数。
- 页正文 = `page[off .. page_size-reserve]`，`off` 仅页 1 为 16，其余为 0。
- `AES-256-CBC` 解密，IV 取该页 reserve 区前 16 字节。
- HMAC 校验范围 = `页正文 || IV || le_u32(page_no)`。
- 解密产物：页 1 前面补标准头 `b"SQLite format 3\0"`，整库写成明文 SQLite 文件。

**参数档位（profiles）**，按序尝试，用页 1 的 HMAC 是否匹配来自动判定：

| profile | kdf hmac | iter | hmac | reserve |
|---|---|---|---|---|
| `sha1-64000`   | HMAC-SHA1   | 64000  | HMAC-SHA1   | 48 |
| `sha512-256000`| HMAC-SHA512 | 256000 | HMAC-SHA512 | 80 |

> `reserve` 由 hmac 长度推导：`ceil((16+hmac_len)/16)*16`（SHA1→48，SHA512→80）。
> 档位与判定逻辑属 S3，必须集中在 `cipher.rs` 的 `CipherProfile`，便于真机修正。

校验（`key verify` / `--verify`）：页 1 HMAC 匹配 **且** 解密后前 16 字节为
`SQLite format 3\0` **且** 对产物跑 `PRAGMA quick_check` 通过。失败按 §7 分别报
`KEY_NOT_FOUND` / `FORMAT_MISMATCH` / `DATA`。

**必须有往返测试**：在 `cfg(test)` 下实现对应的 `encrypt_db`，用 rusqlite 造一个真实
明文 SQLite 库 → 加密成 fixture → 解密 → 断言与原文件**逐字节相等**且 rusqlite 能打开、
`quick_check` 通过。两个 profile 都要测。另须测：错误 key → `KEY_NOT_FOUND`；
明文库当密文输入 → `FORMAT_MISMATCH`。

## 6. 平台与 schema

数据根目录（S2 片段，S3 组装 — 必须可被 `OWC_WECOM_DATA`/`config.data_root` 覆盖）：
- Windows：`%USERPROFILE%\Documents\WXWork\`（客户端 `WXWork.exe`，`C:\Program Files[ (x86)]\WXWork`）
- macOS：`~/Library/Containers/com.tencent.WeWorkMac/Data/Library/Application Support/WXWork/`
- Linux：**不支持** → 一切需要本地数据的命令返回 `PRECONDITION`（不得 panic）。

账号目录按 `corp_id` 划分；库文件 `message.db` / `session.db` / `user.db`（S2）。

**WXWork 5.x 的真实表/列名无法从二进制或文档确定（见 §9）。** 因此禁止硬编码 schema：
- `schema.rs` 用 `PRAGMA table_list` + `PRAGMA table_info(...)` **运行时自省**，
  再用候选名打分解析出 `SchemaMap`（消息表及 conv/sender/time/type/content 列等）。
- 候选名表集中、可扩展；解析失败 → `DATA` 错误，`repair` 提示跑 `owc schema --json` 反馈。
- `owc schema --json` 必须输出自省到的全部表/列，供真机用户回报以修正候选表。

## 7. JSON 信封与错误码（S1 权威）

成功 → **stdout 单行** JSON：
```json
{"ok":true,"command":"messages list","data":{ ... }}
```
失败 → **stderr 最后一行**：
```json
{"ok":false,"error":{"code":"USAGE","message":"...","repair":"..."}}
```
`--json` 下 stdout 只允许这一个信封（日志走 stderr）。非 `--json` 时输出人类可读文本，
但错误信封仍按上面格式打到 stderr 最后一行。

错误码（S2 完整集合 + S1 语义）与建议退出码：

| code | 含义 | exit |
|---|---|---|
| `USAGE` | 参数/时间格式错误 | 2 |
| `PRECONDITION` | 客户端未运行/未登录/首次缺 key/平台不支持 | 3 |
| `KEY_NOT_FOUND` | 密钥缺失或无法提取 | 3 |
| `PERMISSION` | macOS 完全磁盘访问/内存读取受限 | 4 |
| `DEPENDENCY` | `--decode-voice` 但缺 silk 解码器 | 5 |
| `SNAPSHOT` | 活库写入剧烈，快照页校验未过 | 6 |
| `DATA` | 库损坏/schema 不可识别 | 6 |
| `FORMAT_MISMATCH` | 输入不是预期的加密库格式 | 6 |
| `INTERNAL` | 兜底 | 70 |

`repair` 必须给**可直接执行的**建议，复刻 S2 观察到的提示，例如：
`owc doctor` · `owc accounts` · `owc accounts -v` · `owc key scan --account <corp_id>` ·
`owc snapshot --account <corp_id> --out <dir>` · `owc decrypt` · `owc schema` ·
`owc history --refresh` · `owc --help`。

**不得 panic**：`main` 捕获一切，unwrap/expect 只允许出现在不可能失败处并加注释。

## 8. 输出数据结构（S1 权威字段，不得增删基础字段）

`doctor.data`：`platform` · `client_installed` · `client_running` · `memory_access_ok` ·
`state_dir` · `data_root` · `silk_decoder`(可用性) · `accounts[]`

`accounts[]` 元素（S2 字段名）：`corp_id` · `login_user_global_id` · `login_company_id` ·
`db_key_version` · `has_message_db` · `last_activity` · `key_status`(`verified|missing|stale`) ·
`key_fingerprint` · `dbs[]`

`messages list` / `history` 的 `data`：`items[]` · `next_cursor` · `resolved_conv` · `total`(可选)

`items[]` 基础字段（S1，必须全部存在）：
`message_id` · `conv_id` · `conv_name` · `sender_id` · `sender_name` · `send_time` ·
`send_time_epoch` · `content_type` · `type_label` · `text`

`send_time` 为有效时区中的 `YYYY-MM-DDTHH:MM:SS`；统计时间 `first_time` / `last_time`
使用同一时区格式，`send_time_epoch` 保持原始 UTC epoch 秒。

扩展字段（按需出现）：
- `media[]`：`kind`(`image|file|voice|video`) · `name` · `size` · `width` · `height` · `md5` ·
  `duration_s` · `local_path` · `exists` · `ocr`
- `link`：`url` · `title` · `desc`
- `forward`：`title` · `total` · `items[]`（`type_label=="chat_record"`，**最多递归 3 层**）
- `quote`：`sender_name` · `text`
- `recalled`：`revoker_id` · `revoked_at`

`type_label` 取值集合（S2 完整提取）：
`rich_text` `chat_record` `doc_link` `article` `miniprogram` `drive_file` `drive_notice`
`article_card` `app_card` `rich_card` `approval` `stats_card` `moment_notice` `system`
`weather` `finder` `video` `meeting` `other`
外加基础类：`text` `image` `voice` `file` `link`

> 数值 `content_type` 原样透传 DB 值；`type_label` 由 `codec.rs` 中**表驱动**映射，
> 未知值回退 `other`。真实数值映射属 S9 未知项。

`messages stats.data`（S1）：消息条数、发言人数、首末时间、类型分布 →
`total` · `senders` · `first_time` · `last_time` · `by_type{label:count}`（按会话时带 `by_conv[]`）

`sessions list` / `contacts list`：`items[]` + `next_cursor`；会话含 `conv_id`/`conv_name`/
`last_activity`（`--with-counts` 时加 `message_count`），联系人含 `id`/`name`/别名等。

## 9. 导出（`render.rs`）

四种格式：`jsonl`（每行一条消息，结构同 §8）· `txt`（按会话分文件）· `md` · `html`（单文件）。
默认输出目录 `export/`，`--out` 可覆盖。

`md`/`html` 必须复刻上游模板：**CSS 与 HTML 片段见 `docs/upstream-export-template.txt`，
请逐字使用**（含 `:root` 变量 `--fg/--muted/--bg/--card/--line/--accent/--quote`、
深色模式 `@media (prefers-color-scheme: dark)`、`.msg/.avatar/.body/.who/.chip/.text/.ref/
.card/.file/.fwd/.day/.meta` 等类名、`<html lang="zh-CN">`、按天分组的 `.day` 分隔、
footer 内 `open-wechat-cli` 标记）。

`--copy-media` 把命中的本地缓存附件复制到 `--media-out`（默认导出目录下 `media/`），
并把 `local_path` 改写为相对路径；**只复制、不移动、不改原文件**。
`--decode-voice` 把 SILK v3 语音转 `wav`：检测 `#!SILK_V3` magic（S2），
调用外部解码器（`--silk-decoder` / `OWC_SILK_DECODER` / `config.silk_decoder`，
命令模板支持 `{in}`/`{out}` 占位）；缺解码器 → `DEPENDENCY`，并提示可去掉
`--decode-voice` 先导出 `.silk` 原文件（S1）。

## 10. 版本/公告检查

`notice.json` 结构见仓库根同名文件（`latest_version`/`release_url`/`notice_id`/`notice`）。
受 `disable_update_check` / `OWC_NO_UPDATE_CHECK` 控制；**默认实现为完全离线的 no-op**
（本仓库不复刻联网拉取，以满足"不外传"边界），仅保留接口与配置项。

## 11. 未知项（S3 风险清单，必须在 README 如实声明）

以下无法从公开文档/二进制确定，**也无法在无企业微信客户端的 Linux 上验证**：

1. **密钥在进程内存中的定位方式**（`keyscan.rs`/`extract.rs`）：扫描特征、偏移、
   `db_key_version` 语义。实现按"扫描可读堆段 → 收集 32 字节候选 → 用 §5 页 1 校验逐个验证"
   的通用策略，不依赖硬编码偏移。
2. **PBKDF2 迭代次数/HMAC 算法的真实取值**（§5 的两个 profile 是推断，靠校验自动选择）。
3. **WXWork 5.x 的真实表名/列名**（§6 改为运行时自省 + 候选打分）。
4. **`content_type` 数值 → `type_label` 的真实映射**（§8 表驱动，未知回退 `other`）。
5. **消息富内容的二进制/protobuf 布局**（`proto.rs`/`codec.rs`）：实现一个最小 protobuf
   wire 解析器 + XML/JSON 兜底，尽力提取 link/forward/quote/media，解析失败时
   **保留 `text` 为空并降级为 `other`，不得报错中断整次查询**。
   recalled 字段名/别名同样是推测，需用真实设备消息验证。
6. 快照的 WAL 处理细节与 `--verify` 的页校验强度。

## 12. 验证要求（Linux 上必须全绿）

- `cargo fmt --check` · `cargo clippy -- -D warnings`（允许少量有注释的 allow）· `cargo test`
- `cargo build --release` 成功，且**不需要任何 Windows/macOS 专属依赖**。
- `owc --version` / `owc --help` / 每个子命令 `--help` 正常。
- `owc doctor --json` 在 Linux 上输出**合法成功信封**（`client_installed:false`、
  `accounts: []`、platform 为 linux），不得 panic、不得返回非 0。
- 任意需要本地数据的查询（如 `owc history --json`）在 Linux 上返回
  `{"ok":false,"error":{"code":"PRECONDITION",...}}` 到 stderr 最后一行，退出码 3。
- 配置：多余字段 → `USAGE`；`--since` 非法格式 → `USAGE`。
- 用临时明文 SQLite fixture（按 §6 自省机制能识别的表结构）端到端验证
  `sessions list` / `contacts list` / `messages stats` / `messages list`（含 `--query`、
  `--since/--until`、`--order`、`--limit` + `--cursor` 翻页）与四种格式导出；
  断言 md/html 产物含 §9 要求的类名与按天分组。
- §5 要求的解密往返测试与错误路径测试。
