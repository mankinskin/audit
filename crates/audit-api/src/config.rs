use std::{fs, path::Path};

use serde::Deserialize;

use crate::error::AuditError;

pub const CONFIG_FILE_NAME: &str = ".audit.toml";

#[derive(Debug, Clone, Default, Deserialize)]
pub struct AuditFileConfig {
    #[serde(default)]
    pub exclude_paths: Vec<String>,
    /// Lift the default `transcripts` / `.workflow-tools` link-source exclusions.
    #[serde(default)]
    pub include_historical: bool,
    /// Opt in to HTTP checks for external Markdown links.
    #[serde(default)]
    pub check_external: bool,
}

impl AuditFileConfig {
    pub fn load(repo_root: &Path) -> Result<Self, AuditError> {
        let config_path = repo_root.join(CONFIG_FILE_NAME);
        if !config_path.exists() {
            return Ok(Self::default());
        }

        let content = fs::read_to_string(config_path)?;
        let mut config: Self = toml::from_str(&content)?;
        config.exclude_paths = config
            .exclude_paths
            .into_iter()
            .map(|path| normalize_config_path(&path))
            .filter(|path| !path.is_empty())
            .collect();
        Ok(config)
    }
}

pub fn is_repo_relative_path_excluded(relative_path: &Path, exclude_paths: &[String]) -> bool {
    let normalized = normalize_repo_relative_path(relative_path);
    if normalized.is_empty() {
        return false;
    }

    exclude_paths
        .iter()
        .any(|excluded| normalized == *excluded || normalized.starts_with(&format!("{excluded}/")))
}

pub fn normalize_repo_relative_path(path: &Path) -> String {
    normalize_config_path(&path.to_string_lossy())
}

pub fn format_output_path(path: &Path) -> String {
    normalize_output_text(path.to_string_lossy())
}

pub fn normalize_output_text(text: impl AsRef<str>) -> String {
    let text = text.as_ref().replace('\\', "/");
    text.replace("//?/UNC/", "//").replace("//?/", "")
}

fn normalize_config_path(path: &str) -> String {
    path.replace('\\', "/")
        .trim()
        .trim_start_matches("./")
        .trim_matches('/')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_options_default_to_false() {
        let temp = tempfile::tempdir().unwrap();
        let config = AuditFileConfig::load(temp.path()).unwrap();
        assert!(!config.include_historical && !config.check_external);

        std::fs::write(
            temp.path().join(CONFIG_FILE_NAME),
            "exclude_paths = [\"a\"]\n",
        )
        .unwrap();
        let config = AuditFileConfig::load(temp.path()).unwrap();
        assert!(!config.include_historical && !config.check_external);
    }

    #[test]
    fn link_options_are_read_from_config() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(
            temp.path().join(CONFIG_FILE_NAME),
            "include_historical = true\ncheck_external = true\n",
        )
        .unwrap();
        let config = AuditFileConfig::load(temp.path()).unwrap();
        assert!(config.include_historical && config.check_external);
    }
}
