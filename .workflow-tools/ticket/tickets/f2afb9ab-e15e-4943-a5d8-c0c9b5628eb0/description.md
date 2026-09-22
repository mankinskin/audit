## Goal
Add a repository-level configuration file for the repo QA tool so users can exclude paths from audits.

## Scope
- Auto-load a repo-root config file for repo-qa
- Support exclude paths such as `crates/deps/`
- Apply excludes at sync/index time so file-based metrics and findings omit those paths
- Add focused regression coverage
