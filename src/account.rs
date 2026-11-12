//! `--account` resolution and the account prompt shown before `watch --select`.

use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use lazykatok::config::KatokConfig;
use lazykatok::kakao::accounts::{
    choose_account, list_accounts, pinned_account_data_dir, Account, AccountChoice,
};
use lazykatok::kakao::AuthOptions;

use crate::cli::Commands;

fn options(base: &Path) -> Result<AuthOptions> {
    let home = lazykatok::kakao::default_home().context("resolve home directory")?;
    Ok(AuthOptions::new(home, base.to_path_buf()))
}

/// Whether `command` never opens a KakaoTalk database. Only such a command may take a pinned
/// data dir without verifying the account's key: a reader handed a pin that no longer verifies
/// falls back to whichever account does, and would read it into this account's archive.
fn skips_kakao_database(command: &Commands) -> bool {
    #[cfg(all(target_os = "macos", feature = "private-send"))]
    if matches!(command, Commands::Send { .. }) {
        return true;
    }
    let _ = command;
    false
}

/// The account a run reads: its user id and the data dir that holds its archive.
pub(crate) struct Selected {
    pub(crate) user_id: i64,
    pub(crate) data_dir: PathBuf,
}

impl From<Account> for Selected {
    fn from(account: Account) -> Self {
        Self {
            user_id: account.user_id,
            data_dir: account.data_dir,
        }
    }
}

/// `--account`. For `send`, which reads only katok's own archive, a user id katok has already
/// read into a data dir is taken from that dir's pin. Everything else lists the accounts, which
/// verifies every key.
pub(crate) fn resolve(base: &Path, choice: &AccountChoice, command: &Commands) -> Result<Selected> {
    resolve_with(base, choice, command, || options(base))
}

fn resolve_with(
    base: &Path,
    choice: &AccountChoice,
    command: &Commands,
    options: impl FnOnce() -> Result<AuthOptions>,
) -> Result<Selected> {
    if let AccountChoice::UserId(user_id) = *choice {
        if skips_kakao_database(command) {
            if let Some(data_dir) = pinned_account_data_dir(base, user_id) {
                return Ok(Selected { user_id, data_dir });
            }
        }
    }
    let accounts = list_accounts(&options()?).context("list KakaoTalk accounts")?;
    Ok(choose_account(&accounts, choice)?.into())
}

/// `watch --select` on the macOS source, in a terminal: the one place a person picks a chat,
/// so it is also where they pick the account.
pub(crate) fn wants_prompt(command: &Commands, config: &KatokConfig) -> bool {
    let Commands::Watch {
        select: true,
        source,
        ..
    } = command
    else {
        return false;
    };
    let source = source.as_deref().unwrap_or(&config.source_adapter);
    matches!(source, "macos" | "kakao") && io::stdin().is_terminal()
}

/// Ask which account to read when more than one opens. `None` when there is nothing to ask.
pub(crate) fn prompt(base: &Path) -> Result<Option<Selected>> {
    let accounts = list_accounts(&options(base)?).context("list KakaoTalk accounts")?;
    if accounts.len() < 2 {
        return Ok(None);
    }
    let mut stderr = io::stderr().lock();
    write_account_list(&mut stderr, &accounts)?;
    stderr.flush().context("flush account prompt")?;
    let mut input = String::new();
    io::stdin()
        .lock()
        .read_line(&mut input)
        .context("read selected account")?;
    resolve_account_selection(input.trim(), &accounts).map(|account| Some(account.into()))
}

pub(crate) fn describe(account: &Account) -> String {
    let last = account
        .last_modified
        .map(|at| {
            at.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| "unknown".to_string());
    format!(
        "{}{}  last write {}  databases {}",
        account.user_id,
        if account.latest { "  (latest)" } else { "" },
        last,
        account.databases
    )
}

fn write_account_list(out: &mut impl Write, accounts: &[Account]) -> Result<()> {
    writeln!(out, "Choose a KakaoTalk account:").context("write account prompt")?;
    for (index, account) in accounts.iter().enumerate() {
        writeln!(out, "  {}. {}", index + 1, describe(account)).context("write account")?;
    }
    write!(out, "account number or user id [1]> ").context("write account input prompt")?;
    Ok(())
}

/// An empty answer takes the first row, the most recently written account.
fn resolve_account_selection(input: &str, accounts: &[Account]) -> Result<Account> {
    if input.is_empty() {
        return accounts.first().cloned().context("no account to choose");
    }
    if let Ok(number) = input.parse::<usize>() {
        if (1..=accounts.len()).contains(&number) {
            return Ok(accounts[number - 1].clone());
        }
    }
    let choice: AccountChoice = input
        .parse()
        .map_err(|error: String| anyhow::anyhow!(error))?;
    Ok(choose_account(accounts, &choice)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Cli;
    use clap::Parser;
    use std::path::PathBuf;

    fn command(args: &[&str]) -> Commands {
        let mut argv = vec!["lazykatok"];
        argv.extend(args);
        Cli::try_parse_from(argv).expect("parse").command
    }

    /// A base dir pinned to `user_id`, and a home with no KakaoTalk database at all, so a
    /// listing can only fail.
    fn pinned_base(user_id: i64) -> (tempfile::TempDir, tempfile::TempDir) {
        let base = tempfile::tempdir().expect("base");
        std::fs::create_dir_all(base.path().join("kakao")).expect("mkdir");
        std::fs::write(
            base.path().join("kakao").join("auth.json"),
            format!(r#"{{"user_id": {user_id}, "uuid": "00000000-0000-0000-0000-000000000000"}}"#),
        )
        .expect("pin");
        (base, tempfile::tempdir().expect("home"))
    }

    fn resolve_in(
        base: &Path,
        home: &Path,
        choice: AccountChoice,
        command: &Commands,
    ) -> Result<Selected> {
        resolve_with(base, &choice, command, || {
            Ok(AuthOptions::new(home.to_path_buf(), base.to_path_buf()))
        })
    }

    #[cfg(all(target_os = "macos", feature = "private-send"))]
    #[test]
    fn send_takes_the_pinned_dir_without_listing_accounts() {
        let (base, home) = pinned_base(1001);
        let send = command(&["send", "--chat", "1", "--dry-run"]);
        let selected = resolve_in(base.path(), home.path(), AccountChoice::UserId(1001), &send)
            .expect("no database is needed for a pinned send");
        assert_eq!(selected.user_id, 1001);
        assert_eq!(selected.data_dir, base.path());

        // An id nothing is pinned to still has to be verified, and there is nothing to verify.
        assert!(resolve_in(base.path(), home.path(), AccountChoice::UserId(2002), &send).is_err());
        assert!(resolve_in(base.path(), home.path(), AccountChoice::Latest, &send).is_err());
    }

    #[test]
    fn a_command_that_reads_kakaotalk_always_verifies_the_account() {
        let (base, home) = pinned_base(1001);
        let reader = command(&["accounts"]);
        // Pinned, but the key opens nothing: the reader is refused rather than handed the dir.
        assert!(resolve_in(
            base.path(),
            home.path(),
            AccountChoice::UserId(1001),
            &reader
        )
        .is_err());
    }

    fn account(user_id: i64, latest: bool) -> Account {
        Account {
            user_id,
            latest,
            databases: 2,
            last_modified: None,
            data_dir: PathBuf::from(format!("/synthetic/{user_id}")),
        }
    }

    #[test]
    fn selection_takes_a_row_number_a_user_id_or_the_first_row() {
        let accounts = [account(1001, true), account(2002, false)];

        assert_eq!(
            resolve_account_selection("", &accounts).unwrap().user_id,
            1001
        );
        assert_eq!(
            resolve_account_selection("2", &accounts).unwrap().user_id,
            2002
        );
        assert_eq!(
            resolve_account_selection("2002", &accounts)
                .unwrap()
                .user_id,
            2002
        );
        assert_eq!(
            resolve_account_selection("latest", &accounts)
                .unwrap()
                .user_id,
            1001
        );
        assert!(resolve_account_selection("3", &accounts).is_err());
        assert!(resolve_account_selection("x", &accounts).is_err());
    }

    #[test]
    fn the_list_numbers_rows_and_marks_the_latest_account() {
        let mut out = Vec::new();
        write_account_list(&mut out, &[account(1001, true), account(2002, false)]).unwrap();
        let text = String::from_utf8(out).unwrap();

        assert!(text.contains("  1. 1001  (latest)"));
        assert!(text.contains("  2. 2002  last write unknown"));
    }
}
