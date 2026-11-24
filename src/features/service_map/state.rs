//! The saved service maps, and the AI job that generates one from a folder.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::Result;
use chrono::Utc;

use super::graph::{ServiceMap, map_id};
use super::{ai, scan, store};
use crate::features::providers::{Selection, runner};

/// A map being generated in the background.
pub struct Job {
    pub started: Instant,
    pub provider: String,
    pub folder: PathBuf,
    pub stage: String,
    pub cancel: Arc<AtomicBool>,
}

pub enum Event {
    Stage(String),
    Done(Box<Result<ServiceMap>>),
}

#[derive(Default)]
pub struct ServiceMaps {
    pub maps: Vec<ServiceMap>,
    /// Index in `maps` of the map shown when no PR is selected.
    pub selected: usize,
    pub job: Option<Job>,
    pub error: Option<String>,
}

impl ServiceMaps {
    pub fn load() -> Self {
        Self { maps: store::load_all(), ..Default::default() }
    }

    /// The first map with a service whose code lives in `repo`.
    pub fn for_repo(&self, repo: &str) -> Option<&ServiceMap> {
        self.maps.iter().find(|m| m.has_repo(repo))
    }

    pub fn selected(&self) -> Option<&ServiceMap> {
        self.maps.get(self.selected).or(self.maps.first())
    }

    /// Generates the map of `folder` in the background with the picked provider, or cancels
    /// the job already running (`folder` is then ignored).
    pub fn start_or_cancel(
        &mut self,
        folder: Option<PathBuf>,
        selection: Selection,
        timeout: Duration,
        emit: impl Fn(Event) + Send + 'static,
    ) {
        if let Some(job) = &self.job {
            job.cancel.store(true, Ordering::Relaxed);
            return;
        }
        let Some(folder) = folder else { return };
        let Selection { provider, model, label } = selection;
        let cancel = Arc::new(AtomicBool::new(false));
        self.error = None;
        self.job = Some(Job {
            started: Instant::now(),
            provider: label.clone(),
            folder: folder.clone(),
            stage: "Reading the folder…".into(),
            cancel: cancel.clone(),
        });
        std::thread::spawn(move || {
            let result = (|| -> Result<ServiceMap> {
                let scan = scan::scan(&folder)?;
                let folder_name = folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                emit(Event::Stage(format!("{label} is mapping {} repositories…", scan.repos.len())));
                let prompt = ai::build_prompt(&folder_name, &scan);
                let raw = runner::run(&provider, model.as_deref(), &prompt, timeout, cancel)?;
                let mut map = ai::parse(&raw)?;
                if map.name.is_empty() {
                    map.name = folder_name;
                }
                map.id = map_id(&folder);
                map.folder = folder;
                map.provider = label;
                map.created_at = Utc::now().timestamp();
                store::save(&map)?;
                Ok(map)
            })();
            emit(Event::Done(Box::new(result)));
        });
    }

    pub fn on_event(&mut self, event: Event) {
        match event {
            Event::Stage(stage) => {
                if let Some(job) = &mut self.job {
                    job.stage = stage;
                }
            }
            Event::Done(result) => {
                self.job = None;
                match *result {
                    Ok(map) => {
                        let id = map.id.clone();
                        self.maps.retain(|m| m.id != id);
                        self.maps.push(map);
                        self.maps.sort_by_key(|m| m.name.to_ascii_lowercase());
                        self.selected = self.maps.iter().position(|m| m.id == id).unwrap_or(0);
                    }
                    Err(e) => self.error = Some(format!("Could not generate the service map: {e:#}")),
                }
            }
        }
    }

    /// Deletes the map `id` from disk and from the list.
    pub fn remove(&mut self, id: &str) {
        if let Err(e) = store::delete(id) {
            self.error = Some(format!("Could not delete the map: {e:#}"));
        }
        self.maps.retain(|m| m.id != id);
        self.selected = self.selected.min(self.maps.len().saturating_sub(1));
    }
}
