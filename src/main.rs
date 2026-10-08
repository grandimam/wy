use anyhow::{Result, ensure};
use clap::{Args, Parser, Subcommand};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::{Arc, atomic::AtomicBool}};
use wy::{arr, s, history, presentation, reasoning, reflection, repository, service, storage::Store, trace};

#[derive(Parser)]
#[command(version, about = "Understand the engineering decisions behind AI-generated code")]
struct Cli {
    #[arg(long, global = true, default_value = ".")] repo: PathBuf,
    #[arg(long, global = true)] json: bool,
    #[command(subcommand)] command: Option<Command>,
}
#[derive(Args)]
struct History {
    #[arg(long, default_value = "both", value_parser = ["both", "codex", "claude", "none"])] source: String,
}
#[derive(Args)]
struct Reason {
    #[arg(long, default_value = "codex", value_parser = ["codex", "claude"])] agent: String,
    #[arg(long)] question: Option<String>,
    #[arg(long)] file: Option<String>,
    #[command(flatten)] history: History,
}
#[derive(Subcommand)]
enum Command {
    Snapshot,
    Review {
        #[arg(long, conflicts_with_all = ["base", "diff"])] baseline: Option<String>,
        #[arg(long, conflicts_with = "diff")] base: Option<String>,
        #[arg(long)] diff: Option<PathBuf>,
        #[arg(long)] session: Vec<String>,
        #[arg(long)] model: bool,
        #[command(flatten)] history: History,
    },
    Sessions { #[command(flatten)] history: History },
    Decisions,
    Gaps,
    Explain { target: String, #[arg(long)] show_code: bool },
    Ask { target: String, question: String, #[arg(long)] model: bool },
    Evidence { target: String, citation: String },
    Session { #[arg(long)] id: Option<String>, #[arg(long)] event: Option<String> },
    Reason { #[command(flatten)] options: Reason },
    Why { target: String, #[command(flatten)] options: Reason },
    ReasoningEvidence { citation: usize, #[arg(long)] id: Option<String> },
    Focus { target: String, question: String, #[arg(long)] evidence: Vec<String> },
    ReflectionRequest { target: Option<String> },
    RecordReflection { path: PathBuf },
}
fn main() {
    if let Err(error) = run(Cli::parse()) {
        eprintln!("Error: {}", wy::security::redact(&format!("{error:#}")));
        std::process::exit(1);
    }
}
fn run(cli: Cli) -> Result<()> {
    let root = repository::root(&cli.repo)?;
    let Some(command) = cli.command else {
        ensure!(!cli.json, "Choose a subcommand when using --json");
        return wy::tui::run(&root);
    };
    let value = match command {
        Command::Snapshot => service::snapshot(&root)?,
        Command::Review { baseline, base, diff, session, model, history: h } => {
            let sessions = history::resolve(&root, &session, &h.source)?;
            service::review(&root, &service::ReviewOptions { baseline, base, diff, sessions, source: h.source, model })?
        }
        Command::Sessions { history: h } => json!(history::discover(&root, &h.source, None, None)?),
        Command::Decisions => service::load(&root)?,
        Command::Gaps => {
            let review = service::load(&root)?;
            json!(arr(&review["decisions"]).iter().enumerate().filter(|(_, d)| d["stale"] == true || d["provenance"] == "unexplained" || !arr(&d["unresolved_questions"]).is_empty()).map(|(i,d)| json!({"number":i+1,"decision":d})).collect::<Vec<_>>())
        }
        Command::Explain { target, show_code } => {
            let review = service::load(&root)?;
            let d = service::select(&review, &target)?;
            if !cli.json { println!("{}", presentation::decision(d, show_code)); return Ok(()); }
            d.clone()
        }
        Command::Ask { target, question, model } => service::ask(service::select(&service::load(&root)?, &target)?, &question, model)?,
        Command::Evidence { target, citation } => {
            let review = service::load(&root)?;
            let d = service::select(&review, &target)?;
            trace::inspect(&review, d, trace::select(d, &citation)?)?
        }
        Command::Session { id, event } => {
            let sessions = history::saved(&service::load(&root)?)?;
            let selected: Vec<_> = sessions.into_iter().filter(|v| id.as_ref().is_none_or(|id| id == s(&v["id"]) || *id == format!("{}:{}",s(&v["agent"]),s(&v["id"])))).collect();
            ensure!(!selected.is_empty(), "No matching saved session");
            if let Some(event) = event { json!(selected.iter().flat_map(|v| arr(&v["events"])).filter(|v| v["id"] == event).collect::<Vec<_>>()) } else { json!(selected) }
        }
        Command::Reason { options } => explain(&root, options, None)?,
        Command::Why { target, options } => explain(&root, options, Some(target))?,
        Command::ReasoningEvidence { citation, id } => {
            let artifact = Store::open(&root)?.get("reasoning", id.as_deref().unwrap_or("latest"))?;
            let citations = presentation::citations(&artifact);
            citations.get(citation.wrapping_sub(1)).cloned().ok_or_else(|| anyhow::anyhow!("Citation number out of range"))?
        }
        Command::Focus { target, question, evidence } => reflection::focus(&root, &target, &question, &evidence)?,
        Command::ReflectionRequest { target } => reflection::request(&root, target.as_deref())?,
        Command::RecordReflection { path } => {
            ensure!(path.metadata()?.len() <= 5_000_000, "Reflection exceeds the 5 MB limit");
            reflection::record(&root, serde_json::from_str(&std::fs::read_to_string(path)?)?)?
        }
    };
    println!("{}", if cli.json { serde_json::to_string_pretty(&value)? } else { presentation::render(&value) });
    Ok(())
}
fn explain(root: &std::path::Path, options: Reason, target: Option<String>) -> Result<Value> {
    reasoning::run(root, &reasoning::Options {
        agent: options.agent,
        question: options.question.unwrap_or_else(|| if target.is_some() { "Why this design?".into() } else { reasoning::prompt("explain").into() }),
        file: options.file, target, source: options.history.source,
    }, &Arc::new(AtomicBool::new(false)), |message| eprintln!("{message}"))
}
