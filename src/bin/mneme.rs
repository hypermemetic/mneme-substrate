//! mneme — CLI for inspecting program directories and (eventually) running
//! skill invocations.
//!
//! Phase 4c scaffold. Implements the read-only subcommands that don't need
//! the substrate runtime:
//!
//!   mneme inspect <program-id>        Show manifest + artifact for a program
//!   mneme programs list               List all programs under the root
//!   mneme programs trace <program-id> Pretty-print trace.jsonl
//!
//! The `mneme run <skill.method>` subcommand is deferred until SwarmRuntime
//! is wired to a real ClaudeCode handle (see mneme-substrate's
//! src/mneme/runtime/swarm_runtime.rs doc comments).

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "mneme", version, about = "Skill orchestration on Plexus RPC")]
struct Cli {
    /// Filesystem root containing programs/<id>/ directories.
    #[arg(long, global = true, default_value = "./programs")]
    programs_root: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Show the manifest + artifact for a program.
    Inspect {
        /// Program id (UUID).
        program_id: String,
    },
    /// Subcommands operating over the programs directory as a whole.
    Programs {
        #[command(subcommand)]
        action: ProgramsAction,
    },
    /// Run a skill invocation (NOT IMPLEMENTED — needs SwarmRuntime wiring).
    Run {
        /// Method like `forecast.update` or `ticketing.write`.
        method: String,
        /// Pass-through args (TBD format once runtime is wired).
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}

#[derive(Subcommand, Debug)]
enum ProgramsAction {
    /// List all programs under `--programs-root`.
    List,
    /// Pretty-print `trace.jsonl` for a program.
    Trace {
        /// Program id (UUID).
        program_id: String,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("mneme: error: {}", e);
            ExitCode::from(1)
        }
    }
}

fn run(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    match &cli.command {
        Command::Inspect { program_id } => cmd_inspect(&cli.programs_root, program_id),
        Command::Programs {
            action: ProgramsAction::List,
        } => cmd_programs_list(&cli.programs_root),
        Command::Programs {
            action: ProgramsAction::Trace { program_id },
        } => cmd_programs_trace(&cli.programs_root, program_id),
        Command::Run { method, args: _ } => {
            eprintln!(
                "mneme run is not yet implemented; needs SwarmRuntime wired to a real ClaudeCode handle.\n\
                 See mneme-substrate's src/mneme/runtime/swarm_runtime.rs for the integration plan.\n\
                 For now, use synapse against a running mneme-substrate:\n\
                 \n\
                 \tsynapse -P 4444 {}",
                method
            );
            Err("run subcommand not implemented".into())
        }
    }
}

fn cmd_inspect(root: &Path, program_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let dir = root.join(program_id);
    if !dir.is_dir() {
        return Err(format!("no program at {}", dir.display()).into());
    }

    let manifest_path = dir.join("manifest.json");
    let manifest = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("reading {}: {}", manifest_path.display(), e))?;
    println!("=== manifest ({}) ===", manifest_path.display());
    println!("{}", pretty_json(&manifest)?);

    let artifact_path = dir.join("artifact.json");
    if artifact_path.exists() {
        let artifact = std::fs::read_to_string(&artifact_path)?;
        println!();
        println!("=== artifact ({}) ===", artifact_path.display());
        println!("{}", pretty_json(&artifact)?);
    } else {
        let error_path = dir.join("error.json");
        if error_path.exists() {
            let err = std::fs::read_to_string(&error_path)?;
            println!();
            println!("=== error ({}) ===", error_path.display());
            println!("{}", pretty_json(&err)?);
        } else {
            println!();
            println!("(no artifact.json or error.json yet — program may still be running)");
        }
    }

    Ok(())
}

fn cmd_programs_list(root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if !root.is_dir() {
        eprintln!("(no programs directory at {})", root.display());
        return Ok(());
    }

    let mut entries: Vec<_> = std::fs::read_dir(root)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter(|e| {
            // Skip _calibration and other underscore-prefixed dirs.
            e.file_name().to_string_lossy().chars().next() != Some('_')
        })
        .collect();
    entries.sort_by_key(|e| e.file_name());

    if entries.is_empty() {
        println!("(no programs)");
        return Ok(());
    }

    println!("{:<40} {:<12} {:<22} {}", "PROGRAM_ID", "STATUS", "STARTED", "ENTRY_SKILL");
    for entry in entries {
        let id = entry.file_name().to_string_lossy().to_string();
        let manifest_path = entry.path().join("manifest.json");
        let (status, started, entry_skill) = if let Ok(text) = std::fs::read_to_string(&manifest_path)
        {
            match serde_json::from_str::<serde_json::Value>(&text) {
                Ok(v) => {
                    let status = v["status"].as_str().unwrap_or("?").to_string();
                    let started = v["started_at"].as_str().unwrap_or("?").to_string();
                    let entry = v["entry_skill"].as_str().unwrap_or("?").to_string();
                    (status, started, entry)
                }
                Err(_) => ("malformed".into(), "-".into(), "-".into()),
            }
        } else {
            ("missing".into(), "-".into(), "-".into())
        };
        println!("{:<40} {:<12} {:<22} {}", id, status, started, entry_skill);
    }

    Ok(())
}

fn cmd_programs_trace(root: &Path, program_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let trace_path = root.join(program_id).join("trace.jsonl");
    if !trace_path.exists() {
        return Err(format!("no trace at {}", trace_path.display()).into());
    }
    let text = std::fs::read_to_string(&trace_path)?;
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<serde_json::Value>(line) {
            Ok(v) => println!("{}", serde_json::to_string_pretty(&v)?),
            Err(_) => println!("(malformed) {}", line),
        }
        println!("---");
    }
    Ok(())
}

fn pretty_json(text: &str) -> Result<String, Box<dyn std::error::Error>> {
    let value: serde_json::Value = serde_json::from_str(text)?;
    Ok(serde_json::to_string_pretty(&value)?)
}
