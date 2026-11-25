//! Acceptance tests of the core home resolver (#637, #634): `HomeDir`, `expand_home`,
//! `ServersDir` and `SkillsDir`.
//!
//! The crate forbids `unsafe`, so the process environment cannot be changed in place. Each case
//! re-runs its own `#[ignore]`d `child` test in a fresh process of this test binary with `HOME`
//! set, and the parent test asserts that the child passed.
//!
//! Unix only: the home source there is `HOME`. On Unix the only input that makes the home
//! unusable is a relative `HOME`.

#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "test, bench and example code may panic on failure"
)]

use std::assert_matches;
use std::path::{Path, PathBuf};
use std::process::Command;

use mcp_execution_core::{HomeDir, HomeDirError, ServersDir, SkillsDir, expand_home};

const ABS_HOME: &str = "/acceptance/home";
const RELATIVE_HOME: &str = "relative/home";

/// Runs `<module>::child` in a new process of this test binary with `HOME` set to `home`.
fn run_child(module: &str, home: &str) {
    let output = Command::new(std::env::current_exe().expect("test binary path"))
        .args(["--exact", "--ignored", &format!("{module}::child")])
        .env("HOME", home)
        .output()
        .expect("spawn the child test");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && stdout.contains("1 passed"),
        "child {module} failed:\n{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Defines `<name>::run` (parent) and `<name>::child` (the body, run with `HOME` = `$home`).
macro_rules! case {
    ($name:ident, $home:expr, $body:block) => {
        mod $name {
            use super::*;

            #[test]
            fn run() {
                run_child(stringify!($name), $home);
            }

            #[test]
            #[ignore = "runs only inside its parent test, with HOME set"]
            fn child() $body
        }
    };
}

case!(home_from_env_reads_an_absolute_home, ABS_HOME, {
    let home = HomeDir::from_env().expect("an absolute HOME is accepted");
    assert_eq!(home.as_path(), Path::new(ABS_HOME));
});

case!(home_from_env_rejects_a_relative_home, RELATIVE_HOME, {
    let error = HomeDir::from_env().expect_err("a relative HOME must be rejected");
    assert_matches!(error, HomeDirError::NotAbsolute { .. });
});

case!(expand_home_replaces_a_bare_tilde_with_home, ABS_HOME, {
    assert_eq!(
        expand_home(Path::new("~")).expect("expands"),
        PathBuf::from(ABS_HOME)
    );
});

case!(
    expand_home_joins_the_rest_after_a_leading_tilde,
    ABS_HOME,
    {
        assert_eq!(
            expand_home(Path::new("~/x/y")).expect("expands"),
            Path::new(ABS_HOME).join("x").join("y")
        );
    }
);

case!(expand_home_keeps_every_other_path_unchanged, ABS_HOME, {
    for path in ["/abs/x", "rel/x", "x/~", ".~"] {
        assert_eq!(
            expand_home(Path::new(path)).expect("no home needed"),
            PathBuf::from(path),
            "{path}"
        );
    }
});

case!(
    expand_home_rejects_a_tilde_name_instead_of_a_literal_directory,
    ABS_HOME,
    {
        for path in ["~user", "~user/x", "~~"] {
            let error =
                expand_home(Path::new(path)).expect_err("another user's home is unsupported");
            assert_matches!(error, HomeDirError::UnsupportedUserHome { .. }, "{path}");
        }
    }
);

case!(
    expand_home_never_reads_home_for_a_path_without_tilde,
    RELATIVE_HOME,
    {
        assert_eq!(
            expand_home(Path::new("/abs/x")).expect("an unusable home is irrelevant here"),
            PathBuf::from("/abs/x")
        );
    }
);

case!(
    expand_home_fails_on_a_tilde_path_when_home_is_relative,
    RELATIVE_HOME,
    {
        for path in ["~", "~/x"] {
            let error = expand_home(Path::new(path)).expect_err("home is needed");
            assert_matches!(error, HomeDirError::NotAbsolute { .. }, "{path}");
        }
    }
);

case!(
    servers_dir_defaults_to_dot_claude_servers_under_home,
    ABS_HOME,
    {
        let dir = ServersDir::resolve(None).expect("resolves");
        assert_eq!(
            dir.as_path(),
            Path::new(ABS_HOME).join(".claude").join("servers")
        );
    }
);

case!(
    skills_dir_defaults_to_dot_claude_skills_under_home,
    ABS_HOME,
    {
        let dir = SkillsDir::resolve(None).expect("resolves");
        assert_eq!(
            dir.as_path(),
            Path::new(ABS_HOME).join(".claude").join("skills")
        );
    }
);

case!(
    servers_dir_expands_a_tilde_and_keeps_an_absolute_path,
    ABS_HOME,
    {
        let tilde = ServersDir::resolve(Some("~/x".into())).expect("resolves");
        assert_eq!(tilde.as_path(), Path::new(ABS_HOME).join("x"));
        let bare = ServersDir::resolve(Some("~".into())).expect("resolves");
        assert_eq!(bare.as_path(), Path::new(ABS_HOME));
        let abs = ServersDir::resolve(Some("/srv/servers".into())).expect("resolves");
        assert_eq!(abs.as_path(), Path::new("/srv/servers"));
    }
);

case!(
    skills_dir_expands_a_tilde_and_keeps_an_absolute_path,
    ABS_HOME,
    {
        let tilde = SkillsDir::resolve(Some("~/x".into())).expect("resolves");
        assert_eq!(tilde.as_path(), Path::new(ABS_HOME).join("x"));
        let abs = SkillsDir::resolve(Some("/srv/skills".into())).expect("resolves");
        assert_eq!(abs.as_path(), Path::new("/srv/skills"));
    }
);

case!(
    dirs_resolve_an_absolute_custom_path_without_a_usable_home,
    RELATIVE_HOME,
    {
        assert!(ServersDir::resolve(Some("/srv/servers".into())).is_ok());
        assert!(SkillsDir::resolve(Some("/srv/skills".into())).is_ok());
    }
);

case!(dirs_fail_when_home_is_needed_and_relative, RELATIVE_HOME, {
    let errors = [
        ServersDir::resolve(None).expect_err("default needs home"),
        SkillsDir::resolve(None).expect_err("default needs home"),
        ServersDir::resolve(Some("~/x".into())).expect_err("tilde needs home"),
        SkillsDir::resolve(Some("~/x".into())).expect_err("tilde needs home"),
    ];
    for error in errors {
        assert_matches!(error, HomeDirError::NotAbsolute { .. });
    }
});
