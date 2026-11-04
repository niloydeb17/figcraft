//! `cargo xtask clean-caches [--dry-run] [--hours N]`: give back the disk that build output eats.
//!
//! Parallel agents build with their own `CARGO_TARGET_DIR` (see CLAUDE.md), and a debug build of
//! this workspace is 9-27 GB, so forgotten ones fill the disk fast. This task removes:
//!
//! - cargo target directories left in the temp directories by agent runs: a directory named
//!   `fc-*target*` or `figcraft-*target*` that holds a `CACHEDIR.TAG` and has not been written to
//!   for `--hours` hours (default 6, so a build still in progress is never touched);
//! - this checkout's `target/debug/incremental`, which cargo regenerates (a few GB).
//!
//! `--dry-run` only lists what would go. Nothing else is touched.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const DEFAULT_HOURS: u64 = 6;
const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
const MIB: f64 = 1024.0 * 1024.0;

pub fn clean(root: &Path, args: &[String]) -> bool {
    let mut dry_run = false;
    let mut hours = DEFAULT_HOURS;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--dry-run" => dry_run = true,
            "--hours" => match it.next().and_then(|v| v.parse::<u64>().ok()) {
                Some(h) => hours = h,
                None => {
                    eprintln!("--hours needs a whole number");
                    return false;
                }
            },
            other => {
                eprintln!("unknown option {other}; usage: clean-caches [--dry-run] [--hours N]");
                return false;
            }
        }
    }
    let min_age = Duration::from_secs(hours.saturating_mul(3600));
    let in_use = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from);

    let mut victims: Vec<PathBuf> = Vec::new();
    let mut scanned: Vec<PathBuf> = vec![PathBuf::from("/tmp"), std::env::temp_dir()];
    scanned.sort();
    scanned.dedup();
    for base in scanned {
        let Ok(entries) = fs::read_dir(&base) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if in_use.as_deref() == Some(path.as_path()) {
                continue;
            }
            if is_idle_agent_target(&path, min_age) && !victims.contains(&path) {
                victims.push(path);
            }
        }
    }
    let incremental = target_dir(root).join("debug").join("incremental");
    if incremental.is_dir() {
        victims.push(incremental);
    }

    if victims.is_empty() {
        eprintln!("nothing to clean");
        return true;
    }
    let mut total = 0u64;
    let mut ok = true;
    for path in &victims {
        let size = dir_size(path);
        total = total.saturating_add(size);
        if dry_run {
            eprintln!("would remove {} ({})", path.display(), human(size));
        } else if let Err(e) = fs::remove_dir_all(path) {
            eprintln!("could not remove {}: {e}", path.display());
            ok = false;
        } else {
            eprintln!("removed {} ({})", path.display(), human(size));
        }
    }
    let verb = if dry_run { "would free" } else { "freed" };
    eprintln!("{verb} about {}", human(total));
    ok
}

/// Where cargo writes this checkout's build output.
fn target_dir(root: &Path) -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), PathBuf::from)
}

/// The names agent runs give their private target directories.
fn is_agent_target_name(name: &str) -> bool {
    (name.starts_with("fc-") || name.starts_with("figcraft-")) && name.contains("target")
}

/// A real directory (not a link) with an agent-style name that is a cargo target directory and has
/// not been written to for `min_age`.
fn is_idle_agent_target(path: &Path, min_age: Duration) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if !is_agent_target_name(name) {
        return false;
    }
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => {}
        _ => return false,
    }
    if !path.join("CACHEDIR.TAG").is_file() {
        return false;
    }
    // Cargo adds entries to these while it builds, which moves their modification times.
    let busiest = [
        path.to_path_buf(),
        path.join("debug"),
        path.join("debug").join("deps"),
        path.join("release"),
        path.join("release").join("deps"),
        path.join("wasm32-unknown-unknown"),
    ]
    .iter()
    .filter_map(|p| fs::metadata(p).and_then(|m| m.modified()).ok())
    .max();
    match busiest.and_then(|t| SystemTime::now().duration_since(t).ok()) {
        Some(age) => age >= min_age,
        // Unknown or in the future: assume it is in use.
        None => false,
    }
}

/// Bytes under `path`, not following links; unreadable entries count as zero.
fn dir_size(path: &Path) -> u64 {
    let Ok(meta) = fs::symlink_metadata(path) else { return 0 };
    if !meta.is_dir() {
        return meta.len();
    }
    let Ok(entries) = fs::read_dir(path) else { return 0 };
    entries.flatten().fold(0u64, |sum, e| sum.saturating_add(dir_size(&e.path())))
}

fn human(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= GIB { format!("{:.1} GiB", b / GIB) } else { format!("{:.0} MiB", b / MIB) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_agent_target_names_match() {
        assert!(is_agent_target_name("fc-target-a"));
        assert!(is_agent_target_name("figcraft-autolayout-target"));
        assert!(!is_agent_target_name("fc-cfg-a"));
        assert!(!is_agent_target_name("figcraft-home"));
        assert!(!is_agent_target_name("target"));
        assert!(!is_agent_target_name("other-target"));
    }

    #[test]
    fn idle_check_needs_cachedir_tag_and_age() {
        let base = std::env::temp_dir().join(format!("fc-clean-test-target-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("debug")).unwrap();
        // No CACHEDIR.TAG: not a cargo target directory.
        assert!(!is_idle_agent_target(&base, Duration::ZERO));
        fs::write(base.join("CACHEDIR.TAG"), "Signature: 8a477f597d28d172789f06886806bc55").unwrap();
        // Just written: idle only when no age is required.
        assert!(is_idle_agent_target(&base, Duration::ZERO));
        assert!(!is_idle_agent_target(&base, Duration::from_secs(3600)));
        assert_eq!(dir_size(&base), 43);
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn sizes_read_well() {
        assert_eq!(human(0), "0 MiB");
        assert_eq!(human(5 * 1024 * 1024 * 1024), "5.0 GiB");
        assert_eq!(human(300 * 1024 * 1024), "300 MiB");
    }
}
