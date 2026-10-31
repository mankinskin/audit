//! Link-coherence evaluation over a repository and its submodules.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use super::anchors::{AnchorCache, lookup_anchors};
use super::classify::{Disposition, disposition};
use super::external::{PendingExternal, check_pending_external};
use super::findings::*;
use super::model::{LinkClass, MarkdownLinkResult};
use super::options::EvaluateOptions;
use super::repos::{Attr, RepoSet, display_relative, nearest_existing_dir, relative_slash};
use super::scan::parse_sources;
use super::sources::collect_sources;
use super::targets::{is_guidance_source, is_guidance_target, is_markdown_path, resolve_target};
use crate::markdown::checkable_fragment;
use crate::models::{AuditFinding, MarkdownLinkMetric, TrialStatus};

/// Validates every Markdown link in the repository and its submodules with
/// default options plus the given extra exclusions.
pub fn evaluate(repo_root: &Path, exclude_paths: &[String]) -> MarkdownLinkResult {
    evaluate_with_options(
        repo_root,
        &EvaluateOptions {
            include_historical: false,
            exclude_paths: exclude_paths.to_vec(),
            check_external: false,
        },
    )
}

pub fn evaluate_with_options(repo_root: &Path, options: &EvaluateOptions) -> MarkdownLinkResult {
    let mut repos = RepoSet::new(repo_root);
    let sources = collect_sources(repo_root, options, &mut repos);
    let mut links_checked = 0usize;
    let mut broken_links = 0usize;
    let mut skipped_links = 0usize;
    let mut non_guidance_links = 0usize;
    let mut cross_repository_links = 0usize;
    let mut unsupported_dependency_links = 0usize;
    let mut unsafe_path_links = 0usize;
    let mut unreadable_links = 0usize;
    let mut findings: Vec<AuditFinding> = Vec::new();
    let mut existence_cache = HashMap::<PathBuf, bool>::new();
    let mut anchor_cache = AnchorCache::new();
    let mut pending_external: Vec<PendingExternal> = Vec::new();

    // Parallel phase (TD9): read + parse every source once.
    let parsed = parse_sources(repo_root, &sources);

    // Sequential phase: classification needs the (non-`Sync`) git handles.
    for (source, parsed_source) in sources.iter().zip(parsed) {
        let file = source.rel.as_str();
        let source_path = repo_root.join(file);
        let source_repository = repos.nodes[source.node].rel.clone();
        let source_repo_abs = repos.nodes[source.node].abs.clone();
        let in_repo = relative_slash(&source_repo_abs, &source_path);
        let src = Src {
            path: file,
            guidance: is_guidance_source(&in_repo),
            repository: &source_repository,
        };
        let Some(links) = parsed_source.links else {
            unreadable_links += 1;
            findings.push(unreadable_source_finding(&src));
            continue;
        };
        let own_anchors = parsed_source.own_anchors;

        for raw_link in &links {
            let destination = raw_link.destination.as_str();
            let line = raw_link.line;
            let findings_before = findings.len();

            let local_path = match disposition(destination) {
                Disposition::Skip => {
                    skipped_links += 1;
                    continue;
                }
                Disposition::Web => {
                    skipped_links += 1;
                    if options.check_external {
                        pending_external.push(PendingExternal {
                            path: file.to_string(),
                            guidance: src.guidance,
                            repository: source_repository.clone(),
                            line,
                            destination: destination.to_string(),
                        });
                    }
                    continue;
                }
                Disposition::FragmentOnly => {
                    links_checked += 1;
                    if let (Some(fragment), Some(anchors)) =
                        (checkable_fragment(destination), own_anchors.as_ref())
                        && !anchors.matches(&fragment)
                    {
                        broken_links += 1;
                        findings.push(missing_anchor_finding(
                            &src,
                            line,
                            destination,
                            &fragment,
                            file,
                            None,
                        ));
                    }
                    continue;
                }
                Disposition::Unsupported => {
                    skipped_links += 1;
                    unsupported_dependency_links += 1;
                    findings.push(unsupported_finding(&src, line, destination));
                    continue;
                }
                Disposition::Local(path) => path,
            };

            let target_path = match resolve_target(repo_root, file, &local_path) {
                Ok(target_path) => target_path,
                Err(escaped) => {
                    skipped_links += 1;
                    unsafe_path_links += 1;
                    findings.push(unsafe_path_finding(&src, line, destination, &escaped));
                    continue;
                }
            };

            let (target_node, remainder) = repos.descend(&target_path);

            // TD14: a nested repository without a gitlink is not a valid
            // submodule at the active revision.
            let existing_dir = nearest_existing_dir(&target_path);
            if let Attr::Unregistered(boundary) = repos.attribute(&existing_dir) {
                skipped_links += 1;
                broken_links += 1;
                let boundary_rel = display_relative(repo_root, &boundary);
                findings.push(link_finding(
                    LinkClass::SubmoduleGitlinkMissing,
                    &src,
                    line,
                    destination,
                    format!(
                        "Markdown link in {file}:{line} points into nested repository '{boundary_rel}', which has no gitlink at the active revision."
                    ),
                    vec![format!(
                        "Register '{boundary_rel}' as a submodule of its parent repository, or remove the link to '{destination}'."
                    )],
                    Map::from_iter([("target_repository".to_string(), json!(boundary_rel))]),
                ));
                continue;
            }

            let target_relative = relative_slash(repo_root, &target_path);

            if target_node == source.node {
                links_checked += 1;
                let target_exists = *existence_cache
                    .entry(target_path.clone())
                    .or_insert_with(|| target_path.exists());
                if !target_exists {
                    broken_links += 1;
                    findings.push(missing_target_finding(
                        &src,
                        line,
                        destination,
                        repo_root,
                        &target_path,
                    ));
                    continue;
                }
                let in_target_repo = relative_slash(&source_repo_abs, &target_path);
                if src.guidance && !is_guidance_target(&in_target_repo) {
                    non_guidance_links += 1;
                    findings.push(non_guidance_finding(
                        &src,
                        line,
                        destination,
                        &target_relative,
                    ));
                }
            } else if repos.is_descendant(target_node, source.node) {
                links_checked += 1;
                let submodule = repos.nodes[target_node].rel.clone();
                let revision = repos.nodes[target_node]
                    .commit
                    .map(|oid| oid.to_string())
                    .unwrap_or_default();
                match repos.exists_at_revision(target_node, &remainder) {
                    Err(reason) => {
                        broken_links += 1;
                        findings.push(link_finding(
                            LinkClass::SubmoduleRevisionUnavailable,
                            &src,
                            line,
                            destination,
                            format!(
                                "Markdown link in {file}:{line} cannot be checked: {reason}."
                            ),
                            vec![format!(
                                "Initialise submodule '{submodule}' and fetch revision {revision} (for example `git submodule update --init`)."
                            )],
                            Map::from_iter([
                                ("submodule".to_string(), json!(submodule)),
                                ("revision".to_string(), json!(revision)),
                                ("resolved_target".to_string(), json!(target_relative)),
                            ]),
                        ));
                    }
                    Ok(false) => {
                        broken_links += 1;
                        findings.push(link_finding(
                            LinkClass::SubmoduleRevisionMissingTarget,
                            &src,
                            line,
                            destination,
                            format!(
                                "Markdown link in {file}:{line} points to '{remainder}' which does not exist in submodule '{submodule}' at recorded revision {revision}."
                            ),
                            vec![format!(
                                "Commit '{remainder}' in submodule '{submodule}' and update the parent gitlink, or repair the link."
                            )],
                            Map::from_iter([
                                ("submodule".to_string(), json!(submodule)),
                                ("revision".to_string(), json!(revision)),
                                ("resolved_target".to_string(), json!(target_relative)),
                            ]),
                        ));
                    }
                    Ok(true) => {
                        if src.guidance && !is_guidance_target(&remainder) {
                            non_guidance_links += 1;
                            findings.push(non_guidance_finding(
                                &src,
                                line,
                                destination,
                                &target_relative,
                            ));
                        }
                    }
                }
            } else {
                skipped_links += 1;
                cross_repository_links += 1;
                let target_repository = repos.nodes[target_node].rel.clone();
                findings.push(cross_repository_finding(
                    &src,
                    line,
                    destination,
                    &target_repository,
                ));
            }

            // TD7: anchor check once the target itself resolved cleanly.
            if findings.len() == findings_before
                && is_markdown_path(&remainder)
                && let Some(fragment) = checkable_fragment(destination)
            {
                let at_revision = target_node != source.node;
                let anchors = lookup_anchors(
                    &mut repos,
                    &mut anchor_cache,
                    target_node,
                    at_revision,
                    &remainder,
                    &target_path,
                );
                if let Some(anchors) = anchors
                    && !anchors.matches(&fragment)
                {
                    broken_links += 1;
                    let revision = at_revision
                        .then(|| repos.nodes[target_node].commit.map(|oid| oid.to_string()))
                        .flatten();
                    findings.push(missing_anchor_finding(
                        &src,
                        line,
                        destination,
                        &fragment,
                        &target_relative,
                        revision,
                    ));
                }
            }
        }
    }

    // Opt-in external checks (TD8): one request chain per distinct URL.
    let (external_checked, external_broken) =
        check_pending_external(&pending_external, &mut findings);
    links_checked += external_checked;
    broken_links += external_broken;

    // Deterministic output regardless of scheduling (TD9).
    findings.sort_by(|a, b| {
        (&a.path, a.line, &a.category, a.metric_value.to_string()).cmp(&(
            &b.path,
            b.line,
            &b.category,
            b.metric_value.to_string(),
        ))
    });

    let blocking_findings = findings
        .iter()
        .filter(|finding| {
            finding
                .evidence
                .get("blocking")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .count();

    MarkdownLinkResult {
        metric: MarkdownLinkMetric {
            status: TrialStatus::Collected,
            markdown_files: sources.len(),
            links_checked,
            broken_links,
            skipped_links,
            blocking_findings,
            non_guidance_links,
            cross_repository_links,
            unsupported_dependency_links,
            unsafe_path_links,
            unreadable_links,
            details: None,
        },
        findings,
    }
}
