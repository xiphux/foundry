#[test]
fn test_resolve_template_variables() {
    let vars = foundry::config::TemplateVars {
        source: "/Users/me/code/myapp".into(),
        worktree: "/Users/me/.foundry/worktrees/myapp/feat".into(),
        branch: "xiphux/feat".into(),
        name: "feat".into(),
        project: "myapp".into(),
        agent_command: "claude".into(),
    };
    let result = foundry::config::resolve_template("cp {source}/.env {worktree}/.env", &vars).unwrap();
    assert_eq!(result, "cp /Users/me/code/myapp/.env /Users/me/.foundry/worktrees/myapp/feat/.env");
}

#[test]
fn test_resolve_unknown_variable_errors() {
    let vars = foundry::config::TemplateVars {
        source: "".into(),
        worktree: "".into(),
        branch: "".into(),
        name: "".into(),
        project: "".into(),
        agent_command: "".into(),
    };
    let result = foundry::config::resolve_template("echo {unknown}", &vars);
    assert!(result.is_err());
}

#[test]
fn test_resolve_no_variables() {
    let vars = foundry::config::TemplateVars {
        source: "".into(),
        worktree: "".into(),
        branch: "".into(),
        name: "".into(),
        project: "".into(),
        agent_command: "".into(),
    };
    let result = foundry::config::resolve_template("echo hello", &vars).unwrap();
    assert_eq!(result, "echo hello");
}
