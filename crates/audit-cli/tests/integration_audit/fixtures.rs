use std::{
    fs,
    path::Path,
};

pub(super) fn write_sample_repo(repo_root: &Path) {
    fs::create_dir_all(repo_root.join("src")).expect("create src dir");
    fs::write(
        repo_root.join("Cargo.toml"),
        r#"[package]
name = "sample-quality-repo"
version = "0.1.0"
edition = "2021"
"#,
    )
    .expect("write Cargo.toml");

    fs::write(
        repo_root.join("src/lib.rs"),
        r#"pub fn complicated(a: bool, b: bool, c: bool, d: bool) -> usize {
    let never_used = 7;

    if a && b {
        if c || d {
            return 1;
        }
    }

    if a {
        return 2;
    }

    if b {
        return 3;
    }

    if c {
        return 4;
    }

    if d {
        return 5;
    }

    0
}

pub fn line_padding() {
    let _ = 1;
}

pub fn more_padding() {
    let _ = 2;
}

pub fn even_more_padding() {
    let _ = 3;
}

#[cfg(test)]
mod tests {
    use super::complicated;

    #[test]
    fn complicated_returns_expected_branch() {
        assert_eq!(complicated(false, false, true, false), 4);
    }
}
"#,
    )
    .expect("write lib.rs");

    fs::write(
        repo_root.join("src/extra.rs"),
        "pub fn helper() -> usize { 1 }\n",
    )
    .expect("write extra.rs");
}

pub(super) fn write_workspace_repo(repo_root: &Path) {
    fs::create_dir_all(repo_root.join("src")).expect("create root src dir");
    fs::create_dir_all(repo_root.join("crates/nested-member/src"))
        .expect("create nested member src dir");
    fs::create_dir_all(repo_root.join("scripts")).expect("create scripts dir");

    fs::write(
        repo_root.join("Cargo.toml"),
        r#"[package]
name = "workspace-root"
version = "0.1.0"
edition = "2021"

[workspace]
members = ["crates/nested-member"]
resolver = "2"
"#,
    )
    .expect("write workspace Cargo.toml");

    fs::write(
        repo_root.join("src/lib.rs"),
        r#"pub fn root_complicated(a: bool, b: bool, c: bool, d: bool) -> usize {
    if a && b {
        if c || d {
            return 1;
        }
    }

    if a {
        return 2;
    }

    if b {
        return 3;
    }

    if c {
        return 4;
    }

    if d {
        return 5;
    }

    0
}

pub fn padding_1() { let _ = 1; }
pub fn padding_2() { let _ = 2; }
pub fn padding_3() { let _ = 3; }

#[cfg(test)]
mod tests {
    use super::root_complicated;

    #[test]
    fn root_branch() {
        assert_eq!(root_complicated(false, false, true, false), 4);
    }
}
"#,
    )
    .expect("write root lib.rs");

    fs::write(
        repo_root.join("crates/nested-member/Cargo.toml"),
        r#"[package]
name = "nested-member"
version = "0.1.0"
edition = "2021"
"#,
    )
    .expect("write nested Cargo.toml");

    fs::write(
        repo_root.join("crates/nested-member/src/lib.rs"),
        r#"pub fn nested_complicated(a: bool, b: bool, c: bool, d: bool) -> usize {
    if a && b {
        if c || d {
            return 1;
        }
    }

    if a {
        return 2;
    }

    if b {
        return 3;
    }

    if c {
        return 4;
    }

    if d {
        return 5;
    }

    0
}

pub fn nested_padding_1() { let _ = 1; }
pub fn nested_padding_2() { let _ = 2; }
pub fn nested_padding_3() { let _ = 3; }

#[cfg(test)]
mod tests {
    use super::nested_complicated;

    #[test]
    fn nested_branch() {
        assert_eq!(nested_complicated(false, false, true, false), 4);
    }
}
"#,
    )
    .expect("write nested lib.rs");

    let helper_script = (0..24)
        .map(|index| format!("print({index})"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(
        repo_root.join("scripts/helper.py"),
        format!("{helper_script}\n"),
    )
    .expect("write helper.py");
}

pub(super) fn assert_unix_formatted_output_value(value: &serde_json::Value) {
    let text = value.as_str().expect("string output value");
    assert_unix_formatted_output_text(text);
}

pub(super) fn assert_unix_formatted_output_text(text: &str) {
    assert!(
        !text.contains('\\'),
        "expected Unix-style path separators: {text}"
    );
    assert!(
        !text.contains("//?/"),
        "expected no Windows extended path prefix: {text}"
    );
}
