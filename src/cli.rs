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
    /// Definitions named NAME anywhere under DIR (leaf or qualified path)
    Find {
        name: String,
        #[arg(default_value = ".")]
        dir: PathBuf,
        /// Match names that start with NAME
        #[arg(long)]
        prefix: bool,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        est: bool,
    },
    /// Every symbol under DIR as an embedding chunk (NDJSON)
    Chunks { dir: PathBuf },
    /// Build or refresh the semantic index of DIR at OUT (embeds new chunks when --embed-url is set)
    Index {
        dir: PathBuf,
        #[arg(long)]
        out: PathBuf,
        /// OpenAI-compatible embeddings base URL (http://host:port[/v1])
        #[arg(long)]
        embed_url: Option<String>,
        #[arg(long, default_value = "")]
        embed_model: String,
        #[arg(long, default_value_t = 32)]
        batch: usize,
    },
    /// Code by meaning: the chunks closest to QUERY in an index built by `sym index`
    Where {
        query: String,
        #[arg(long)]
        index: PathBuf,
        #[arg(long, default_value_t = 8)]
        k: usize,
        /// Embeddings base URL for the query (default: the one the index recorded)
        #[arg(long)]
        embed_url: Option<String>,
        #[arg(long, default_value = "")]
        embed_model: String,
        /// Add a small boost for query words found in the symbol's path or signature
        #[arg(long)]
        hybrid: bool,
        #[arg(long)]
        json: bool,
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
        SymCmd::Find {
            name,
            dir,
            prefix,
            json,
            est,
        } => {
            let out = ops::find(&dir, &name, prefix)?;
            finish(render::find_text(&out), &out, json, est)
        }
        SymCmd::Chunks { dir } => {
            let cs = crate::index::chunks(&dir)?;
            let mut s = String::new();
            for c in cs {
                s.push_str(&serde_json::to_string(&c).map_err(|e| e.to_string())?);
                s.push('\n');
            }
            Ok(s)
        }
        SymCmd::Index { dir, out, embed_url, embed_model, batch } => {
            let embed = embed_url.as_deref().map(|u| (u, embed_model.as_str()));
            let r = crate::index::build(&dir, &out, embed, batch)?;
            Ok(format!(
                "indexed {} ({} files): {} chunks, {} embedded, {} reused → {}\n",
                crate::ops::display(&dir), r.files, r.chunks, r.embedded, r.reused, crate::ops::display(&out)
            ))
        }
        SymCmd::Where { query, index, k, embed_url, embed_model, hybrid, json, .. } => {
            let embed = embed_url.as_deref().map(|u| (u, embed_model.as_str()));
            let out = crate::index::find_where(&index, &query, k, embed, hybrid)?;
            finish(crate::index::where_text(&out), &out, json, false)
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
