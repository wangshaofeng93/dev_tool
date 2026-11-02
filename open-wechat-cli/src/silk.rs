use crate::output::{ErrorCode, OwcError, Result};
use std::path::Path;
use std::process::Command;

pub fn decode(input: &Path, output: &Path, decoder: &str) -> Result<()> {
    let bytes = std::fs::read(input).map_err(|error| OwcError::data(error.to_string()))?;
    if !bytes.starts_with(b"#!SILK_V3") {
        return Err(OwcError::new(
            ErrorCode::FormatMismatch,
            "Input does not have the SILK v3 magic header.",
            "Provide a SILK v3 voice file.",
        ));
    }
    // S3: The external command is tokenized without a shell so file paths cannot become shell code.
    let tokens = decoder.split_whitespace().collect::<Vec<_>>();
    let executable = tokens
        .first()
        .map(|token| {
            token
                .replace("{in}", &input.to_string_lossy())
                .replace("{out}", &output.to_string_lossy())
        })
        .ok_or_else(|| {
            OwcError::new(
                ErrorCode::Dependency,
                "No SILK decoder is configured.",
                "Remove --decode-voice to export .silk files, or configure --silk-decoder.",
            )
        })?;
    let mut command = Command::new(executable);
    let input_string = input.to_string_lossy();
    let output_string = output.to_string_lossy();
    let has_placeholders = tokens
        .iter()
        .any(|token| token.contains("{in}") || token.contains("{out}"));
    for token in tokens.iter().skip(1) {
        command.arg(
            token
                .replace("{in}", &input_string)
                .replace("{out}", &output_string),
        );
    }
    if !has_placeholders {
        command.arg(input).arg(output);
    }
    let status = command.status().map_err(|error| {
        OwcError::new(
            ErrorCode::Dependency,
            format!("Could not start SILK decoder: {error}"),
            "Install a SILK v3 decoder or remove --decode-voice.",
        )
    })?;
    if !status.success() || !output.is_file() {
        return Err(OwcError::data(
            "The SILK decoder did not produce a WAV output.",
        ));
    }
    Ok(())
}
