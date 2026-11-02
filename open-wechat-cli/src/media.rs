use crate::cli::{Cli, MediaCommand};
use crate::config::Config;
use crate::output::{ErrorCode, OwcError, Result};
use crate::state::State;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub fn run(
    command: MediaCommand,
    _cli: &Cli,
    config: &Config,
    _state: &State,
) -> Result<(String, Value, String)> {
    match command {
        MediaCommand::Decode(args) => {
            let decoder = args
                .decoder
                .or_else(|| std::env::var("OWC_SILK_DECODER").ok())
                .or_else(|| (!config.silk_decoder.is_empty()).then(|| config.silk_decoder.clone()))
                .ok_or_else(|| {
                    OwcError::new(
                        ErrorCode::Dependency,
                        "No SILK decoder is configured.",
                        "Specify --decoder or set OWC_SILK_DECODER.",
                    )
                })?;
            crate::silk::decode(&args.input, &args.out, &decoder)?;
            Ok(crate::output::result(
                "media decode",
                json!({"input": args.input, "output": args.out}),
            ))
        }
    }
}

pub fn copy_message_media(
    messages: &[crate::store::Message],
    media_out: &Path,
    decode_voice: bool,
    decoder: Option<&str>,
) -> Result<Vec<crate::store::Message>> {
    let mut messages = messages.to_vec();
    std::fs::create_dir_all(media_out).map_err(|error| OwcError::data(error.to_string()))?;
    for (message_index, message) in messages.iter_mut().enumerate() {
        for (media_index, media) in message.extra.media.iter_mut().enumerate() {
            let Some(local_path) = media.local_path.as_deref() else {
                continue;
            };
            let source = PathBuf::from(local_path);
            if !source.is_file() {
                media.exists = Some(false);
                continue;
            }
            media.exists = Some(true);
            let name = source
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("attachment");
            let destination = media_out.join(format!("{message_index}-{media_index}-{name}"));
            std::fs::copy(&source, &destination)
                .map_err(|error| OwcError::data(error.to_string()))?;
            let relative = destination
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("attachment")
                .to_owned();
            media.local_path = Some(relative.clone());
            if decode_voice && media.kind == "voice" {
                let copied = destination;
                let bytes =
                    std::fs::read(&copied).map_err(|error| OwcError::data(error.to_string()))?;
                if bytes.starts_with(b"#!SILK_V3") {
                    let decoder = decoder.ok_or_else(|| {
                        OwcError::new(
                        ErrorCode::Dependency,
                        "SILK decoding requires an external decoder.",
                        "Remove --decode-voice to export .silk files, or configure --silk-decoder.",
                    )
                    })?;
                    let wav_name = format!("{message_index}-{media_index}-{name}.wav");
                    let wav_path = media_out.join(&wav_name);
                    crate::silk::decode(&copied, &wav_path, decoder)?;
                    media.local_path = Some(wav_name);
                }
            }
        }
    }
    Ok(messages)
}
