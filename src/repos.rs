use apt_fetcher::{SourceError, SourcesLists};
use std::{collections::HashMap, io};

#[derive(Debug, Error)]
pub enum RepoError {
    #[error(display = "failed to scan apt sources: {}", _0)]
    ListsScan(SourceError),
    #[error(display = "failed to find repo ({})", repo)]
    NotFound { repo: String },
    #[error(display = "error syncing lists to disk: {}", _0)]
    Sync(io::Error)
}

/// Modify repos on the system, using the repo instructions provided.
pub fn modify_repos(repos: &HashMap<&str, u8>) -> Result<(), RepoError> {
    let mut lists = SourcesLists::scan().map_err(RepoError::ListsScan)?;

    for (repo, status) in repos {
        if !lists.repo_modify(repo, *status != 0) {
            return Err(RepoError::NotFound {
                repo: repo.to_string(),
            });
        }
    }

    lists.write_sync().map_err(RepoError::Sync)
}
