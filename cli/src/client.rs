//! Shared HTTP client for coordinator REST API and ledger REST API.

use anyhow::Result;
use reqwest::Client;

#[derive(Clone)]
pub struct ClientContext {
    pub coordinator_url: String,
    pub ledger_url: String,
    pub output_json: bool,
    pub account_id: Option<String>,
}

impl ClientContext {
    pub fn http(&self) -> Client {
        Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("HTTP client build")
    }

    pub fn coordinator_url(&self, path: &str) -> String {
        format!("{}{}", self.coordinator_url.trim_end_matches('/'), path)
    }

    pub fn ledger_url(&self, path: &str) -> String {
        format!("{}{}", self.ledger_url.trim_end_matches('/'), path)
    }

    pub fn require_account_id(&self) -> Result<&str> {
        self.account_id.as_deref()
            .ok_or_else(|| anyhow::anyhow!(
                "No account configured. Run `nm provider install` or set NM_ACCOUNT_ID."
            ))
    }
}
