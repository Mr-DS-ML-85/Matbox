use clap::{Parser, Subcommand};
use matbox_core::Engine;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "matbox", version, about = "MATrix BOX — MATLAB replica in Rust")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Start an interactive REPL session
    Repl,
    /// Run a .mbox script file
    Run {
        /// Path to .mbox file
        file: PathBuf,
    },
    /// Check syntax of a .mbox file without executing
    Check {
        /// Path to .mbox file
        file: PathBuf,
    },
    /// Print version info
    Version,
    /// Start the LSP server
    Lsp,
}

fn main() {
    let cli = Cli::parse();

    match cli.command.unwrap_or(Command::Repl) {
        Command::Repl => repl(),
        Command::Run { file } => run_file(&file),
        Command::Check { file } => check_file(&file),
        Command::Version => version(),
        Command::Lsp => lsp(),
    }
}

fn repl() {
    let mut engine = Engine::new();
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let mut buf = String::new();

    println!("MATBOX v{} — type 'exit' to quit", env!("CARGO_PKG_VERSION"));
    loop {
        print!(">> ");
        stdout.flush().unwrap();
        buf.clear();
        let n = stdin.lock().read_line(&mut buf).unwrap();
        if n == 0 { break; }
        let trimmed = buf.trim();
        if trimmed.is_empty() { continue; }
        if matches!(trimmed, "exit" | "quit" | "q") { break; }

        let mut input = trimmed.to_string();
        loop {
            match engine.eval(&input) {
                Ok(Some(v)) if is_displayable(&v) => {
                    println!("ans =\n{}", matbox_core::builtins::format_value(&v));
                    break;
                }
                Ok(_) => break,
                Err(e) => {
                    let msg = format!("{}", e);
                    if msg.to_lowercase().contains("eof")
                        || msg.to_lowercase().contains("unexpected end")
                    {
                        print!(".. ");
                        stdout.flush().unwrap();
                        let mut extra = String::new();
                        if stdin.lock().read_line(&mut extra).unwrap() == 0 { break; }
                        input.push(' ');
                        input.push_str(extra.trim());
                    } else {
                        eprintln!("Error: {}", msg);
                        break;
                    }
                }
            }
        }
    }
}

fn run_file(path: &PathBuf) {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error reading '{}': {}", path.display(), e);
            std::process::exit(1);
        }
    };
    let mut engine = Engine::new();
    match engine.eval(&content) {
        Ok(Some(v)) if is_displayable(&v) => {
            println!("ans =\n{}", matbox_core::builtins::format_value(&v))
        }
        Ok(_) => {}
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}

/// Void builtins (`disp`, `clear`, `who`, ...) yield an empty array as a
/// "nothing" sentinel; such results should not be echoed as `ans`.
fn is_displayable(v: &matbox_core::Value) -> bool {
    !matches!(v, matbox_core::Value::Array(a) if a.numel() == 0)
}

fn check_file(path: &PathBuf) {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error reading '{}': {}", path.display(), e);
            std::process::exit(1);
        }
    };
    let mut p = match matbox_core::Parser::new(&content) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Syntax error: {}", e);
            std::process::exit(1);
        }
    };
    match p.parse_program() {
        Ok(_) => {
            println!("✓ Syntax OK — {}", path.display());
        }
        Err(e) => {
            eprintln!("Syntax error: {}", e);
            std::process::exit(1);
        }
    }
}

fn version() {
    println!("MATBOX v{}", env!("CARGO_PKG_VERSION"));
    println!("Language: MATBOX .mbox (MATLAB-compatible)");
    println!("Engine:   Rust (pure, no Python/C/C++)");
    println!("License:  AGPL-3.0-or-later");
}

fn lsp() {
    // Delegate to matbox-lsp crate
    matbox_lsp::run();
}
