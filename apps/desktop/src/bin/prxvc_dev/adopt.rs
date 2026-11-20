//! `prxvc-dev adopt-models`: put the models of the default call plan into the model store from files already on this machine.
//!
//! The store's own adopt path does the work: this program only stages APFS clones of the source
//! files under their catalogue names (instant, no extra disk), then calls
//! `ModelStore::migrate_legacy`, which checks every file's size and SHA-256 against the catalogue
//! pins, moves it into the store and writes the receipt. The source files are never moved or
//! changed. Running it again finds everything installed and does nothing.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use prxvc_media::Direction;
use prxvc_models::{Catalogue, ModelEntry, ModelId, ModelStore};

use crate::call_kit::config::{load_current, paths_for};
use crate::call_kit::providers::ProviderHost;

/// Where the files of this machine's benchmark and server directories are.
const DEFAULT_SOURCES: [&str; 3] = [
    "/Users/nn/worker/voice/bench/asr/models/hf",
    "/Users/nn/worker/voice/bench/tts/models",
    "/Users/nn/worker/voice/code/prx-voice/models/llm",
];

/// The models of the default call plan, whatever the resolution says is missing besides.
const DEFAULT_MODELS: [&str; 5] = [
    "silero-vad-6.2",
    "qwen3-asr-0.6b-q8",
    "qwen3-asr-1.7b-q8",
    "qwen3-tts-1.7b-base-q8",
    "gemma-4-e4b-it-qat-q4-0",
];

const CLONE_TIMEOUT: Duration = Duration::from_secs(120);

struct Options {
    sources: Vec<PathBuf>,
    dry_run: bool,
}

fn parse(args: &[String]) -> Result<Options, String> {
    let mut sources: Vec<PathBuf> = Vec::new();
    let mut dry_run = false;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--source" => sources.push(PathBuf::from(
                it.next().ok_or("--source needs a directory")?,
            )),
            "--dry-run" => dry_run = true,
            other => return Err(format!("unknown argument {other}")),
        }
    }
    if let Some(env) = std::env::var_os("PRXVC_DEV_SOURCES") {
        sources.extend(std::env::split_paths(&env));
    }
    sources.extend(DEFAULT_SOURCES.iter().map(PathBuf::from));
    Ok(Options { sources, dry_run })
}

/// A file called `name` of exactly `size` bytes in one of the directories or directly below them.
fn find_source(dirs: &[PathBuf], name: &str, size: u64) -> Option<PathBuf> {
    let fits = |p: &Path| fs::metadata(p).is_ok_and(|m| m.is_file() && m.len() == size);
    for dir in dirs {
        let direct = dir.join(name);
        if fits(&direct) {
            return Some(direct);
        }
        let Ok(children) = fs::read_dir(dir) else {
            continue;
        };
        for child in children.flatten() {
            let nested = child.path().join(name);
            if fits(&nested) {
                return Some(nested);
            }
        }
    }
    None
}

/// An APFS clone (`cp -c`), else a hard link, else a copy. The subprocess is bounded.
fn clone_file(src: &Path, dst: &Path) -> Result<&'static str, String> {
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let _ = fs::remove_file(dst);
    let child = Command::new("/bin/cp")
        .arg("-c")
        .arg(src)
        .arg(dst)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    if let Ok(mut child) = child {
        let started = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) if status.success() => return Ok("cloned"),
                Ok(Some(_)) | Err(_) => break,
                Ok(None) if started.elapsed() > CLONE_TIMEOUT => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            }
        }
    }
    let _ = fs::remove_file(dst);
    if fs::hard_link(src, dst).is_ok() {
        return Ok("hard-linked");
    }
    fs::copy(src, dst)
        .map(|_| "copied")
        .map_err(|e| e.to_string())
}

/// The ids the default plan needs: the defaults and what the resolution reports as not installed.
fn needed_ids(catalogue: &Catalogue) -> Vec<ModelId> {
    let mut ids: Vec<ModelId> = DEFAULT_MODELS.iter().map(|s| ModelId::new(*s)).collect();
    let config = load_current();
    if let Ok(registry) = ProviderHost::registry_for(&config, None)
        && let Err(problems) =
            registry.resolve_directions(&[Direction::Outgoing, Direction::Incoming])
    {
        for p in problems {
            if let prxvc_providers::ProblemKind::ModelNotInstalled { model, .. } = p.kind
                && !ids.contains(&model)
            {
                ids.push(model);
            }
        }
    }
    ids.retain(|id| catalogue.get(id).is_some());
    ids
}

/// Adopt what can be adopted; `true` when every needed model ends up installed.
pub(crate) fn run(args: &[String]) -> Result<bool, String> {
    let opts = parse(args)?;
    let config = load_current();
    let paths = paths_for(&config)?;
    let catalogue = Catalogue::embedded();
    let store = ModelStore::open(paths.models_dir.clone()).map_err(|e| e.to_string())?;
    if store.is_read_only() {
        return Err(format!(
            "the model store at {} is in use by another process (quit PRX Voice first)",
            paths.models_dir.display()
        ));
    }
    println!("model store: {}", paths.models_dir.display());
    let staging = paths.data_dir.join("adopt-staging").join("files");
    let mut pending: Vec<&ModelEntry> = Vec::new();
    let mut missing = false;
    for id in needed_ids(&catalogue) {
        let Some(entry) = catalogue.get(&id) else {
            continue;
        };
        if store.ready(entry).is_some() {
            println!("already installed: {id}");
            continue;
        }
        let mut found = Vec::new();
        for spec in &entry.files {
            match find_source(&opts.sources, &spec.name, spec.size) {
                Some(path) => found.push((spec, path)),
                None => {
                    println!(
                        "MISSING: {id}: no file {} of {} bytes in {:?}",
                        spec.name, spec.size, opts.sources
                    );
                    missing = true;
                }
            }
        }
        if found.len() != entry.files.len() {
            continue;
        }
        for (spec, path) in &found {
            if opts.dry_run {
                println!("would adopt {id}: {} from {}", spec.name, path.display());
                continue;
            }
            let how = clone_file(path, &staging.join(&spec.name))?;
            println!("staged {id}: {} ({how}) from {}", spec.name, path.display());
        }
        pending.push(entry);
    }
    if opts.dry_run {
        return Ok(!missing);
    }
    if !pending.is_empty() {
        // One line per file, not one per block hashed.
        let announced = std::cell::RefCell::new(String::new());
        let report = store.migrate_legacy(&staging, &catalogue, &|p| {
            if *announced.borrow() != p.file {
                eprintln!(
                    "verifying {} ({}/{} files)",
                    p.file,
                    p.files_done + 1,
                    p.files_total
                );
                *announced.borrow_mut() = p.file;
            }
        });
        for (name, id) in &report.moved {
            println!("adopted {name} into {id}");
        }
        for id in &report.installed {
            println!("installed: {id}");
        }
        for p in &report.unrecognised {
            println!(
                "NOT ADOPTED (size or SHA-256 differs from the catalogue pin): {}",
                p.display()
            );
            missing = true;
        }
        for e in &report.errors {
            println!("ERROR: {e}");
            missing = true;
        }
    }
    let _ = fs::remove_dir_all(paths.data_dir.join("adopt-staging"));
    let mut all = true;
    for entry in &pending {
        let ready = store.ready(entry).is_some();
        println!(
            "{}: {}",
            entry.id,
            if ready { "installed" } else { "NOT installed" }
        );
        all &= ready;
    }
    Ok(all && !missing)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_source_is_found_by_name_and_exact_size_directly_or_one_level_down() {
        let dir = tempfile::tempdir().expect("temporary directory");
        fs::create_dir_all(dir.path().join("repo")).unwrap();
        fs::write(dir.path().join("repo/model.gguf"), vec![0u8; 10]).unwrap();
        fs::write(dir.path().join("flat.gguf"), vec![0u8; 7]).unwrap();
        let dirs = vec![dir.path().to_path_buf()];
        assert!(find_source(&dirs, "model.gguf", 10).is_some());
        assert!(find_source(&dirs, "flat.gguf", 7).is_some());
        assert!(
            find_source(&dirs, "model.gguf", 11).is_none(),
            "a different size is not the file"
        );
        assert!(find_source(&dirs, "ghost.gguf", 1).is_none());
    }

    #[test]
    fn a_clone_leaves_the_source_alone_and_has_its_content() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let src = dir.path().join("src.bin");
        fs::write(&src, b"content").unwrap();
        let dst = dir.path().join("stage/dst.bin");
        let how = clone_file(&src, &dst).expect("staged");
        assert!(["cloned", "hard-linked", "copied"].contains(&how));
        assert_eq!(fs::read(&dst).unwrap(), b"content");
        assert_eq!(fs::read(&src).unwrap(), b"content");
        // Staging again over an existing file works.
        clone_file(&src, &dst).expect("staged again");
    }

    #[test]
    fn the_arguments_add_sources_before_the_defaults() {
        let opts = parse(&["--source".into(), "/x".into(), "--dry-run".into()]).unwrap();
        assert!(opts.dry_run);
        assert_eq!(opts.sources.first(), Some(&PathBuf::from("/x")));
        assert!(opts.sources.contains(&PathBuf::from(DEFAULT_SOURCES[0])));
        assert!(parse(&["--nope".into()]).is_err());
        assert!(parse(&["--source".into()]).is_err());
    }
}
