use apt_fetcher::{SourceError, SourcesLists};
use std::{
    collections::{HashMap, HashSet},
    io,
};

#[derive(Debug, Error)]
pub enum RepoError {
    #[error(display = "failed to scan apt sources: {}", _0)]
    ListsScan(SourceError),
    #[error(display = "failed to find repo ({})", repo)]
    NotFound { repo: String },
    #[error(display = "error syncing lists to disk: {}", _0)]
    Sync(io::Error),
}

/// Modify repos on the system, using the repo instructions provided.
pub fn modify_repos(
    retain: &mut HashSet<Box<str>>,
    repos: &HashMap<&str, bool>,
) -> Result<(), RepoError> {
    let mut lists = SourcesLists::scan().map_err(RepoError::ListsScan)?;

    for (&repo, &enabled) in repos {
        if !lists.repo_modify(repo, enabled) {
            return Err(RepoError::NotFound { repo: repo.to_string() });
        }

        if enabled && !retain.contains(repo) {
            retain.insert(Box::from(repo));
        }
    }

    lists.write_sync().map_err(RepoError::Sync)
}
