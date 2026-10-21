// SPDX-License-Identifier: Apache-2.0
//! What the reports of a scan hide of the analysed files: `--no-snippets` and
//! `--redact` (SPEC section 8, ADR-0007).
//!
//! The report is rewritten once, after the scan and before any report is
//! written, so that the JSON, the HTML and the CSV carry the same values. The
//! key of the hashes is chosen before the scan: this is the one place of the
//! program where randomness is allowed, and a system that gives none is known
//! before minutes of analysis, not after.

use clap::Args as ClapArgs;
use macroport_core::redact::{Redaction, remove_snippets};
use macroport_core::report::Report;

/// The bytes of the random key of a run.
const RANDOM_KEY_BYTES: usize = 32;

/// The arguments that say what the reports of a scan hide.
#[derive(Debug, Clone, ClapArgs)]
pub struct PrivacyArgs {
    /// Leave the code of the files out of the reports: no snippet of evidence.
    /// The paths and the names stay.
    #[arg(long)]
    pub no_snippets: bool,

    /// Make reports that can leave the organisation: the paths and the name of
    /// the VBA project are replaced by a keyed hash, the snippets are left out
    /// and the messages of the errors read `[redacted]`.
    #[arg(long)]
    pub redact: bool,

    /// Key of the hashes of `--redact`. The same key gives the same hashes in
    /// every run, so that the reports can be compared. By default a random key
    /// is drawn for each run and forgotten, which makes the runs unlinkable.
    /// A key on the command line shows in the list of processes of the machine.
    #[arg(long, value_name = "KEY", requires = "redact", value_parser = non_empty)]
    pub redact_key: Option<String>,

    /// With `--redact`, hash the names of the modules and procedures too, and
    /// every other name that the project defines: they stay by default, for the
    /// people who review the report.
    #[arg(long, requires = "redact")]
    pub redact_identifiers: bool,
}

/// A key that is not empty: with none, the hashes would be a public function of
/// the text.
fn non_empty(value: &str) -> Result<String, String> {
    if value.is_empty() {
        Err("the key is empty".to_owned())
    } else {
        Ok(value.to_owned())
    }
}

/// What the reports of a scan hide.
#[derive(Debug)]
pub enum Privacy {
    /// Nothing: the reports are as the analysis made them.
    AsAnalysed,
    /// `--no-snippets`: the code of the files is left out.
    NoSnippets,
    /// `--redact`, with its key.
    Redact(Redaction),
}

impl Privacy {
    /// What `args` ask for, with the key of the redaction drawn at random if
    /// `--redact-key` does not give one.
    ///
    /// # Errors
    ///
    /// The reason, as text, when the system gives no random bytes or HMAC
    /// refuses the key, which it does for none.
    pub fn new(args: &PrivacyArgs) -> Result<Self, String> {
        if !args.redact {
            return Ok(if args.no_snippets {
                Self::NoSnippets
            } else {
                Self::AsAnalysed
            });
        }
        let key = match &args.redact_key {
            Some(key) => key.as_bytes().to_vec(),
            None => random_key()?.to_vec(),
        };
        Redaction::new(&key, args.redact_identifiers)
            .map(Self::Redact)
            .map_err(|error| error.to_string())
    }

    /// Rewrites `report` as the privacy says.
    pub fn apply(&self, report: &mut Report) {
        match self {
            Self::AsAnalysed => {}
            Self::NoSnippets => remove_snippets(report),
            Self::Redact(redaction) => redaction.apply(report),
        }
    }
}

/// A key of random bytes from the system, which no one else knows.
fn random_key() -> Result<[u8; RANDOM_KEY_BYTES], String> {
    let mut key = [0_u8; RANDOM_KEY_BYTES];
    getrandom::fill(&mut key).map_err(|error| error.to_string())?;
    Ok(key)
}
