//! `--account` resolution and the account prompt shown before `watch --select`.

use std::io::{self, BufRead, IsTerminal, Write};
use std::path::Path;

use anyhow::{Context, Result};
use lazykatok::config::KatokConfig;
use lazykatok::kakao::accounts::{choose_account, list_accounts, Account, AccountChoice};
use lazykatok::kakao::AuthOptions;

use crate::cli::Commands;

fn options(base: &Path) -> Result<AuthOptions> {
    let home = lazykatok::kakao::default_home().context("resolve home directory")?;
    Ok(AuthOptions::new(home, base.to_path_buf()))
}

pub(crate) fn resolve(base: &Path, choice: &AccountChoice) -> Result<Account> {
    let accounts = list_accounts(&options(base)?).context("list KakaoTalk accounts")?;
    Ok(choose_account(&accounts, choice)?)
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
pub(crate) fn prompt(base: &Path) -> Result<Option<Account>> {
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
    resolve_account_selection(input.trim(), &accounts).map(Some)
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
    use std::path::PathBuf;

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
