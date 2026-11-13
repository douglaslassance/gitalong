//! Carries clones over from the names Git Along used when it was Gitalong.

use std::fs;
use std::path::Path;

use crate::error::Result;
use crate::hooks;

/// Ref namespace clones published to before the rename.
pub const LEGACY_NAMESPACE: &str = "refs/gitalong/v1";

/// Clone id and fetch marker, inside the git dir.
const LEGACY_STATE_DIRNAME: &str = "gitalong";

/// Git store clone or JSONBin cache, inside the working tree.
const LEGACY_CACHE_DIRNAME: &str = ".gitalong";

/// Text only the hooks installed before the rename contain.
const LEGACY_HOOK_MARKER: &str = "gitalong update";

/// Move legacy local state into `data_dir` and refresh legacy hooks.
pub fn local_state(
    git_dir: &Path,
    working_dir: &Path,
    data_dir: &Path,
    hooks_dir: &Path,
) -> Result<()> {
    let state = git_dir.join(LEGACY_STATE_DIRNAME);
    if state.is_dir() && !data_dir.exists() {
        fs::rename(&state, data_dir)?;
    }

    let cache = working_dir.join(LEGACY_CACHE_DIRNAME);
    if cache.is_dir() {
        fs::create_dir_all(data_dir)?;
        let (from, to) = if cache.join(".git").is_dir() {
            (cache.clone(), data_dir.join("store"))
        } else {
            (cache.join("commits.json"), data_dir.join("commits.json"))
        };
        // A failed move only loses a cache that the next read rebuilds.
        if from.exists() && !to.exists() {
            let _ = fs::rename(&from, &to);
        }
        if cache.exists() {
            fs::remove_dir_all(&cache)?;
        }
    }

    for hook in hooks::HOOKS {
        let path = hooks_dir.join(hook.name);
        if fs::read_to_string(&path).is_ok_and(|body| body.contains(LEGACY_HOOK_MARKER)) {
            fs::write(&path, hook.body)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    struct Dirs {
        _root: tempfile::TempDir,
        git: std::path::PathBuf,
        work: std::path::PathBuf,
        data: std::path::PathBuf,
        hooks: std::path::PathBuf,
    }

    fn dirs() -> Dirs {
        let root = tempdir().unwrap();
        let work = root.path().to_path_buf();
        let git = work.join(".git");
        let hooks = git.join("hooks");
        fs::create_dir_all(&hooks).unwrap();
        Dirs {
            data: git.join("along"),
            git,
            work,
            hooks,
            _root: root,
        }
    }

    fn run(d: &Dirs) {
        local_state(&d.git, &d.work, &d.data, &d.hooks).unwrap();
    }

    #[test]
    fn state_dir_keeps_the_clone_id() {
        let d = dirs();
        fs::create_dir_all(d.git.join("gitalong")).unwrap();
        fs::write(d.git.join("gitalong/clone-id"), "abc").unwrap();
        run(&d);
        assert_eq!(fs::read_to_string(d.data.join("clone-id")).unwrap(), "abc");
        assert!(!d.git.join("gitalong").exists());
    }

    #[test]
    fn git_store_clone_moves_out_of_the_working_tree() {
        let d = dirs();
        fs::create_dir_all(d.work.join(".gitalong/.git")).unwrap();
        fs::write(d.work.join(".gitalong/commits.json"), "[]").unwrap();
        run(&d);
        assert!(d.data.join("store/.git").is_dir());
        assert!(d.data.join("store/commits.json").is_file());
        assert!(!d.work.join(".gitalong").exists());
    }

    #[test]
    fn jsonbin_cache_moves_out_of_the_working_tree() {
        let d = dirs();
        fs::create_dir_all(d.work.join(".gitalong")).unwrap();
        fs::write(d.work.join(".gitalong/commits.json"), "[]").unwrap();
        fs::write(d.work.join(".gitalong/.pull"), "").unwrap();
        run(&d);
        assert!(d.data.join("commits.json").is_file());
        assert!(!d.work.join(".gitalong").exists());
    }

    #[test]
    fn only_legacy_hooks_are_rewritten() {
        let d = dirs();
        fs::write(d.hooks.join("post-commit"), "#!/bin/sh\ngitalong update\n").unwrap();
        fs::write(d.hooks.join("post-checkout"), "#!/bin/sh\necho mine\n").unwrap();
        run(&d);
        let commit = fs::read_to_string(d.hooks.join("post-commit")).unwrap();
        assert!(commit.contains("git along update"));
        assert!(!commit.contains(LEGACY_HOOK_MARKER));
        let checkout = fs::read_to_string(d.hooks.join("post-checkout")).unwrap();
        assert_eq!(checkout, "#!/bin/sh\necho mine\n");
    }

    #[test]
    fn running_twice_changes_nothing() {
        let d = dirs();
        fs::create_dir_all(d.git.join("gitalong")).unwrap();
        fs::write(d.git.join("gitalong/clone-id"), "abc").unwrap();
        run(&d);
        run(&d);
        assert_eq!(fs::read_to_string(d.data.join("clone-id")).unwrap(), "abc");
    }
}
