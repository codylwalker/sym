//! `sym` — read the function, not the file.

use clap::{Parser, Subcommand};
use std::io::Write as _;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "sym", version, about = "Read the function, not the file: symbol-level code reads for AI coding agents.")]
struct Cli {
    #[command(subcommand)]
    cmd: Top,
}

#[derive(Subcommand, Debug)]
enum Top {
    #[command(flatten)]
    Sym(sym::cli::SymCmd),
    /// Serve the three verbs as MCP tools over stdio (for Claude Code,
    /// Claude Desktop, or any MCP client): {"command":"sym","args":["mcp"]}
    Mcp,
    /// Serve the three verbs as loopback HTTP JSON (for a gateway to front)
    Serve {
        #[arg(long, default_value_t = 8431)]
        port: u16,
        /// Jail every path under this directory
        #[arg(long)]
        root: Option<PathBuf>,
    },
    /// Claude Code hook helpers
    Hook {
        #[command(subcommand)]
        which: HookCmd,
    },
}

#[derive(Subcommand, Debug)]
enum HookCmd {
    /// PreToolUse on Read: nudge whole-file reads of big source files toward
    /// `sym ls` / `sym read`. Reads the hook JSON on stdin; always exits 0.
    Pre {
        #[arg(long, default_value_t = 200, env = "SYM_HOOK_MIN_LINES")]
        min_lines: usize,
    },
}

/// Print without panicking on a closed pipe (`| head`).
fn out(s: &str) {
    let so = std::io::stdout();
    let mut l = so.lock();
    let _ = l.write_all(s.as_bytes());
    let _ = l.flush();
}

fn main() {
    let cli = Cli::parse();
    let res: Result<(), String> = match cli.cmd {
        Top::Sym(c) => sym::cli::run_to_string(c).map(|s| out(&s)),
        Top::Mcp => sym::mcp::serve_stdio(),
        Top::Serve { port, root } => sym::serve::serve(port, root),
        Top::Hook {
            which: HookCmd::Pre { min_lines },
        } => {
            let mut input = String::new();
            let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut input);
            if let Some(json) = sym::hook::pre_read(&input, min_lines) {
                out(&json);
            }
            Ok(())
        }
    };
    if let Err(e) = res {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
