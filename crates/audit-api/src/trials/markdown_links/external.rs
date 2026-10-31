//! Opt-in external URL checks (TD8).

use std::collections::{BTreeSet, HashMap};
use std::time::Duration;

use rayon::prelude::*;

use super::findings::{Src, external_finding};
use super::model::LinkClass;
use crate::models::AuditFinding;

/// Outcome of one external URL check.
pub(super) enum ExternalOutcome {
    Reachable,
    RateLimited(u16),
    Unreachable(String),
}

pub(super) struct PendingExternal {
    pub(super) path: String,
    pub(super) guidance: bool,
    pub(super) repository: String,
    pub(super) line: usize,
    pub(super) destination: String,
}

pub(super) const EXTERNAL_TIMEOUT: Duration = Duration::from_secs(10);
pub(super) const EXTERNAL_CONCURRENCY: usize = 8;

pub(super) fn check_url(client: &reqwest::blocking::Client, url: &str) -> ExternalOutcome {
    let status_of = |result: Result<reqwest::blocking::Response, reqwest::Error>| {
        result.map(|response| response.status().as_u16())
    };
    let mut status = status_of(client.head(url).send());
    if matches!(status, Ok(403 | 405)) {
        status = status_of(client.get(url).send());
    }
    match status {
        Ok(429) => ExternalOutcome::RateLimited(429),
        Ok(code) if code >= 400 => ExternalOutcome::Unreachable(format!("HTTP status {code}")),
        Ok(_) => ExternalOutcome::Reachable,
        Err(error) => ExternalOutcome::Unreachable(format!("request failed: {error}")),
    }
}

/// Checks each distinct URL once, with at most [`EXTERNAL_CONCURRENCY`]
/// requests in flight.
pub(super) fn check_urls(urls: &BTreeSet<String>) -> HashMap<String, ExternalOutcome> {
    let client = reqwest::blocking::Client::builder()
        .timeout(EXTERNAL_TIMEOUT)
        .user_agent("audit-markdown-links")
        .build();
    let client = match client {
        Ok(client) => client,
        Err(error) => {
            return urls
                .iter()
                .map(|url| {
                    (
                        url.clone(),
                        ExternalOutcome::Unreachable(format!("HTTP client unavailable: {error}")),
                    )
                })
                .collect();
        }
    };
    let list: Vec<&String> = urls.iter().collect();
    let run = || -> HashMap<String, ExternalOutcome> {
        list.par_iter()
            .map(|url| ((*url).clone(), check_url(&client, url)))
            .collect()
    };
    match rayon::ThreadPoolBuilder::new()
        .num_threads(EXTERNAL_CONCURRENCY)
        .build()
    {
        Ok(pool) => pool.install(run),
        Err(_) => list
            .iter()
            .map(|url| ((*url).clone(), check_url(&client, url)))
            .collect(),
    }
}

/// Checks every pending URL once and appends findings. Returns
/// `(links_checked, broken_links)` contributed by the external phase.
pub(super) fn check_pending_external(
    pending_external: &[PendingExternal],
    findings: &mut Vec<AuditFinding>,
) -> (usize, usize) {
    let mut links_checked = 0usize;
    let mut broken_links = 0usize;
    if pending_external.is_empty() {
        return (links_checked, broken_links);
    }
    let urls: BTreeSet<String> = pending_external
        .iter()
        .map(|pending| url_without_fragment(&pending.destination).to_string())
        .collect();
    let outcomes = check_urls(&urls);
    for pending in pending_external {
        let url = url_without_fragment(&pending.destination);
        let src = Src {
            path: &pending.path,
            guidance: pending.guidance,
            repository: &pending.repository,
        };
        links_checked += 1;
        match outcomes.get(url) {
            Some(ExternalOutcome::Unreachable(reason)) => {
                broken_links += 1;
                findings.push(external_finding(
                    LinkClass::ExternalUnreachable,
                    &src,
                    pending.line,
                    &pending.destination,
                    reason,
                    None,
                ));
            }
            Some(ExternalOutcome::RateLimited(status)) => {
                findings.push(external_finding(
                    LinkClass::ExternalRateLimited,
                    &src,
                    pending.line,
                    &pending.destination,
                    "HTTP status 429 (rate limited)",
                    Some(*status),
                ));
            }
            Some(ExternalOutcome::Reachable) | None => {}
        }
    }
    (links_checked, broken_links)
}

pub(super) fn url_without_fragment(destination: &str) -> &str {
    destination.split('#').next().unwrap_or(destination)
}
