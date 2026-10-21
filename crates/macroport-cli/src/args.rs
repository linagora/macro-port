// SPDX-License-Identifier: Apache-2.0
//! The arguments that several commands share, and the checks of their values.
//! A value that does not pass is a usage error: exit code 2 (SPEC M4).

use std::fs;
use std::path::PathBuf;

use clap::{Args, ValueEnum};
use macroport_core::analyze::DEFAULT_MAX_FILE_SIZE;
use macroport_core::report::Lang;

/// The number of bytes in the megabyte of `--max-file-size`, like the limits of
/// the container layer.
const BYTES_PER_MB: u64 = 1024 * 1024;

/// The default of `--max-file-size`, in MB.
const DEFAULT_MAX_FILE_MB: u64 = DEFAULT_MAX_FILE_SIZE / BYTES_PER_MB;

/// The largest `--max-file-size`: a terabyte, which no file reaches.
const LARGEST_MAX_FILE_MB: u64 = 1024 * 1024;

/// The language of the messages and of the report.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum)]
pub enum LangArg {
    /// English.
    #[default]
    En,
    /// French.
    Fr,
}

impl From<LangArg> for Lang {
    fn from(lang: LangArg) -> Self {
        match lang {
            LangArg::En => Self::En,
            LangArg::Fr => Self::Fr,
        }
    }
}

/// A kind of report that `scan` writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub enum Format {
    /// `report.json`, the fleet report that the schema describes.
    Json,
    /// `report.html`, one page that holds its data and its style.
    Html,
    /// `files.csv`, `procedures.csv` and `signals.csv`, for a spreadsheet.
    Csv,
}

impl Format {
    /// Every format, the default of `--format`.
    pub const ALL: [Self; 3] = [Self::Json, Self::Html, Self::Csv];
}

/// A kind of report that `file` prints: one document, so no CSV.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum)]
pub enum FileFormat {
    /// The report that the schema describes, for the one file.
    #[default]
    Json,
    /// One page that holds its data and its style.
    Html,
}

impl From<FileFormat> for Format {
    fn from(format: FileFormat) -> Self {
        match format {
            FileFormat::Json => Self::Json,
            FileFormat::Html => Self::Html,
        }
    }
}

/// How a command analyses the files it is given.
#[derive(Debug, Clone, Args)]
pub struct AnalysisArgs {
    /// Language of the messages and of the report.
    #[arg(long, value_enum, default_value_t)]
    pub lang: LangArg,

    /// Directory with `families.yaml`, `security.yaml` and `complexity.yaml`,
    /// to use instead of the embedded rules. They are checked as `rules check`
    /// checks them, and a problem stops the command with exit code 3.
    #[arg(long, value_name = "DIR", value_parser = existing_dir)]
    pub rules: Option<PathBuf>,

    /// Largest file to analyse, in MB. A larger file is not analysed: it is
    /// hashed and reported as too large.
    #[arg(
        long,
        value_name = "MB",
        default_value_t = DEFAULT_MAX_FILE_MB,
        value_parser = clap::value_parser!(u64).range(1..=LARGEST_MAX_FILE_MB),
    )]
    pub max_file_size: u64,

    /// Fix the date of the report and leave out the duration of the scan, so
    /// that two runs on the same files give the same bytes. For tests.
    #[arg(long)]
    pub deterministic: bool,
}

impl AnalysisArgs {
    /// The language of the messages and of the report.
    pub fn lang(&self) -> Lang {
        self.lang.into()
    }

    /// `--max-file-size` in bytes.
    pub fn max_file_size_bytes(&self) -> u64 {
        self.max_file_size.saturating_mul(BYTES_PER_MB)
    }
}

/// A path that exists and is a file or a directory.
pub fn existing_path(value: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(value);
    match fs::metadata(&path) {
        Ok(metadata) if metadata.is_file() || metadata.is_dir() => Ok(path),
        Ok(_) => Err("neither a file nor a directory".to_owned()),
        Err(error) => Err(error.to_string()),
    }
}

/// A path that exists and is a file.
pub fn existing_file(value: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(value);
    match fs::metadata(&path) {
        Ok(metadata) if metadata.is_file() => Ok(path),
        Ok(_) => Err("not a file".to_owned()),
        Err(error) => Err(error.to_string()),
    }
}

/// A path that exists and is a directory.
pub fn existing_dir(value: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(value);
    if path.is_dir() {
        Ok(path)
    } else {
        Err("not an existing directory".to_owned())
    }
}
