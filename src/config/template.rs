use anyhow::Result;

/// Values available for template variable substitution.
#[derive(Debug, Clone)]
pub struct TemplateVars {
    pub source: String,
    pub worktree: String,
    pub branch: String,
    pub name: String,
    pub project: String,
}

/// The set of known template variable names.
const KNOWN_VARS: &[&str] = &["source", "worktree", "branch", "name", "project"];

/// One piece of a template: text to copy as-is, or a variable name.
enum Segment<'a> {
    Literal(&'a str),
    Var(&'a str),
}

/// Split a template into literal text and `{variable}` references.
///
/// Only `{identifier}` — letters, digits and `_` between the braces — is a
/// variable, and only when the `{` is not preceded by `$`. Commands are shell,
/// and shell is full of braces: `awk '{print $1}'`, `docker ps --format
/// '{{.Names}}'`, `${HOME}`, `{a,b}`, `{1..3}`. Treating every `{` as a
/// variable rejected all of those at config load. An identifier that is not a
/// known variable is still an error, so a typo like `{wroktree}` is caught.
fn segments(template: &str) -> Vec<Segment<'_>> {
    let mut out = Vec::new();
    let mut literal_start = 0;
    let mut i = 0;
    let bytes = template.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'{' && (i == 0 || bytes[i - 1] != b'$') {
            let rest = &template[i + 1..];
            if let Some(len) = rest.find('}') {
                let name = &rest[..len];
                if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                    out.push(Segment::Literal(&template[literal_start..i]));
                    out.push(Segment::Var(name));
                    i += len + 2;
                    literal_start = i;
                    continue;
                }
            }
        }
        i += 1;
    }
    out.push(Segment::Literal(&template[literal_start..]));
    out
}

/// Validate that a template string only uses known variable names.
/// Called at config parse time. Does NOT resolve values.
pub fn validate_template(template: &str) -> Result<()> {
    for segment in segments(template) {
        if let Segment::Var(name) = segment
            && !KNOWN_VARS.contains(&name)
        {
            anyhow::bail!("unknown template variable: {{{name}}}");
        }
    }
    Ok(())
}

/// Resolve template variables in a string. Returns an error for unknown variables.
pub fn resolve_template(template: &str, vars: &TemplateVars) -> Result<String> {
    let mut result = String::with_capacity(template.len());
    for segment in segments(template) {
        match segment {
            Segment::Literal(text) => result.push_str(text),
            Segment::Var(name) => result.push_str(match name {
                "source" => &vars.source,
                "worktree" => &vars.worktree,
                "branch" => &vars.branch,
                "name" => &vars.name,
                "project" => &vars.project,
                _ => anyhow::bail!("unknown template variable: {{{name}}}"),
            }),
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_template_empty_string() {
        assert!(validate_template("").is_ok());
    }

    #[test]
    fn validate_template_no_variables() {
        assert!(validate_template("echo hello world").is_ok());
    }

    #[test]
    fn validate_template_valid_variable() {
        assert!(validate_template("cd {worktree} && ls").is_ok());
    }

    #[test]
    fn validate_template_invalid_variable_name() {
        let result = validate_template("cd {nonexistent}");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("unknown template variable")
        );
    }

    #[test]
    fn an_unclosed_brace_is_literal() {
        assert!(validate_template("cd {bogus").is_ok());
        assert_eq!(
            resolve_template("cd {worktree", &vars()).unwrap(),
            "cd {worktree"
        );
    }

    #[test]
    fn shell_braces_are_left_alone() {
        for cmd in [
            "awk '{print $1}' file",
            "docker ps --format '{{.Names}}'",
            "echo ${HOME} $${name}",
            "cp x.{a,b} {1..3} {}",
        ] {
            assert!(validate_template(cmd).is_ok(), "{cmd}");
            assert_eq!(resolve_template(cmd, &vars()).unwrap(), cmd);
        }
    }

    #[test]
    fn variables_resolve_next_to_shell_braces() {
        assert_eq!(
            resolve_template("awk '{print $1}' {worktree}/x{name}", &vars()).unwrap(),
            "awk '{print $1}' /wt/xfeat"
        );
    }

    #[test]
    fn a_misspelled_variable_is_still_an_error() {
        assert!(validate_template("cd {wroktree}").is_err());
    }

    fn vars() -> TemplateVars {
        TemplateVars {
            source: "/src".into(),
            worktree: "/wt".into(),
            branch: "dev/feat".into(),
            name: "feat".into(),
            project: "app".into(),
        }
    }
}
