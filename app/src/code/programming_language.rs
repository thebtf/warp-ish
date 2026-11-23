//! Language labels and shell-language behavior shared by code snippets.

use std::fmt::Display;

use serde::{Deserialize, Serialize};

use crate::terminal::shell::ShellType;

#[allow(unused)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProgrammingLanguage {
    Shell(ShellType),
    Other(String),
}

impl ProgrammingLanguage {
    pub fn display_name(&self) -> String {
        match self {
            Self::Shell(shell_type) => shell_type.name().to_owned(),
            Self::Other(language) => language.to_lowercase(),
        }
    }

    /// Returns the file extension for the given programming language.
    // TODO(INT-605): Refactor so we don't have to edit this function and the `languages` crate.
    #[cfg_attr(target_family = "wasm", allow(unused))]
    pub fn to_extension(&self) -> Option<&str> {
        match self {
            // The arms below cover both canonical language names emitted by the agent (e.g.
            // "rust", "kotlin") and common markdown code-fence aliases (e.g. "rs", "kt") to keep
            // syntax highlighting working when the model uses either. The set of recognized
            // languages here is kept in sync with `SUPPORTED_LANGUAGES` in the `languages` crate.
            Self::Other(language) => match language.to_lowercase().as_str() {
                "rust" | "rs" => Some("rs"),
                "go" | "golang" => Some("go"),
                "python" | "py" => Some("py"),
                "javascript" | "js" => Some("js"),
                "typescript" | "ts" => Some("ts"),
                "jsx" => Some("jsx"),
                "tsx" => Some("tsx"),
                "yaml" | "yml" => Some("yaml"),
                "cpp" | "c++" => Some("cpp"),
                "java" => Some("java"),
                "groovy" => Some("java"),
                "shell" => Some("sh"),
                "c#" | "csharp" => Some("cs"),
                "html" => Some("html"),
                "css" => Some("css"),
                "c" => Some("c"),
                "json" => Some("json"),
                "jq" => Some("jq"),
                "hcl" | "terraform" | "tf" => Some("hcl"),
                "lua" => Some("lua"),
                "ruby" | "rb" => Some("rb"),
                "php" => Some("php"),
                "toml" => Some("toml"),
                "swift" => Some("swift"),
                "kotlin" | "kt" => Some("kt"),
                "powershell" => Some("ps1"),
                "elixir" => Some("exs"),
                "scala" => Some("scala"),
                "sql" => Some("sql"),
                "objective-c" | "objc" => Some("m"),
                "starlark" => Some("bzl"),
                "xml" => Some("xml"),
                "vue" => Some("vue"),
                "dockerfile" | "docker" | "containerfile" => Some("dockerfile"),
                "markdown" | "md" => Some("md"),
                _ => None,
            },
            Self::Shell(ShellType::PowerShell) => Some("ps1"),
            _ => None,
        }
    }

    /// Return whether this language is a shell language.
    /// This is used to determine whether to show the "execute in terminal" button.
    pub fn is_shell(&self) -> bool {
        matches!(self, Self::Shell(_))
    }
}

impl Display for ProgrammingLanguage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProgrammingLanguage::Shell(shell_type) => write!(f, "{}", shell_type.name()),
            ProgrammingLanguage::Other(language) => write!(f, "{}", language.to_lowercase()),
        }
    }
}

impl From<String> for ProgrammingLanguage {
    // Returns a programming language for a markdown language specifier
    fn from(value: String) -> Self {
        if let Some(shell_type) = ShellType::from_markdown_language_spec(value.as_str()) {
            ProgrammingLanguage::Shell(shell_type)
        } else {
            ProgrammingLanguage::Other(value)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_programming_language_from_string() {
        // Shell language specifiers should produce Shell variants
        assert_eq!(
            ProgrammingLanguage::from("bash".to_string()),
            ProgrammingLanguage::Shell(ShellType::Bash)
        );
        assert_eq!(
            ProgrammingLanguage::from("shell".to_string()),
            ProgrammingLanguage::Shell(ShellType::Bash)
        );
        assert_eq!(
            ProgrammingLanguage::from("sh".to_string()),
            ProgrammingLanguage::Shell(ShellType::Bash)
        );
        assert_eq!(
            ProgrammingLanguage::from("zsh".to_string()),
            ProgrammingLanguage::Shell(ShellType::Zsh)
        );
        assert_eq!(
            ProgrammingLanguage::from("fish".to_string()),
            ProgrammingLanguage::Shell(ShellType::Fish)
        );
        assert_eq!(
            ProgrammingLanguage::from("powershell".to_string()),
            ProgrammingLanguage::Shell(ShellType::PowerShell)
        );
        assert_eq!(
            ProgrammingLanguage::from("pwsh".to_string()),
            ProgrammingLanguage::Shell(ShellType::PowerShell)
        );

        // Non-shell languages should produce Other variants
        assert_eq!(
            ProgrammingLanguage::from("python".to_string()),
            ProgrammingLanguage::Other("python".to_string())
        );
        assert_eq!(
            ProgrammingLanguage::from("rust".to_string()),
            ProgrammingLanguage::Other("rust".to_string())
        );
        assert_eq!(
            ProgrammingLanguage::from("javascript".to_string()),
            ProgrammingLanguage::Other("javascript".to_string())
        );
    }

    #[test]
    fn test_programming_language_to_extension() {
        // Each entry is (markdown language token, expected extension). The expected extension
        // must resolve back to a recognized language via `languages::language_by_filename` so that
        // syntax highlighting is applied to the AI block.
        let cases: &[(&str, &str)] = &[
            // Canonical names.
            ("rust", "rs"),
            ("go", "go"),
            ("python", "py"),
            ("javascript", "js"),
            ("typescript", "ts"),
            ("yaml", "yaml"),
            ("cpp", "cpp"),
            ("java", "java"),
            ("c#", "cs"),
            ("csharp", "cs"),
            ("html", "html"),
            ("css", "css"),
            ("c", "c"),
            ("json", "json"),
            ("hcl", "hcl"),
            ("lua", "lua"),
            ("ruby", "rb"),
            ("php", "php"),
            ("toml", "toml"),
            ("swift", "swift"),
            ("kotlin", "kt"),
            ("powershell", "ps1"),
            ("elixir", "exs"),
            ("scala", "scala"),
            ("sql", "sql"),
            // Languages newly covered by this fix — previously fell through to None and rendered
            // without syntax highlighting in AI blocks even though the `languages` crate supports them.
            ("jsx", "jsx"),
            ("tsx", "tsx"),
            ("xml", "xml"),
            ("vue", "vue"),
            ("dockerfile", "dockerfile"),
            ("starlark", "bzl"),
            ("objective-c", "m"),
            ("objc", "m"),
            // Common markdown code-fence aliases.
            ("rs", "rs"),
            ("golang", "go"),
            ("py", "py"),
            ("js", "js"),
            ("ts", "ts"),
            ("yml", "yaml"),
            ("c++", "cpp"),
            ("rb", "rb"),
            ("kt", "kt"),
            ("terraform", "hcl"),
            ("tf", "hcl"),
            ("docker", "dockerfile"),
            ("containerfile", "dockerfile"),
            ("markdown", "md"),
            ("md", "md"),
        ];
        for (token, expected_extension) in cases {
            let language = ProgrammingLanguage::from((*token).to_string());
            assert_eq!(
                language.to_extension(),
                Some(*expected_extension),
                "expected to_extension({token:?}) to be Some({expected_extension:?})",
            );
        }

        // PowerShell remains the only Shell variant whose extension is exposed; this preserves
        // existing behavior for the other Shell variants which are intentionally not extended here.
        assert_eq!(
            ProgrammingLanguage::Shell(ShellType::PowerShell).to_extension(),
            Some("ps1"),
        );

        // Unrecognized tokens still return None.
        assert_eq!(
            ProgrammingLanguage::Other("definitely-not-a-language".to_string()).to_extension(),
            None,
        );
    }

    #[test]
    fn programming_language_preserves_existing_serialized_tags() {
        for (language, expected) in [
            (
                ProgrammingLanguage::Shell(ShellType::PowerShell),
                serde_json::json!({ "Shell": "PowerShell" }),
            ),
            (
                ProgrammingLanguage::Other("Rust".to_owned()),
                serde_json::json!({ "Other": "Rust" }),
            ),
        ] {
            assert_eq!(serde_json::to_value(&language).unwrap(), expected);
            assert_eq!(
                serde_json::from_value::<ProgrammingLanguage>(expected).unwrap(),
                language
            );
        }
    }
}
