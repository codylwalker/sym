//! The CLI surface, as a `clap::Subcommand` so a host binary can embed it.

use crate::{ops, render};
use clap::Subcommand;
use std::path::PathBuf;

#[derive(Subcommand, Debug)]
pub enum SymCmd {
    /// List symbols (skeleton view) of one file
    Ls {
        file: PathBuf,
        /// Emit JSON instead of text
        #[arg(long)]
        json: bool,
        /// Append a token estimate (chars/4)
        #[arg(long)]
        est: bool,
    },
    /// Print one symbol's source (function/struct/class/…) with its doc block
    Read {
        file: PathBuf,
        /// Symbol name; nested symbols match on the leaf name or
        /// `outer.leaf` / `outer::leaf`
        symbol: String,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        est: bool,
    },
    /// Budgeted repo map: per-file signatures, ranked by import fan-in
    Map {
        dir: PathBuf,
        /// Approximate token budget for the whole map (tokens ≈ chars/4)
        #[arg(long, default_value_t = 1000)]
        budget: usize,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        est: bool,
    },
}

/// Run one verb and return what should be printed (text or JSON).
pub fn run_to_string(cmd: SymCmd) -> Result<String, String> {
    match cmd {
        SymCmd::Ls { file, json, est } => {
            let out = ops::ls(&file)?;
            finish(render::ls_text(&out), &out, json, est)
        }
        SymCmd::Read {
            file,
            symbol,
            json,
            est,
        } => {
            let out = ops::read(&file, &symbol)?;
            finish(render::read_text(&out), &out, json, est)
        }
        SymCmd::Map {
            dir,
            budget,
            json,
            est,
        } => {
            let out = ops::map(&dir, budget)?;
            finish(render::map_text(&out), &out, json, est)
        }
    }
}

/// JSON carries the data plus the text rendering's token estimate; text
/// gets the estimate as a trailing line only when asked.
pub fn finish<T: serde::Serialize>(
    text: String,
    data: &T,
    json: bool,
    est: bool,
) -> Result<String, String> {
    if json {
        let mut v = serde_json::to_value(data).map_err(|e| e.to_string())?;
        if let Some(obj) = v.as_object_mut() {
            obj.insert("tokens_est".into(), crate::tokens_est(text.len()).into());
        }
        let mut s = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?;
        s.push('\n');
        Ok(s)
    } else if est {
        Ok(render::with_est(text))
    } else {
        Ok(text)
    }
}
