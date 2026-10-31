use anyhow::{Context, Result};
use clap::Parser;
use cli::Cli;
use lazykatok::{
    config::KatokConfig,
    paths::{default_data_dir, ensure_private_dir, resolve_semantic_dir},
};

mod account;
mod cli;
mod commands;
mod support;

fn main() {
    let cli = Cli::parse();
    let json = commands::command_requests_json(&cli.command);
    if let Err(error) = run(cli) {
        if json {
            let code = error_code(&error);
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "ok": false,
                    "error": {
                        "code": code,
                        "message": error.to_string(),
                        "cause": format!("{error:#}")
                    }
                }))
                .unwrap_or_else(|_| {
                    "{\"ok\":false,\"error\":{\"code\":\"serialization_failed\"}}".to_string()
                })
            );
        } else {
            eprintln!("Error: {error:#}");
        }
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    let config = KatokConfig::load(cli.config.as_deref())
        .context("load config")?
        .with_env_overrides();
    let base_data_dir = match cli.data_dir {
        Some(path) => path,
        None => default_data_dir().context("resolve default data directory")?,
    };
    let account = match cli.account {
        Some(choice) => Some(account::resolve(&base_data_dir, &choice)?),
        None if account::wants_prompt(&cli.command, &config) => account::prompt(&base_data_dir)?,
        None => None,
    };
    let data_dir = match &account {
        Some(account) => {
            // Every reader in this process, and the `send` child the reply TUI starts, takes
            // the account from here.
            std::env::set_var("KATOK_KAKAO_USER_ID", account.user_id.to_string());
            account.data_dir.clone()
        }
        None => base_data_dir,
    };
    ensure_private_dir(&data_dir).context("create private data directory")?;
    let archive_path = data_dir.join("archive.sqlite3");
    let semantic_dir =
        resolve_semantic_dir(&data_dir, &config.semantic_dir).context("resolve semantic dir")?;

    commands::run(cli.command, config, data_dir, archive_path, semantic_dir)
}

fn error_code(error: &anyhow::Error) -> &'static str {
    match error.downcast_ref::<lazykatok::Error>() {
        Some(lazykatok::Error::SemanticIndexMissing) => "semantic_index_missing",
        Some(lazykatok::Error::SemanticIndexStale(_)) => "semantic_index_stale",
        Some(lazykatok::Error::SemanticIndexBusy(_)) => "semantic_index_busy",
        Some(lazykatok::Error::EmptyQuery) => "empty_query",
        Some(lazykatok::Error::Sql(_)) => "sqlite_error",
        _ => "command_failed",
    }
}
