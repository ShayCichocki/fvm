use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

pub fn resolve_init_wrapper(
    choice: &str,
    dry_run: bool,
    require_guest_rss: bool,
    legacy_snapshot: bool,
) -> Result<Option<PathBuf>> {
    if choice == "none" {
        if legacy_snapshot {
            bail!("legacy-snapshot mode requires fvm-init; do not use `--init-wrapper none`");
        }
        if require_guest_rss && !dry_run {
            bail!(
                "guest RSS measurement requires fvm-init; pass --allow-missing-guest-rss to build without it"
            );
        }
        return Ok(None);
    }

    if choice != "auto" {
        let path = PathBuf::from(choice);
        validate_init_path(&path)?;
        return Ok(Some(path));
    }

    let Some(path) = companion_init_path()? else {
        if legacy_snapshot {
            bail!(
                "legacy-snapshot mode requires fvm-init next to the fvm executable or --init-wrapper /path/to/fvm-init"
            );
        }
        if require_guest_rss && !dry_run {
            bail!(
                "could not find companion fvm-init binary next to fvm; build/install fvm-init or pass --init-wrapper /path/to/fvm-init"
            );
        }
        return Ok(None);
    };

    Ok(Some(path))
}

pub fn companion_init_path() -> Result<Option<PathBuf>> {
    let current = std::env::current_exe().context("failed to locate current fvm executable")?;
    let Some(dir) = current.parent() else {
        return Ok(None);
    };
    let candidate = dir.join(format!("fvm-init{}", std::env::consts::EXE_SUFFIX));
    if candidate.is_file() {
        return Ok(Some(candidate));
    }
    Ok(None)
}

fn validate_init_path(path: &Path) -> Result<()> {
    if !path.is_file() {
        bail!(
            "init wrapper {} does not exist or is not a file",
            path.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn none_is_allowed_when_guest_rss_not_required() {
        let resolved = resolve_init_wrapper("none", false, false, false).unwrap();
        assert!(resolved.is_none());
    }

    #[test]
    fn none_is_rejected_for_legacy_mode() {
        assert!(resolve_init_wrapper("none", false, false, true).is_err());
    }
}
