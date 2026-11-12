// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! Where summaries are filed: the GitHub REST API, or in tests a tracker
//! that records what it is given.
//!
//! The GitHub client calls two endpoints of the repository named in the
//! configuration, and nothing else:
//!
//! * `POST /repos/{owner}/{repo}/issues` with `title`, `body` and `labels`
//!   (answer 201 with the issue `number`);
//! * `POST /repos/{owner}/{repo}/issues/{number}/comments` with `body`
//!   (answer 201).
//!
//! The token is read from its file at every call, so it can be replaced
//! without a restart, and it goes only into the `Authorization` header. It
//! needs the "Issues: read and write" permission on that one repository.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::{Value, json};

use crate::config::GitHubConfig;
use crate::crypto::check_private;

/// Why filing did not succeed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrackerError {
    /// The tracker could not be reached or refused for now; try again
    /// later. The text is a short code such as `http-503` or `network`.
    Unavailable(String),
    /// The issue to comment on no longer exists; file a new issue instead.
    Gone,
}

/// A place summaries can be filed.
pub trait Tracker: Send + Sync {
    /// Opens an issue and returns its number.
    fn create_issue(&self, title: &str, body: &str, labels: &[String])
    -> Result<u64, TrackerError>;

    /// Adds a comment to an issue.
    fn comment(&self, issue: u64, body: &str) -> Result<(), TrackerError>;
}

/// The GitHub REST API.
pub struct GitHub {
    api_url: String,
    repository: String,
    token_file: PathBuf,
    agent: ureq::Agent,
}

impl std::fmt::Debug for GitHub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GitHub")
            .field("api_url", &self.api_url)
            .field("repository", &self.repository)
            .finish_non_exhaustive()
    }
}

/// The API version the requests are written against.
const API_VERSION: &str = "2022-11-28";

impl GitHub {
    /// A client for the repository in `config`.
    pub fn new(config: &GitHubConfig) -> Self {
        let agent_config = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(30)))
            .http_status_as_error(false)
            .max_redirects(0)
            .build();
        GitHub {
            api_url: config.api_url.clone(),
            repository: config.repository.clone(),
            token_file: config.token_file.clone(),
            agent: ureq::Agent::new_with_config(agent_config),
        }
    }

    /// Reads the token. It must be one line of printable characters in a
    /// file only its owner can read.
    pub fn token(&self) -> Result<String, TrackerError> {
        let unusable = || TrackerError::Unavailable("token-file".to_owned());
        check_private(&self.token_file).map_err(|_| unusable())?;
        let text = std::fs::read_to_string(&self.token_file).map_err(|_| unusable())?;
        let token = text.trim();
        if token.is_empty() || token.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err(unusable());
        }
        Ok(token.to_owned())
    }

    fn post(&self, path: &str, body: &Value) -> Result<(u16, Value), TrackerError> {
        let token = self.token()?;
        let url = format!("{}/repos/{}{path}", self.api_url, self.repository);
        let text = body.to_string();
        let mut response = self
            .agent
            .post(&url)
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", API_VERSION)
            .header("User-Agent", "tur-report-receiver")
            .header("Content-Type", "application/json")
            .send(text.as_bytes())
            .map_err(|_| TrackerError::Unavailable("network".to_owned()))?;
        let status = response.status().as_u16();
        let answer = response
            .body_mut()
            .read_to_string()
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or(Value::Null);
        Ok((status, answer))
    }
}

impl Tracker for GitHub {
    fn create_issue(
        &self,
        title: &str,
        body: &str,
        labels: &[String],
    ) -> Result<u64, TrackerError> {
        let (status, answer) = self.post(
            "/issues",
            &json!({ "title": title, "body": body, "labels": labels }),
        )?;
        if status != 201 {
            return Err(TrackerError::Unavailable(format!("http-{status}")));
        }
        answer
            .get("number")
            .and_then(Value::as_u64)
            .ok_or_else(|| TrackerError::Unavailable("no-issue-number".to_owned()))
    }

    fn comment(&self, issue: u64, body: &str) -> Result<(), TrackerError> {
        let (status, _) = self.post(
            &format!("/issues/{issue}/comments"),
            &json!({ "body": body }),
        )?;
        match status {
            201 => Ok(()),
            404 | 410 => Err(TrackerError::Gone),
            other => Err(TrackerError::Unavailable(format!("http-{other}"))),
        }
    }
}

/// One call a [`RecordingTracker`] received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    /// An issue was opened.
    Issue {
        /// Its number.
        number: u64,
        /// Its title.
        title: String,
        /// Its body.
        body: String,
        /// Its labels.
        labels: Vec<String>,
    },
    /// A comment was added.
    Comment {
        /// The issue.
        issue: u64,
        /// The comment.
        body: String,
    },
}

/// A tracker that keeps what it is given, and can be switched off. For
/// tests.
#[derive(Debug, Default)]
pub struct RecordingTracker {
    calls: Mutex<Vec<Call>>,
    issues: Mutex<BTreeMap<u64, bool>>,
    down: AtomicBool,
}

impl RecordingTracker {
    /// A tracker that is up and has no issues.
    pub fn new() -> Self {
        RecordingTracker::default()
    }

    /// Makes every call fail as if the tracker were unreachable, or not.
    pub fn set_down(&self, down: bool) {
        self.down.store(down, Ordering::SeqCst);
    }

    /// Deletes an issue, as a maintainer could.
    pub fn delete_issue(&self, number: u64) {
        if let Ok(mut issues) = self.issues.lock() {
            issues.insert(number, false);
        }
    }

    /// Every call so far.
    pub fn calls(&self) -> Vec<Call> {
        self.calls
            .lock()
            .map(|calls| calls.clone())
            .unwrap_or_default()
    }
}

impl Tracker for RecordingTracker {
    fn create_issue(
        &self,
        title: &str,
        body: &str,
        labels: &[String],
    ) -> Result<u64, TrackerError> {
        if self.down.load(Ordering::SeqCst) {
            return Err(TrackerError::Unavailable("network".to_owned()));
        }
        let mut issues = self
            .issues
            .lock()
            .map_err(|_| TrackerError::Unavailable("poisoned".to_owned()))?;
        let number = u64::try_from(issues.len()).unwrap_or(u64::MAX) + 1;
        issues.insert(number, true);
        if let Ok(mut calls) = self.calls.lock() {
            calls.push(Call::Issue {
                number,
                title: title.to_owned(),
                body: body.to_owned(),
                labels: labels.to_vec(),
            });
        }
        Ok(number)
    }

    fn comment(&self, issue: u64, body: &str) -> Result<(), TrackerError> {
        if self.down.load(Ordering::SeqCst) {
            return Err(TrackerError::Unavailable("network".to_owned()));
        }
        let exists = self
            .issues
            .lock()
            .map(|issues| issues.get(&issue).copied().unwrap_or(false))
            .unwrap_or(false);
        if !exists {
            return Err(TrackerError::Gone);
        }
        if let Ok(mut calls) = self.calls.lock() {
            calls.push(Call::Comment {
                issue,
                body: body.to_owned(),
            });
        }
        Ok(())
    }
}
