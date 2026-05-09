//! Embedded git hook scripts and the install routine for `setup --update-hooks`.
//!
//! Hook contents are baked into the binary via [`include_str!`] so a release
//! build is fully self-contained — no need to ship a resources directory
//! alongside the executable.

use std::fs;
use std::path::Path;

use crate::error::Result;

/// One installable hook: its filename and shell-script body.
pub struct Hook {
    pub name: &'static str,
    pub body: &'static str,
}

/// Hooks installed by `gitalong setup --update-hooks`. The set mirrors the
/// Python 0.x install: post-applypatch, post-checkout, post-commit, post-rewrite.
pub const HOOKS: &[Hook] = &[
    Hook {
        name: "post-applypatch",
        body: include_str!("resources/hooks/post-applypatch"),
    },
    Hook {
        name: "post-checkout",
        body: include_str!("resources/hooks/post-checkout"),
    },
    Hook {
        name: "post-commit",
        body: include_str!("resources/hooks/post-commit"),
    },
    Hook {
        name: "post-rewrite",
        body: include_str!("resources/hooks/post-rewrite"),
    },
];

/// Snippet appended to `.gitignore` by `setup --update-gitignore`.
pub const GITIGNORE_PATCH: &str = include_str!("resources/gitignore_patch");

/// Write every [`HOOKS`] entry into `dir`, overwriting any pre-existing file.
///
/// Hook files are made executable on Unix so git can run them directly. Like
/// the Python implementation, we don't merge with existing hook content — the
/// caller is expected to back up custom hooks before opting in.
pub fn install(dir: &Path) -> Result<()> {
    fs::create_dir_all(dir)?;
    for hook in HOOKS {
        let path = dir.join(hook.name);
        fs::write(&path, hook.body)?;
        make_executable(&path)?;
    }
    Ok(())
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path)?.permissions();
    // 0o755: rwxr-xr-x — owner can write, everyone else can read+execute.
    perms.set_mode(0o755);
    fs::set_permissions(path, perms)?;
    Ok(())
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<()> {
    // On Windows git uses the file extension, not the executable bit.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn install_writes_every_hook() {
        let dir = tempdir().unwrap();
        install(dir.path()).unwrap();
        for hook in HOOKS {
            let p = dir.path().join(hook.name);
            assert!(p.is_file(), "hook {} was not written", hook.name);
            let body = fs::read_to_string(&p).unwrap();
            assert!(body.contains("gitalong update"));
        }
    }

    #[test]
    fn hooks_call_update_not_sync() {
        // The Python hooks called the non-existent `gitalong sync` subcommand.
        // The Rust port must call `gitalong update`.
        for hook in HOOKS {
            assert!(!hook.body.contains("gitalong sync"));
            assert!(hook.body.contains("gitalong update"));
        }
    }

    #[test]
    fn gitignore_patch_has_expected_directives() {
        assert!(GITIGNORE_PATCH.contains("/.gitalong/"));
        assert!(GITIGNORE_PATCH.contains("!/.gitalong.cfg"));
    }

    #[cfg(unix)]
    #[test]
    fn installed_hooks_are_executable() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().unwrap();
        install(dir.path()).unwrap();
        for hook in HOOKS {
            let mode = fs::metadata(dir.path().join(hook.name))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o111, 0o111, "{} not executable", hook.name);
        }
    }
}
