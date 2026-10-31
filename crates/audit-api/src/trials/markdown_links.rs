use std::{
    collections::{BTreeSet, HashMap, HashSet},
    fs,
    path::{Component, Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use git2::{Oid, Repository, TreeWalkMode, TreeWalkResult};
use ignore::WalkBuilder;
use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::models::{AuditFinding, MarkdownLinkMetric, Severity, TrialStatus};

/// Git file mode of a gitlink (submodule) tree/index entry.
const GITLINK_MODE: u32 = 0o160000;

/// Stable, machine-readable classification for a Markdown link.
///
/// `category()` is the finding `category`/id prefix; `is_blocking()` decides
/// whether the class must fail `audit links` and the pre-commit hook.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkClass {
    /// Local target exists and is part of the portable guidance corpus.
    GuidanceTarget,
    /// Local target does not exist.
    MissingTarget,
    /// Local target exists but is outside the portable guidance corpus
    /// (ticket/spec store, source code, generated artifact, unrelated file),
    /// and the link source is a guidance file.
    NonGuidanceTarget,
    /// Local target lives in an upward or sibling repository (a repository
    /// that is not the source repository or one of its descendant
    /// submodules).
    CrossRepository,
    /// http(s), mailto:, or data: destination; not resolved for existence.
    External,
    /// Pure `#fragment` destination with no path component.
    FragmentOnly,
    /// Destination uses a scheme/shape this audit does not support (e.g. a
    /// non-http(s) scheme, an absolute filesystem path, or a malformed
    /// destination).
    UnsupportedDependency,
    /// Local destination resolves outside the repository root entirely.
    UnsafePath,
    /// The Markdown source file could not be read as UTF-8 Markdown.
    UnreadableArtifact,
    /// Target is inside a descendant submodule but absent at the commit the
    /// parent records for that submodule.
    SubmoduleRevisionMissingTarget,
    /// The recorded submodule commit (or the submodule itself) is not
    /// available locally, so the target cannot be checked.
    SubmoduleRevisionUnavailable,
    /// Target is inside a nested repository that has no gitlink entry at the
    /// active revision (TD14).
    SubmoduleGitlinkMissing,
    /// `#fragment` matches no heading slug or HTML `id`/`name` in the target
    /// Markdown document (TD7).
    MissingAnchor,
    /// Opt-in external check: HTTP status >= 400 (except 429) or transport
    /// error (TD8).
    ExternalUnreachable,
    /// Opt-in external check: HTTP 429; reported but not blocking.
    ExternalRateLimited,
}

impl LinkClass {
    pub fn category(self) -> &'static str {
        match self {
            LinkClass::GuidanceTarget => "markdown_link_guidance_target",
            LinkClass::MissingTarget => "markdown_link_missing_target",
            LinkClass::NonGuidanceTarget => "markdown_link_non_guidance_target",
            LinkClass::CrossRepository => "markdown_link_cross_repository",
            LinkClass::External => "markdown_link_external",
            LinkClass::FragmentOnly => "markdown_link_fragment_only",
            LinkClass::UnsupportedDependency => "markdown_link_unsupported_dependency",
            LinkClass::UnsafePath => "markdown_link_unsafe_path",
            LinkClass::UnreadableArtifact => "markdown_link_unreadable_artifact",
            LinkClass::SubmoduleRevisionMissingTarget => "markdown_link_submodule_missing_target",
            LinkClass::SubmoduleRevisionUnavailable => {
                "markdown_link_submodule_revision_unavailable"
            }
            LinkClass::SubmoduleGitlinkMissing => "markdown_link_submodule_gitlink_missing",
            LinkClass::MissingAnchor => "markdown_link_missing_anchor",
            LinkClass::ExternalUnreachable => "markdown_link_external_unreachable",
            LinkClass::ExternalRateLimited => "markdown_link_external_rate_limited",
        }
    }

    /// Blocking classes fail `audit links` and the pre-commit hook.
    pub fn is_blocking(self) -> bool {
        matches!(
            self,
            LinkClass::MissingTarget
                | LinkClass::NonGuidanceTarget
                | LinkClass::CrossRepository
                | LinkClass::UnsupportedDependency
                | LinkClass::UnsafePath
                | LinkClass::UnreadableArtifact
                | LinkClass::SubmoduleRevisionMissingTarget
                | LinkClass::SubmoduleRevisionUnavailable
                | LinkClass::SubmoduleGitlinkMissing
                | LinkClass::MissingAnchor
                | LinkClass::ExternalUnreachable
        )
    }
}

/// Inputs for [`evaluate_with_options`].
#[derive(Debug, Clone, Default)]
pub struct EvaluateOptions {
    /// Lift the default `transcripts` / `.workflow-tools` source exclusions.
    pub include_historical: bool,
    /// Extra repo-relative paths to exclude from the source walk.
    pub exclude_paths: Vec<String>,
    /// Opt-in (TD8): issue HTTP requests for `http(s)` destinations.
    /// Default `false`; no network access happens unless enabled.
    pub check_external: bool,
}

pub struct MarkdownLinkResult {
    pub metric: MarkdownLinkMetric,
    pub findings: Vec<AuditFinding>,
}

enum DestinationShape<'a> {
    Local(&'a str),
    /// `http(s)` destination (candidate for the opt-in external check).
    Web,
    /// `mailto:` / `data:` destination; never resolved.
    OtherExternal,
    FragmentOnly,
    Unsupported,
}

/// Link anchors of one Markdown document (TD7).
#[derive(Default)]
struct Anchors {
    /// GitHub-style heading slugs, with duplicate suffixes applied.
    slugs: HashSet<String>,
    /// Explicit HTML `id=` / `name=` attribute values.
    ids: HashSet<String>,
}

impl Anchors {
    fn matches(&self, fragment: &str) -> bool {
        self.ids.contains(fragment)
            || self.slugs.contains(fragment)
            || self.slugs.contains(&fragment.to_lowercase())
    }
}

struct RawLink {
    line: usize,
    destination: String,
}

/// Result of the parallel read+parse phase for one source file.
struct ParsedSource {
    /// `None` when the file could not be read as UTF-8.
    links: Option<Vec<RawLink>>,
    /// Own anchors, kept only when a fragment-only link needs them.
    own_anchors: Option<Arc<Anchors>>,
}

/// GitHub-style heading slug: lowercase, keep alphanumerics, `-`, `_` and
/// spaces, then spaces become `-`.
fn slugify(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_'))
        .map(|c| if c == ' ' { '-' } else { c })
        .collect()
}

/// Collects `id="…"` / `name="…"` attribute values from a raw HTML chunk.
fn collect_html_ids(html: &str, ids: &mut HashSet<String>) {
    let bytes = html.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let rest = &html[index..];
        let name_len = if rest.starts_with("id") {
            2
        } else if rest.starts_with("name") {
            4
        } else {
            index += 1;
            continue;
        };
        let preceded_by_space = index > 0 && bytes[index - 1].is_ascii_whitespace();
        let mut cursor = index + name_len;
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if !preceded_by_space || cursor >= bytes.len() || bytes[cursor] != b'=' {
            index += name_len;
            continue;
        }
        cursor += 1;
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        let value = match bytes.get(cursor) {
            Some(&quote @ (b'"' | b'\'')) => {
                let start = cursor + 1;
                let len = html[start..]
                    .find(quote as char)
                    .unwrap_or(html.len() - start);
                cursor = start + len;
                &html[start..start + len]
            }
            Some(_) => {
                let start = cursor;
                while cursor < bytes.len()
                    && !bytes[cursor].is_ascii_whitespace()
                    && bytes[cursor] != b'>'
                {
                    cursor += 1;
                }
                &html[start..cursor]
            }
            None => "",
        };
        if !value.is_empty() {
            ids.insert(value.to_string());
        }
        index = cursor.max(index + name_len);
    }
}

/// One pass over a Markdown document: link destinations (inline, reference,
/// autolink, image) with their line numbers, plus its anchors.
fn parse_markdown(contents: &str) -> (Vec<RawLink>, Anchors) {
    let line_index = LineIndex::new(contents);
    let mut links = Vec::new();
    let mut anchors = Anchors::default();
    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut heading: Option<String> = None;
    for (event, range) in Parser::new(contents).into_offset_iter() {
        match event {
            Event::Start(Tag::Link { dest_url, .. })
            | Event::Start(Tag::Image { dest_url, .. }) => {
                links.push(RawLink {
                    line: line_index.line_number(range.start),
                    destination: dest_url.into_string(),
                });
            }
            Event::Start(Tag::Heading { .. }) => heading = Some(String::new()),
            Event::End(TagEnd::Heading(_)) => {
                if let Some(text) = heading.take() {
                    let base = slugify(&text);
                    let mut candidate = base.clone();
                    if anchors.slugs.contains(&candidate) {
                        let next = counts.entry(base.clone()).or_insert(1);
                        loop {
                            candidate = format!("{base}-{next}");
                            *next += 1;
                            if !anchors.slugs.contains(&candidate) {
                                break;
                            }
                        }
                    }
                    anchors.slugs.insert(candidate);
                }
            }
            Event::Text(text) | Event::Code(text) => {
                if let Some(buffer) = heading.as_mut() {
                    buffer.push_str(&text);
                }
            }
            Event::Html(html) | Event::InlineHtml(html) => {
                collect_html_ids(&html, &mut anchors.ids);
            }
            _ => {}
        }
    }
    (links, anchors)
}

/// `#L10`, `#L10-L20`, `#L10C2-L12C4`: GitHub line-range anchors, never
/// checked against headings.
fn is_line_anchor(fragment: &str) -> bool {
    let Some(rest) = fragment.strip_prefix('L') else {
        return false;
    };
    let mut seen_digit = false;
    for c in rest.chars() {
        match c {
            '0'..='9' => seen_digit = true,
            'C' | '-' | 'L' if seen_digit => {}
            _ => return false,
        }
    }
    seen_digit
}

fn decode_fragment(fragment: &str) -> String {
    let bytes = fragment.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(high), Some(low)) =
                (hex_digit(bytes[index + 1]), hex_digit(bytes[index + 2]))
        {
            out.push((high << 4) | low);
            index += 3;
            continue;
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Fragment of a destination, if it has a non-empty checkable one.
fn checkable_fragment(destination: &str) -> Option<String> {
    let (_, fragment) = destination.split_once('#')?;
    let decoded = decode_fragment(fragment);
    if decoded.is_empty() || is_line_anchor(&decoded) {
        None
    } else {
        Some(decoded)
    }
}

fn is_markdown_path(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
}

/// Per-target anchor cache keyed by `(repo node, revision?, path)`.
type AnchorCache = HashMap<(usize, Option<Oid>, String), Option<Arc<Anchors>>>;

/// Outcome of one external URL check.
enum ExternalOutcome {
    Reachable,
    RateLimited(u16),
    Unreachable(String),
}

struct PendingExternal {
    path: String,
    guidance: bool,
    repository: String,
    line: usize,
    destination: String,
}

const EXTERNAL_TIMEOUT: Duration = Duration::from_secs(10);
const EXTERNAL_CONCURRENCY: usize = 8;

fn check_url(client: &reqwest::blocking::Client, url: &str) -> ExternalOutcome {
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
fn check_urls(urls: &BTreeSet<String>) -> HashMap<String, ExternalOutcome> {
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

struct LineIndex {
    newlines: Vec<usize>,
}

impl LineIndex {
    fn new(contents: &str) -> Self {
        let mut newlines = Vec::new();
        for (offset, &byte) in contents.as_bytes().iter().enumerate() {
            if byte == b'\n' {
                newlines.push(offset);
            }
        }
        Self { newlines }
    }

    #[inline]
    fn line_number(&self, byte_offset: usize) -> usize {
        self.newlines.partition_point(|&pos| pos < byte_offset) + 1
    }
}

/// One repository in the scanned tree (the scanned root or a registered
/// submodule at any depth).
struct RepoNode {
    /// Path relative to the scanned root; `.` for the root.
    rel: String,
    abs: PathBuf,
    parent: Option<usize>,
    /// Commit recorded for this submodule by its parent (`None` for root).
    commit: Option<Oid>,
    repo: Option<Repository>,
    open_attempted: bool,
    /// Cached root tree of `commit` (children only).
    tree: Option<Result<Oid, String>>,
    /// Gitlinks as `(path inside this repo, commit)`, loaded once.
    gitlinks: Option<Vec<(String, Oid)>>,
}

#[derive(Clone)]
enum Attr {
    Node(usize),
    /// Directory of a nested repository lacking a gitlink (TD14).
    Unregistered(PathBuf),
}

struct RepoSet {
    root: PathBuf,
    nodes: Vec<RepoNode>,
    by_abs: HashMap<PathBuf, usize>,
    attr_cache: HashMap<PathBuf, Attr>,
    exists_cache: HashMap<(usize, String), bool>,
}

impl RepoSet {
    fn new(root: &Path) -> Self {
        let node = RepoNode {
            rel: ".".to_string(),
            abs: root.to_path_buf(),
            parent: None,
            commit: None,
            repo: None,
            open_attempted: false,
            tree: None,
            gitlinks: None,
        };
        Self {
            root: root.to_path_buf(),
            nodes: vec![node],
            by_abs: HashMap::from([(root.to_path_buf(), 0)]),
            attr_cache: HashMap::new(),
            exists_cache: HashMap::new(),
        }
    }

    fn ensure_open(&mut self, idx: usize) -> Result<(), String> {
        let node = &mut self.nodes[idx];
        if !node.open_attempted {
            node.open_attempted = true;
            node.repo = Repository::open(&node.abs).ok();
        }
        if node.repo.is_some() {
            Ok(())
        } else {
            Err(format!(
                "repository '{}' is not initialised or cannot be opened",
                node.rel
            ))
        }
    }

    /// Root tree of the commit recorded for a submodule node.
    fn root_tree(&mut self, idx: usize) -> Result<Oid, String> {
        if let Some(cached) = &self.nodes[idx].tree {
            return cached.clone();
        }
        let computed = self.load_root_tree(idx);
        self.nodes[idx].tree = Some(computed.clone());
        computed
    }

    fn load_root_tree(&mut self, idx: usize) -> Result<Oid, String> {
        self.ensure_open(idx)?;
        let node = &self.nodes[idx];
        let commit_id = node
            .commit
            .ok_or_else(|| format!("repository '{}' has no recorded revision", node.rel))?;
        let repo = node.repo.as_ref().expect("opened above");
        let commit = repo.find_commit(commit_id).map_err(|_| {
            format!(
                "commit {commit_id} recorded for submodule '{}' is not available locally",
                node.rel
            )
        })?;
        Ok(commit.tree_id())
    }

    /// Gitlinks of a repository: the index for the scanned root (TD1), the
    /// tree at the recorded commit for submodules.
    fn gitlinks(&mut self, idx: usize) -> Vec<(String, Oid)> {
        if let Some(cached) = &self.nodes[idx].gitlinks {
            return cached.clone();
        }
        let computed = self.load_gitlinks(idx).unwrap_or_default();
        self.nodes[idx].gitlinks = Some(computed.clone());
        computed
    }

    fn load_gitlinks(&mut self, idx: usize) -> Result<Vec<(String, Oid)>, String> {
        let mut links = Vec::new();
        if idx == 0 {
            self.ensure_open(0)?;
            let repo = self.nodes[0].repo.as_ref().expect("opened above");
            let index = repo.index().map_err(|e| e.to_string())?;
            for entry in index.iter() {
                if entry.mode == GITLINK_MODE {
                    links.push((String::from_utf8_lossy(&entry.path).into_owned(), entry.id));
                }
            }
        } else {
            let tree_id = self.root_tree(idx)?;
            let repo = self.nodes[idx].repo.as_ref().expect("opened by root_tree");
            let tree = repo.find_tree(tree_id).map_err(|e| e.to_string())?;
            let _ = tree.walk(TreeWalkMode::PreOrder, |dir, entry| {
                if entry.filemode() as u32 == GITLINK_MODE
                    && let Some(name) = entry.name()
                {
                    links.push((format!("{dir}{name}"), entry.id()));
                }
                TreeWalkResult::Ok
            });
        }
        Ok(links)
    }

    fn child_node(&mut self, parent: usize, gitlink_path: &str, commit: Oid) -> usize {
        let abs = self.nodes[parent].abs.join(gitlink_path);
        if let Some(&idx) = self.by_abs.get(&abs) {
            return idx;
        }
        let rel = abs
            .strip_prefix(&self.root)
            .unwrap_or(&abs)
            .to_string_lossy()
            .replace('\\', "/");
        let idx = self.nodes.len();
        self.nodes.push(RepoNode {
            rel,
            abs: abs.clone(),
            parent: Some(parent),
            commit: Some(commit),
            repo: None,
            open_attempted: false,
            tree: None,
            gitlinks: None,
        });
        self.by_abs.insert(abs, idx);
        idx
    }

    /// Innermost repository owning `dir`, or the unregistered nested
    /// repository boundary it sits inside.
    fn attribute(&mut self, dir: &Path) -> Attr {
        if let Some(cached) = self.attr_cache.get(dir) {
            return cached.clone();
        }
        let attr = if dir == self.root || !dir.starts_with(&self.root) {
            Attr::Node(0)
        } else {
            let parent = dir.parent().unwrap_or(&self.root).to_path_buf();
            match self.attribute(&parent) {
                Attr::Unregistered(p) => Attr::Unregistered(p),
                Attr::Node(parent_idx) => {
                    if dir.join(".git").exists() {
                        let rel = relative_slash(&self.nodes[parent_idx].abs, dir);
                        let found = self
                            .gitlinks(parent_idx)
                            .into_iter()
                            .find(|(path, _)| *path == rel);
                        match found {
                            Some((path, commit)) => {
                                Attr::Node(self.child_node(parent_idx, &path, commit))
                            }
                            None => Attr::Unregistered(dir.to_path_buf()),
                        }
                    } else {
                        Attr::Node(parent_idx)
                    }
                }
            }
        };
        self.attr_cache.insert(dir.to_path_buf(), attr.clone());
        attr
    }

    /// Resolves a target to its owning repository by following gitlinks from
    /// the root, returning the repository and the path inside it.
    fn descend(&mut self, target: &Path) -> (usize, String) {
        let mut current = 0usize;
        let mut remainder = relative_slash(&self.root, target);
        loop {
            let hit = self
                .gitlinks(current)
                .into_iter()
                .find(|(path, _)| remainder == *path || remainder.starts_with(&format!("{path}/")));
            let Some((path, commit)) = hit else {
                return (current, remainder);
            };
            let child = self.child_node(current, &path, commit);
            remainder = remainder[path.len()..].trim_start_matches('/').to_string();
            current = child;
        }
    }

    fn is_descendant(&self, node: usize, ancestor: usize) -> bool {
        let mut current = self.nodes[node].parent;
        while let Some(idx) = current {
            if idx == ancestor {
                return true;
            }
            current = self.nodes[idx].parent;
        }
        false
    }

    /// Existence of `path` (blob or tree) in the submodule's recorded tree.
    fn exists_at_revision(&mut self, idx: usize, path: &str) -> Result<bool, String> {
        let tree_id = self.root_tree(idx)?;
        if path.is_empty() {
            return Ok(true);
        }
        let key = (idx, path.to_string());
        if let Some(&cached) = self.exists_cache.get(&key) {
            return Ok(cached);
        }
        let repo = self.nodes[idx].repo.as_ref().expect("opened by root_tree");
        let tree = repo.find_tree(tree_id).map_err(|e| e.to_string())?;
        let exists = tree.get_path(Path::new(path)).is_ok();
        self.exists_cache.insert(key, exists);
        Ok(exists)
    }

    /// Contents of the blob at `path` in the submodule's recorded tree
    /// (`None` when absent or not a blob).
    fn blob_at_revision(&mut self, idx: usize, path: &str) -> Result<Option<Vec<u8>>, String> {
        let tree_id = self.root_tree(idx)?;
        let repo = self.nodes[idx].repo.as_ref().expect("opened by root_tree");
        let tree = repo.find_tree(tree_id).map_err(|e| e.to_string())?;
        let Ok(entry) = tree.get_path(Path::new(path)) else {
            return Ok(None);
        };
        match repo.find_blob(entry.id()) {
            Ok(blob) => Ok(Some(blob.content().to_vec())),
            Err(_) => Ok(None),
        }
    }
}

fn relative_slash(base: &Path, path: &Path) -> String {
    path.strip_prefix(base)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// A Markdown file to scan, attributed to its innermost repository.
struct SourceFile {
    rel: String,
    node: usize,
}

/// Identity of the source of a link, used for finding evidence.
struct Src<'a> {
    path: &'a str,
    guidance: bool,
    repository: &'a str,
}

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
    let parsed: Vec<ParsedSource> = sources
        .par_iter()
        .map(|source| {
            let Ok(contents) = fs::read_to_string(repo_root.join(&source.rel)) else {
                return ParsedSource {
                    links: None,
                    own_anchors: None,
                };
            };
            let (links, anchors) = parse_markdown(&contents);
            let needs_own = links.iter().any(|link| link.destination.starts_with('#'));
            ParsedSource {
                links: Some(links),
                own_anchors: needs_own.then(|| Arc::new(anchors)),
            }
        })
        .collect();

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

            let local_path = match classify_destination(destination) {
                DestinationShape::OtherExternal => {
                    skipped_links += 1;
                    continue;
                }
                DestinationShape::Web => {
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
                DestinationShape::FragmentOnly => {
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
                DestinationShape::Unsupported => {
                    skipped_links += 1;
                    unsupported_dependency_links += 1;
                    findings.push(unsupported_finding(&src, line, destination));
                    continue;
                }
                DestinationShape::Local(path) => path,
            };

            let target_path = lexical_normalize(
                &source_path
                    .parent()
                    .unwrap_or(repo_root)
                    .join(decode_percent_escapes(local_path)),
            );

            if !target_path.starts_with(repo_root) {
                skipped_links += 1;
                unsafe_path_links += 1;
                findings.push(unsafe_path_finding(&src, line, destination, &target_path));
                continue;
            }

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
    if !pending_external.is_empty() {
        let urls: BTreeSet<String> = pending_external
            .iter()
            .map(|pending| url_without_fragment(&pending.destination).to_string())
            .collect();
        let outcomes = check_urls(&urls);
        for pending in &pending_external {
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
    }

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

fn url_without_fragment(destination: &str) -> &str {
    destination.split('#').next().unwrap_or(destination)
}

/// Anchors of a Markdown target, computed once per `(repo, revision?, path)`.
/// Submodule targets are read from the blob at the recorded gitlink commit;
/// same-repository targets from the working tree. Unreadable or non-UTF-8
/// targets are not anchor-checked.
fn lookup_anchors(
    repos: &mut RepoSet,
    cache: &mut AnchorCache,
    node: usize,
    at_revision: bool,
    path_in_repo: &str,
    target_path: &Path,
) -> Option<Arc<Anchors>> {
    let revision = if at_revision {
        repos.nodes[node].commit
    } else {
        None
    };
    let key_path = if at_revision {
        path_in_repo.to_string()
    } else {
        target_path.to_string_lossy().into_owned()
    };
    let key = (node, revision, key_path);
    if let Some(cached) = cache.get(&key) {
        return cached.clone();
    }
    let contents = if at_revision {
        repos
            .blob_at_revision(node, path_in_repo)
            .ok()
            .flatten()
            .and_then(|bytes| String::from_utf8(bytes).ok())
    } else if target_path.is_file() {
        fs::read_to_string(target_path).ok()
    } else {
        None
    };
    let anchors = contents.map(|text| Arc::new(parse_markdown(&text).1));
    cache.insert(key, anchors.clone());
    anchors
}

fn missing_anchor_finding(
    src: &Src<'_>,
    line: usize,
    destination: &str,
    fragment: &str,
    target_relative: &str,
    revision: Option<String>,
) -> AuditFinding {
    let file = src.path;
    let mut evidence = Map::from_iter([
        ("fragment".to_string(), json!(fragment)),
        ("resolved_target".to_string(), json!(target_relative)),
    ]);
    if let Some(revision) = &revision {
        evidence.insert("revision".to_string(), json!(revision));
    }
    link_finding(
        LinkClass::MissingAnchor,
        src,
        line,
        destination,
        format!(
            "Markdown link in {file}:{line} points to anchor '#{fragment}' which does not exist in '{target_relative}'."
        ),
        vec![format!(
            "Update the fragment to match a current heading slug or HTML id/name in '{target_relative}'."
        )],
        evidence,
    )
}

fn external_finding(
    class: LinkClass,
    src: &Src<'_>,
    line: usize,
    destination: &str,
    reason: &str,
    status: Option<u16>,
) -> AuditFinding {
    let file = src.path;
    let mut evidence = Map::from_iter([("reason".to_string(), json!(reason))]);
    if let Some(status) = status {
        evidence.insert("status".to_string(), json!(status));
    }
    link_finding(
        class,
        src,
        line,
        destination,
        format!(
            "External link in {file}:{line} '{destination}' failed the opt-in check: {reason}."
        ),
        vec![format!(
            "Verify '{destination}' is reachable, or replace/remove the link in {file}:{line}."
        )],
        evidence,
    )
}

fn nearest_existing_dir(path: &Path) -> PathBuf {
    let mut current = path.to_path_buf();
    loop {
        if current.is_dir() {
            return current;
        }
        match current.parent() {
            Some(parent) => current = parent.to_path_buf(),
            None => return current,
        }
    }
}

/// Classifies a raw link destination before any filesystem resolution.
fn classify_destination(destination: &str) -> DestinationShape<'_> {
    let path = destination.split('#').next().unwrap_or(destination);
    if path.is_empty() {
        return DestinationShape::FragmentOnly;
    }
    if path.starts_with("mailto:") || path.starts_with("data:") {
        return DestinationShape::OtherExternal;
    }
    if path.starts_with("http://") || path.starts_with("https://") {
        return DestinationShape::Web;
    }
    if path.starts_with("//") || path.contains("://") {
        return DestinationShape::Unsupported;
    }
    if path.contains('<') || path.contains('>') {
        return DestinationShape::Unsupported;
    }
    if path.starts_with('/') {
        // Absolute filesystem path: not portable across checkouts/machines.
        return DestinationShape::Unsupported;
    }
    DestinationShape::Local(path)
}

/// The per-repository guidance set (A1): `.agents/**`, `AGENTS.md`,
/// `.github/copilot-instructions.md`, `.claude/**`. Paths are relative to the
/// owning repository root.
fn is_guidance_set(relative_path: &str) -> bool {
    let path = relative_path.replace('\\', "/");
    path == "AGENTS.md"
        || path.ends_with("/AGENTS.md")
        || path == ".github/copilot-instructions.md"
        || path == ".agents"
        || path.starts_with(".agents/")
        || path.contains("/.agents/")
        || path == ".claude"
        || path.starts_with(".claude/")
}

fn is_guidance_source(relative_path: &str) -> bool {
    is_guidance_set(relative_path)
}

/// Allowed targets of guidance sources: the guidance set, or any `README.md`
/// (D6).
fn is_guidance_target(relative_path: &str) -> bool {
    let path = relative_path.replace('\\', "/");
    is_guidance_set(&path) || path == "README.md" || path.ends_with("/README.md")
}

/// Best-effort human-readable subclass for a non-guidance target, used in
/// finding summaries/evidence only; does not affect the blocking decision.
fn detect_non_guidance_subclass(target_relative: &str) -> &'static str {
    if target_relative.starts_with(".ticket/") || target_relative.contains("/.ticket/") {
        "ticket_store"
    } else if target_relative.starts_with(".spec/") || target_relative.contains("/.spec/") {
        "spec_store"
    } else if target_relative.starts_with("target/") || target_relative.contains("/target/") {
        "generated_artifact"
    } else {
        match Path::new(target_relative)
            .extension()
            .and_then(|ext| ext.to_str())
        {
            Some(
                "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "toml" | "json" | "sh" | "ps1" | "yml"
                | "yaml",
            ) => "source_code",
            _ => "unrelated_repository_file",
        }
    }
}

fn link_finding(
    class: LinkClass,
    src: &Src<'_>,
    line: usize,
    destination: &str,
    summary: String,
    instructions: Vec<String>,
    mut evidence: Map<String, Value>,
) -> AuditFinding {
    let file = src.path;
    evidence.insert("source".to_string(), json!(file));
    evidence.insert("line".to_string(), json!(line));
    evidence.insert("target".to_string(), json!(destination));
    evidence.insert("classification".to_string(), json!(class.category()));
    evidence.insert("blocking".to_string(), json!(class.is_blocking()));
    evidence.insert("source_guidance".to_string(), json!(src.guidance));
    evidence.insert("source_repository".to_string(), json!(src.repository));
    AuditFinding {
        id: format!("{}:{file}:{line}:{destination}", class.category()),
        category: class.category().to_string(),
        severity: if class.is_blocking() {
            Severity::High
        } else {
            Severity::Low
        },
        summary,
        path: Some(file.to_string()),
        line: Some(line),
        metric_name: "markdown_link_coherence".to_string(),
        metric_value: json!(destination),
        threshold: None,
        instructions,
        evidence: Value::Object(evidence),
    }
}

fn missing_target_finding(
    src: &Src<'_>,
    line: usize,
    destination: &str,
    repo_root: &Path,
    target_path: &Path,
) -> AuditFinding {
    let file = src.path;
    let display_target = display_relative(repo_root, target_path);
    link_finding(
        LinkClass::MissingTarget,
        src,
        line,
        destination,
        format!("Markdown link in {file}:{line} points to missing target '{destination}'."),
        vec![format!(
            "Repair the Markdown link in {file}:{line} so it resolves to an existing target (currently '{display_target}')."
        )],
        Map::from_iter([("resolved_target".to_string(), json!(display_target))]),
    )
}

fn non_guidance_finding(
    src: &Src<'_>,
    line: usize,
    destination: &str,
    target_relative: &str,
) -> AuditFinding {
    let file = src.path;
    let detected_class = detect_non_guidance_subclass(target_relative);
    link_finding(
        LinkClass::NonGuidanceTarget,
        src,
        line,
        destination,
        format!(
            "Markdown link in {file}:{line} points to a non-guidance target '{target_relative}' ({detected_class})."
        ),
        vec![format!(
            "Remove the reference to '{target_relative}' from the portable guidance corpus, or replace it with a guidance-corpus pointer; do not link directly to {detected_class}."
        )],
        Map::from_iter([
            ("resolved_target".to_string(), json!(target_relative)),
            ("detected_class".to_string(), json!(detected_class)),
        ]),
    )
}

fn cross_repository_finding(
    src: &Src<'_>,
    line: usize,
    destination: &str,
    target_repository: &str,
) -> AuditFinding {
    let file = src.path;
    let source_repo_display = src.repository;
    link_finding(
        LinkClass::CrossRepository,
        src,
        line,
        destination,
        format!(
            "Markdown link in {file}:{line} crosses upward/sideways from repository '{source_repo_display}' to '{target_repository}'."
        ),
        vec![format!(
            "Only the same repository or its descendant submodules may be linked; replace '{destination}' with a link inside the repository or plain text."
        )],
        Map::from_iter([("target_repository".to_string(), json!(target_repository))]),
    )
}

fn unsupported_finding(src: &Src<'_>, line: usize, destination: &str) -> AuditFinding {
    let file = src.path;
    link_finding(
        LinkClass::UnsupportedDependency,
        src,
        line,
        destination,
        format!("Markdown link in {file}:{line} uses an unsupported destination '{destination}'."),
        vec![
            "Use a relative repository path, an http(s) URL, or a mailto:/data: destination instead.".to_string(),
        ],
        Map::new(),
    )
}

fn unsafe_path_finding(
    src: &Src<'_>,
    line: usize,
    destination: &str,
    target_path: &Path,
) -> AuditFinding {
    let file = src.path;
    link_finding(
        LinkClass::UnsafePath,
        src,
        line,
        destination,
        format!(
            "Markdown link in {file}:{line} resolves outside the repository root ('{destination}')."
        ),
        vec![format!(
            "Repair the Markdown link in {file}:{line} so it stays within the repository."
        )],
        Map::from_iter([(
            "resolved_target".to_string(),
            json!(target_path.to_string_lossy().replace('\\', "/")),
        )]),
    )
}

fn unreadable_source_finding(src: &Src<'_>) -> AuditFinding {
    let file = src.path;
    link_finding(
        LinkClass::UnreadableArtifact,
        src,
        0,
        "",
        format!("Markdown source '{file}' could not be read as UTF-8 Markdown."),
        vec![format!(
            "Ensure '{file}' is readable UTF-8 Markdown or exclude it from the audit."
        )],
        Map::new(),
    )
}

fn display_relative(repo_root: &Path, path: &Path) -> String {
    if path == repo_root {
        ".".to_string()
    } else {
        relative_slash(repo_root, path)
    }
}

/// Walks the scanned tree (gitignore respected, hidden included, symlinks
/// not followed) and attributes every `*.md` to its innermost repository.
/// Files inside unregistered nested repositories are dropped (TD14).
fn collect_sources(
    repo_root: &Path,
    options: &EvaluateOptions,
    repos: &mut RepoSet,
) -> Vec<SourceFile> {
    let mut walker = WalkBuilder::new(repo_root);
    walker
        .standard_filters(true)
        .hidden(false)
        .follow_links(false);
    let filter_root = repo_root.to_path_buf();
    let exclude_paths = options.exclude_paths.clone();
    let include_historical = options.include_historical;
    walker.filter_entry(move |entry| {
        let Ok(relative_path) = entry.path().strip_prefix(&filter_root) else {
            return true;
        };
        !is_excluded_path(relative_path, &exclude_paths, include_historical)
    });

    let mut files = Vec::new();
    for entry in walker.build().filter_map(Result::ok) {
        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
            continue;
        }
        let Ok(relative) = path.strip_prefix(repo_root) else {
            continue;
        };
        let dir = path.parent().unwrap_or(repo_root);
        if let Attr::Node(node) = repos.attribute(dir) {
            files.push(SourceFile {
                rel: relative.to_string_lossy().replace('\\', "/"),
                node,
            });
        }
    }
    files.sort_by(|a, b| a.rel.cmp(&b.rel));
    files
}

fn is_excluded_path(path: &Path, exclude_paths: &[String], include_historical: bool) -> bool {
    if let Some(file_name) = path.file_name().and_then(|name| name.to_str()) {
        if matches!(
            file_name,
            ".git"
                | "target"
                | "node_modules"
                | ".audit"
                | ".idea"
                | ".vscode"
                | ".ticket"
                | ".spec"
                | ".rule"
                | ".test"
                | ".session"
                | ".feedback"
                | ".worktrees"
                | ".cargo"
                | "dist"
                | "build"
        ) {
            return true;
        }
        if !include_historical && matches!(file_name, "transcripts" | ".workflow-tools") {
            return true;
        }
    }

    if !exclude_paths.is_empty() {
        let path_str = path.to_string_lossy();
        let normalized = path_str.replace('\\', "/");
        let trimmed = normalized.trim_matches('/');
        if exclude_paths.iter().any(|excluded| {
            let excluded = excluded.trim_matches('/');
            !excluded.is_empty()
                && (trimmed == excluded || trimmed.starts_with(&format!("{excluded}/")))
        }) {
            return true;
        }
    }

    false
}

fn lexical_normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            component => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

fn decode_percent_escapes(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = String::with_capacity(value.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let high = hex_digit(bytes[index + 1]);
            let low = hex_digit(bytes[index + 2]);
            if let (Some(high), Some(low)) = (high, low) {
                decoded.push(char::from((high << 4) | low));
                index += 3;
                continue;
            }
        }
        decoded.push(char::from(bytes[index]));
        index += 1;
    }
    decoded
}

fn hex_digit(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command;

    use tempfile::{TempDir, tempdir};

    use super::{EvaluateOptions, LinkClass, MarkdownLinkResult, evaluate, evaluate_with_options};

    fn git(dir: &Path, args: &[&str]) {
        let output = Command::new("git")
            .current_dir(dir)
            .args([
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "protocol.file.allow=always",
            ])
            .args(args)
            .output()
            .expect("git runs");
        assert!(
            output.status.success(),
            "git {args:?} failed in {}: {}",
            dir.display(),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn write(dir: &Path, rel: &str, contents: &str) {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    /// Initialises a repository with the given files and one commit.
    fn init_repo(files: &[(&str, &str)]) -> TempDir {
        let dir = tempdir().unwrap();
        git(dir.path(), &["init", "-q", "-b", "main"]);
        for (rel, contents) in files {
            write(dir.path(), rel, contents);
        }
        git(dir.path(), &["add", "-A"]);
        git(dir.path(), &["commit", "-q", "-m", "init"]);
        dir
    }

    fn add_submodule(parent: &Path, source: &Path, name: &str) {
        git(
            parent,
            &["submodule", "add", "-q", source.to_str().unwrap(), name],
        );
    }

    fn categories(result: &MarkdownLinkResult) -> Vec<&str> {
        result
            .findings
            .iter()
            .map(|f| f.category.as_str())
            .collect()
    }

    /// Root repo with `README.md` and a committed submodule `sub`.
    fn root_with_sub(root_readme: &str, sub_files: &[(&str, &str)]) -> (TempDir, TempDir) {
        let sub_src = init_repo(sub_files);
        let root = init_repo(&[("README.md", root_readme)]);
        add_submodule(root.path(), sub_src.path(), "sub");
        git(root.path(), &["commit", "-q", "-m", "add sub"]);
        (root, sub_src)
    }

    #[test]
    fn reports_broken_guidance_links_and_skips_non_local_links() {
        let repo = tempdir().unwrap();
        std::fs::create_dir_all(repo.path().join(".agents/instructions")).unwrap();
        std::fs::write(repo.path().join("README.md"), "# Repo\n").unwrap();
        std::fs::write(
            repo.path().join(".agents/instructions/links.md"),
            "[good](../../README.md)\n[bad](missing.md)\n[web](https://example.com)\n[anchor](#repo)\n\n# Repo\n",
        )
        .unwrap();

        let result = evaluate(repo.path(), &[]);

        assert_eq!(result.metric.markdown_files, 2);
        assert_eq!(result.metric.links_checked, 3);
        assert_eq!(result.metric.broken_links, 1);
        assert_eq!(result.metric.skipped_links, 1);
        assert_eq!(result.metric.blocking_findings, 1);
        assert_eq!(result.findings[0].line, Some(2));
        assert_eq!(
            result.findings[0].path.as_deref(),
            Some(".agents/instructions/links.md")
        );
        assert_eq!(
            result.findings[0].category,
            LinkClass::MissingTarget.category()
        );
    }

    #[test]
    fn nested_repository_without_gitlink_is_blocking_and_not_scanned() {
        let repo = tempdir().unwrap();
        std::fs::create_dir_all(repo.path().join(".agents")).unwrap();
        std::fs::create_dir_all(repo.path().join("nested/.git")).unwrap();
        std::fs::write(repo.path().join("nested/README.md"), "[b](gone.md)\n").unwrap();
        std::fs::write(
            repo.path().join(".agents/links.md"),
            "[nested](../nested/README.md)\n",
        )
        .unwrap();

        let result = evaluate(repo.path(), &[]);

        assert_eq!(result.metric.markdown_files, 1);
        assert_eq!(result.metric.links_checked, 0);
        assert_eq!(result.metric.blocking_findings, 1);
        assert_eq!(result.findings.len(), 1);
        assert_eq!(
            result.findings[0].category,
            LinkClass::SubmoduleGitlinkMissing.category()
        );
        assert_eq!(result.findings[0].evidence["source_repository"], ".");
        assert_eq!(result.findings[0].evidence["target_repository"], "nested");
    }

    #[test]
    fn blocks_links_to_non_guidance_local_targets() {
        let repo = tempdir().unwrap();
        std::fs::create_dir_all(repo.path().join(".agents")).unwrap();
        std::fs::create_dir_all(repo.path().join("src")).unwrap();
        std::fs::write(repo.path().join("src/lib.rs"), "fn main() {}\n").unwrap();
        std::fs::write(
            repo.path().join(".agents/links.md"),
            "[code](../src/lib.rs)\n",
        )
        .unwrap();

        let result = evaluate(repo.path(), &[]);

        assert_eq!(result.metric.non_guidance_links, 1);
        assert_eq!(result.metric.blocking_findings, 1);
        assert_eq!(
            result.findings[0].category,
            LinkClass::NonGuidanceTarget.category()
        );
        assert_eq!(result.findings[0].evidence["detected_class"], "source_code");
    }

    #[test]
    fn blocks_unsupported_and_unsafe_destinations() {
        let repo = tempdir().unwrap();
        std::fs::create_dir_all(repo.path().join(".agents")).unwrap();
        std::fs::write(
            repo.path().join(".agents/links.md"),
            "[ftp](ftp://example.com/file)\n[escape](../../../../outside.md)\n",
        )
        .unwrap();

        let result = evaluate(repo.path(), &[]);

        assert_eq!(result.metric.unsupported_dependency_links, 1);
        assert_eq!(result.metric.unsafe_path_links, 1);
        assert_eq!(result.metric.blocking_findings, 2);
        let categories: Vec<&str> = result
            .findings
            .iter()
            .map(|finding| finding.category.as_str())
            .collect();
        assert!(categories.contains(&LinkClass::UnsupportedDependency.category()));
        assert!(categories.contains(&LinkClass::UnsafePath.category()));
    }

    #[test]
    fn permits_non_markdown_files_under_agents_directory() {
        let repo = tempdir().unwrap();
        std::fs::create_dir_all(repo.path().join(".agents/skills")).unwrap();
        std::fs::write(repo.path().join(".agents/skills/icon.png"), b"\x89PNG").unwrap();
        std::fs::write(
            repo.path().join(".agents/links.md"),
            "[icon](skills/icon.png)\n",
        )
        .unwrap();

        let result = evaluate(repo.path(), &[]);

        assert_eq!(result.metric.links_checked, 1);
        assert_eq!(result.metric.blocking_findings, 0);
        assert!(result.findings.is_empty());
    }

    // 1
    #[test]
    fn submodule_target_present_at_gitlink_passes() {
        let (root, _sub) = root_with_sub(
            "[x](sub/docs/x.md)\n",
            &[("docs/x.md", "# x\n"), ("README.md", "# sub\n")],
        );
        let result = evaluate(root.path(), &[]);
        assert!(result.findings.is_empty(), "{:?}", categories(&result));
        assert_eq!(result.metric.links_checked, 1);
        assert!(result.metric.markdown_files >= 3);
    }

    // 2
    #[test]
    fn submodule_target_only_in_working_tree_is_missing_at_revision() {
        let (root, _sub) = root_with_sub("[x](sub/docs/new.md)\n", &[("README.md", "# sub\n")]);
        write(&root.path().join("sub"), "docs/new.md", "# uncommitted\n");
        let result = evaluate(root.path(), &[]);
        assert_eq!(
            categories(&result),
            vec![LinkClass::SubmoduleRevisionMissingTarget.category()]
        );
        assert_eq!(result.metric.blocking_findings, 1);
        assert_eq!(result.findings[0].path.as_deref(), Some("README.md"));
    }

    // 3
    #[test]
    fn submodule_upward_link_is_blocking_cross_repository() {
        let (root, _sub) = root_with_sub("# root\n", &[("docs/y.md", "[up](../../README.md)\n")]);
        let result = evaluate(root.path(), &[]);
        assert_eq!(
            categories(&result),
            vec![LinkClass::CrossRepository.category()]
        );
        assert_eq!(result.findings[0].evidence["blocking"], true);
        assert_eq!(result.findings[0].evidence["source_repository"], "sub");
        assert_eq!(result.findings[0].path.as_deref(), Some("sub/docs/y.md"));
    }

    // 4
    #[test]
    fn guidance_sources_may_only_target_guidance_or_readme() {
        let repo = tempdir().unwrap();
        write(repo.path(), "src/lib.rs", "fn main() {}\n");
        write(repo.path(), "README.md", "# r\n");
        write(repo.path(), ".github/copilot-instructions.md", "# c\n");
        write(
            repo.path(),
            ".agents/a.md",
            "[code](../src/lib.rs)\n[readme](../README.md)\n[cp](../.github/copilot-instructions.md)\n",
        );
        let result = evaluate(repo.path(), &[]);
        let hits: Vec<_> = result
            .findings
            .iter()
            .filter(|f| f.path.as_deref() == Some(".agents/a.md"))
            .collect();
        assert_eq!(hits.len(), 1, "{:?}", categories(&result));
        assert_eq!(hits[0].category, LinkClass::NonGuidanceTarget.category());
        assert_eq!(hits[0].line, Some(1));
    }

    // 5
    #[test]
    fn non_guidance_sources_may_link_anywhere_in_repository() {
        let repo = tempdir().unwrap();
        write(repo.path(), "src/lib.rs", "fn main() {}\n");
        write(repo.path(), "docs/a.md", "[code](../src/lib.rs)\n");
        let result = evaluate(repo.path(), &[]);
        assert!(result.findings.is_empty(), "{:?}", categories(&result));
        assert_eq!(result.metric.links_checked, 1);
    }

    // 6
    #[test]
    fn transcripts_are_excluded_unless_historical_included() {
        let repo = tempdir().unwrap();
        write(repo.path(), "transcripts/x.md", "[bad](missing.md)\n");
        write(repo.path(), ".workflow-tools/y.md", "[bad](missing.md)\n");
        let default = evaluate(repo.path(), &[]);
        assert!(default.findings.is_empty());
        assert_eq!(default.metric.markdown_files, 0);
        let historical = evaluate_with_options(
            repo.path(),
            &EvaluateOptions {
                include_historical: true,
                ..EvaluateOptions::default()
            },
        );
        assert_eq!(historical.findings.len(), 2);
        assert_eq!(historical.metric.markdown_files, 2);
    }

    // 7
    #[test]
    fn missing_image_target_is_reported() {
        let repo = tempdir().unwrap();
        write(repo.path(), "README.md", "![i](missing.png)\n");
        let result = evaluate(repo.path(), &[]);
        assert_eq!(
            categories(&result),
            vec![LinkClass::MissingTarget.category()]
        );
    }

    // 8
    #[cfg(unix)]
    #[test]
    fn symlinked_directories_are_not_scanned_twice() {
        let repo = tempdir().unwrap();
        write(repo.path(), ".agents/prompts/p.md", "[bad](missing.md)\n");
        std::fs::create_dir_all(repo.path().join(".claude")).unwrap();
        std::os::unix::fs::symlink("../.agents/prompts", repo.path().join(".claude/commands"))
            .unwrap();
        let result = evaluate(repo.path(), &[]);
        assert_eq!(result.metric.markdown_files, 1);
        assert_eq!(result.findings.len(), 1);
    }

    // 9
    #[test]
    fn nested_submodule_chain_resolves_via_intermediate_tree() {
        let inner = init_repo(&[("docs/z.md", "# z\n")]);
        let mid = init_repo(&[("README.md", "# mid\n")]);
        add_submodule(mid.path(), inner.path(), "inner");
        git(mid.path(), &["commit", "-q", "-m", "add inner"]);
        let root = init_repo(&[(
            "README.md",
            "[ok](mid/inner/docs/z.md)\n[no](mid/inner/docs/nope.md)\n",
        )]);
        add_submodule(root.path(), mid.path(), "mid");
        git(root.path(), &["commit", "-q", "-m", "add mid"]);
        git(
            root.path(),
            &["submodule", "update", "--init", "--recursive", "-q"],
        );
        let result = evaluate(root.path(), &[]);
        assert_eq!(
            categories(&result),
            vec![LinkClass::SubmoduleRevisionMissingTarget.category()]
        );
        assert_eq!(result.findings[0].line, Some(2));
        assert_eq!(result.metric.links_checked, 2);
    }

    // 10
    #[test]
    fn findings_carry_blocking_guidance_and_repository_evidence() {
        let (root, _sub) = root_with_sub(
            "[nope](nope.md)\n",
            &[
                (".agents/s.md", "[m](missing.md)\n"),
                ("up.md", "[u](../README.md)\n"),
            ],
        );
        let result = evaluate(root.path(), &[]);
        assert_eq!(result.findings.len(), 3, "{:?}", categories(&result));
        for finding in &result.findings {
            let ev = &finding.evidence;
            let class_blocking = [LinkClass::MissingTarget, LinkClass::CrossRepository]
                .iter()
                .find(|c| c.category() == finding.category)
                .map(|c| c.is_blocking())
                .unwrap();
            assert_eq!(ev["blocking"], class_blocking);
            match finding.path.as_deref().unwrap() {
                "README.md" => {
                    assert_eq!(ev["source_guidance"], false);
                    assert_eq!(ev["source_repository"], ".");
                }
                "sub/.agents/s.md" => {
                    assert_eq!(ev["source_guidance"], true);
                    assert_eq!(ev["source_repository"], "sub");
                }
                "sub/up.md" => {
                    assert_eq!(ev["source_guidance"], false);
                    assert_eq!(ev["source_repository"], "sub");
                }
                other => panic!("unexpected source {other}"),
            }
        }
    }

    // 11
    #[test]
    fn nested_repo_never_added_to_index_is_gitlink_missing() {
        let root = init_repo(&[("README.md", "[x](nested/x.md)\n")]);
        let nested = root.path().join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        git(&nested, &["init", "-q", "-b", "main"]);
        write(&nested, "x.md", "[broken](missing.md)\n");
        let result = evaluate(root.path(), &[]);
        assert_eq!(
            categories(&result),
            vec![LinkClass::SubmoduleGitlinkMissing.category()]
        );
        assert_eq!(result.metric.markdown_files, 1);
    }

    // 12
    #[test]
    fn intermediate_tree_without_inner_gitlink_is_gitlink_missing() {
        let inner = init_repo(&[("z.md", "[broken](missing.md)\n")]);
        let mid = init_repo(&[("README.md", "# mid\n")]);
        let root = init_repo(&[("README.md", "[z](mid/inner/z.md)\n")]);
        add_submodule(root.path(), mid.path(), "mid");
        git(root.path(), &["commit", "-q", "-m", "add mid"]);
        // Inner repo exists on disk and is staged only in mid's index.
        add_submodule(&root.path().join("mid"), inner.path(), "inner");
        let result = evaluate(root.path(), &[]);
        assert_eq!(
            categories(&result),
            vec![LinkClass::SubmoduleGitlinkMissing.category()]
        );
        assert!(
            result.findings.iter().all(|f| !f
                .path
                .as_deref()
                .unwrap_or("")
                .starts_with("mid/inner"))
        );
    }

    fn anchor_result(target: &str, link: &str) -> MarkdownLinkResult {
        let repo = init_repo(&[("other.md", target), ("a.md", link), ("code.rs", "//\n")]);
        evaluate(repo.path(), &[])
    }

    // Part 02 test 1
    #[test]
    fn heading_slug_with_parentheses_matches() {
        let result = anchor_result(
            "## Ambiguity Resolution Gate (before prompt-ingestion Stage 2)\n",
            "[a](other.md#ambiguity-resolution-gate-before-prompt-ingestion-stage-2)\n",
        );
        assert!(result.findings.is_empty(), "{:?}", categories(&result));
    }

    // Part 02 test 2
    #[test]
    fn heading_slug_drops_dots_in_file_names() {
        let result = anchor_result(
            "## audio-transcript.instructions.md Stage 1\n",
            "[a](other.md#audio-transcriptinstructionsmd-stage-1)\n",
        );
        assert!(result.findings.is_empty(), "{:?}", categories(&result));
    }

    // Part 02 test 3
    #[test]
    fn duplicate_headings_get_numeric_suffixes() {
        let result = anchor_result(
            "# x\n\n# x\n",
            "[a](other.md#x) [b](other.md#x-1) [c](other.md#x-2)\n",
        );
        assert_eq!(
            categories(&result),
            vec![LinkClass::MissingAnchor.category()]
        );
        assert_eq!(result.findings[0].evidence["fragment"], "x-2");
        assert_eq!(result.metric.blocking_findings, 1);
    }

    // Part 02 test 4
    #[test]
    fn html_id_and_name_attributes_are_anchors() {
        let result = anchor_result(
            "<a id=\"custom\"></a>\n\ntext <a name='legacy'>x</a> and <span id=bare>y</span>\n",
            "[a](other.md#custom) [b](other.md#legacy) [c](other.md#bare) [d](other.md#nope)\n",
        );
        assert_eq!(
            categories(&result),
            vec![LinkClass::MissingAnchor.category()]
        );
        assert_eq!(result.findings[0].evidence["fragment"], "nope");
    }

    // Part 02 test 5
    #[test]
    fn fragment_only_missing_is_blocking_and_line_anchors_are_unchecked() {
        let repo = init_repo(&[
            ("code.rs", "//\n"),
            (
                "a.md",
                "# Title\n[ok](#title) [bad](#missing)\n[l](code.rs#L10) [r](code.rs#L10-L20) [m](a.md#L3)\n[x](code.rs#anything)\n",
            ),
        ]);
        let result = evaluate(repo.path(), &[]);
        assert_eq!(
            categories(&result),
            vec![LinkClass::MissingAnchor.category()]
        );
        assert_eq!(result.findings[0].evidence["fragment"], "missing");
        assert_eq!(result.findings[0].line, Some(2));
        assert_eq!(result.metric.blocking_findings, 1);
    }

    // Part 02 test 6
    #[test]
    fn submodule_anchor_is_read_from_gitlink_blob_not_working_tree() {
        let (root, _sub) = root_with_sub(
            "[old](sub/docs/x.md#old-heading)\n[new](sub/docs/x.md#new-heading)\n",
            &[("docs/x.md", "# Old Heading\n"), ("README.md", "# sub\n")],
        );
        write(&root.path().join("sub"), "docs/x.md", "# New Heading\n");
        let result = evaluate(root.path(), &[]);
        assert_eq!(
            categories(&result),
            vec![LinkClass::MissingAnchor.category()]
        );
        assert_eq!(result.findings[0].evidence["fragment"], "new-heading");
        assert_eq!(result.findings[0].line, Some(2));
        assert!(result.findings[0].evidence["revision"].is_string());
    }

    /// Minimal local HTTP stub; records `METHOD /path` per request.
    fn spawn_stub() -> (String, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let thread_log = log.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let log = thread_log.clone();
                std::thread::spawn(move || {
                    let mut buffer = Vec::new();
                    let mut chunk = [0u8; 512];
                    while !buffer.windows(4).any(|w| w == b"\r\n\r\n") {
                        match stream.read(&mut chunk) {
                            Ok(0) | Err(_) => return,
                            Ok(n) => buffer.extend_from_slice(&chunk[..n]),
                        }
                    }
                    let text = String::from_utf8_lossy(&buffer).into_owned();
                    let mut parts = text.lines().next().unwrap_or("").split_whitespace();
                    let method = parts.next().unwrap_or("").to_string();
                    let path = parts.next().unwrap_or("").to_string();
                    log.lock().unwrap().push(format!("{method} {path}"));
                    let status = match (method.as_str(), path.as_str()) {
                        (_, "/missing") => "404 Not Found",
                        (_, "/limited") => "429 Too Many Requests",
                        ("HEAD", "/head405") => "405 Method Not Allowed",
                        _ => "200 OK",
                    };
                    let _ = write!(
                        stream,
                        "HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    );
                });
            }
        });
        (base, log)
    }

    // Part 02 test 7
    #[test]
    fn external_links_are_checked_only_when_opted_in() {
        let (base, log) = spawn_stub();
        let readme = format!(
            "[a]({base}/ok) [b]({base}/missing) [c]({base}/limited) [d]({base}/head405) [e]({base}/ok#frag)\n"
        );
        let repo = init_repo(&[("README.md", readme.as_str())]);

        let off = evaluate_with_options(repo.path(), &EvaluateOptions::default());
        assert!(off.findings.is_empty());
        assert!(log.lock().unwrap().is_empty(), "no request when disabled");

        let on = evaluate_with_options(
            repo.path(),
            &EvaluateOptions {
                check_external: true,
                ..EvaluateOptions::default()
            },
        );
        assert_eq!(
            categories(&on),
            vec![
                LinkClass::ExternalRateLimited.category(),
                LinkClass::ExternalUnreachable.category()
            ]
        );
        assert_eq!(on.findings[0].evidence["blocking"], false);
        assert_eq!(on.findings[1].evidence["blocking"], true);
        assert_eq!(on.metric.blocking_findings, 1);
        let requests = log.lock().unwrap().clone();
        let count = |needle: &str| requests.iter().filter(|r| r.as_str() == needle).count();
        assert_eq!(count("HEAD /ok"), 1, "URLs are de-duplicated: {requests:?}");
        assert_eq!(count("HEAD /head405"), 1);
        assert_eq!(count("GET /head405"), 1, "GET fallback after 405");
        assert_eq!(count("GET /missing"), 0);
    }

    // Part 02 test 8
    #[test]
    fn finding_order_is_deterministic() {
        let mut files: Vec<(String, String)> = (0..40)
            .map(|i| {
                (
                    format!("docs/d{i}.md"),
                    "[a](#nope) [b](missing.md) [c](#also)\n".to_string(),
                )
            })
            .collect();
        files.push(("README.md".to_string(), "# r\n".to_string()));
        let refs: Vec<(&str, &str)> = files
            .iter()
            .map(|(p, c)| (p.as_str(), c.as_str()))
            .collect();
        let repo = init_repo(&refs);
        let ids =
            |r: &MarkdownLinkResult| r.findings.iter().map(|f| f.id.clone()).collect::<Vec<_>>();
        let first = ids(&evaluate(repo.path(), &[]));
        assert_eq!(first.len(), 120);
        assert_eq!(first, ids(&evaluate(repo.path(), &[])));
        let mut sorted = first.clone();
        sorted.sort_by_key(|id| id.split(':').nth(1).map(str::to_string));
        assert_eq!(
            first
                .iter()
                .map(|id| id.split(':').nth(1))
                .collect::<Vec<_>>(),
            sorted
                .iter()
                .map(|id| id.split(':').nth(1))
                .collect::<Vec<_>>()
        );
    }
}
