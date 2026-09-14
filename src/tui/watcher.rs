use std::{
    path::{Path, PathBuf},
    sync::mpsc,
};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

#[derive(Debug)]
pub struct MdWatcher {
    /// Fires with `()` on any filesystem event — we don't care what changed, only that it did.
    pub watch_rx: mpsc::Receiver<()>,
    #[allow(dead_code)] // kept alive only so the watch isn't dropped
    watcher: RecommendedWatcher,
}

impl MdWatcher {
    pub fn new(md_filepath: String) -> Self {
        let (watch_tx, watch_rx) = mpsc::channel::<()>();

        // An editor's save replaces the file's inode, so compare canonical paths
        // rather than anything the old file held.
        let target: PathBuf =
            std::fs::canonicalize(&md_filepath).unwrap_or_else(|_| PathBuf::from(&md_filepath));

        // A directory watch reports every entry inside it, so filter down to ours.
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            let event = match res {
                Ok(event) => event,
                Err(e) => {
                    log::warn!("watch error: {e:?}");
                    return;
                }
            };
            if event.kind.is_modify() && event.paths.iter().any(|p| p == &target) {
                log::debug!("file modified: {:?}", event.paths);
                let _ = watch_tx.send(());
            }
        })
        .unwrap();

        // Watch the file's *parent directory*, not the file itself — a directory
        // watch survives the file inside it being replaced by an editor's save.
        let watch_dir = Path::new(&md_filepath)
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        watcher
            .watch(watch_dir, RecursiveMode::NonRecursive)
            .unwrap();

        MdWatcher { watch_rx, watcher }
    }
}
