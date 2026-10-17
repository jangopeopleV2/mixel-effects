//! Folder permission requirements for a render's complete, already expanded output plan.
//! A sequence is represented by its containing directory, so its frame count does not allocate
//! a second list. Paths describe requirements; only the live native registry grants access.

use std::path::{Component, Path, PathBuf};

use effectcraft_project::render_queue::{RenderLog, RenderQueueItem};

pub const FOLDER_REQUIRED: &str = "Choose the containing folders for all render output files before rendering. No files were written.";
pub const WRONG_FOLDER: &str = "Choose a required output folder or a folder containing it. No files were written.";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OutputAccess {
    directories: Vec<PathBuf>,
}

impl OutputAccess {
    /// Requirements for one actual filesystem export, also used at the host writer boundary.
    pub fn for_output(item: &RenderQueueItem, path: &str) -> Result<Self, String> {
        Self::for_output_with(item, path, |path| crate::macos_mas::resolve(path))
    }

    fn for_output_with(item: &RenderQueueItem, path: &str, mut resolve: impl FnMut(&Path) -> Result<PathBuf, String>) -> Result<Self, String> {
        let mut access = Self::default();
        let current = resolve(Path::new(path))?;
        let selected = if Path::new(&item.output.output).is_absolute() { resolve(Path::new(&item.output.output))? } else { PathBuf::from(&item.output.output) };
        let derived = item.output_modules().count() > 1 || current != selected;
        access.include(item, &current.to_string_lossy(), derived);
        Ok(access)
    }

    pub(super) fn extend(&mut self, other: &Self) {
        for directory in &other.directories {
            self.directory(directory);
        }
    }

    /// A possible storage-overflow destination is part of the same preflight.
    pub fn include_overflow(&mut self, folder: &str) {
        self.directory(Path::new(folder));
    }
    pub(super) fn include(&mut self, item: &RenderQueueItem, path: &str, derived: bool) {
        if (derived || item.output.format.is_sequence() || item.log != RenderLog::ErrorsOnly)
            && let Some(parent) = Path::new(path).parent()
        {
            self.directory(parent);
        }
    }

    pub(super) fn directory(&mut self, path: &Path) {
        if !self.directories.iter().any(|parent| path.starts_with(parent)) {
            self.directories.retain(|child| !child.starts_with(path));
            self.directories.push(path.to_path_buf());
        }
    }

    /// Return only requirements not covered by an explicitly selected live folder or container.
    pub fn missing(&self) -> Result<Self, String> {
        self.missing_with(|path| crate::macos_mas::resolve(path), |path| crate::macos_mas::directory_granted(path))
    }

    fn missing_with(&self, mut resolve: impl FnMut(&Path) -> Result<PathBuf, String>, mut granted: impl FnMut(&Path) -> bool) -> Result<Self, String> {
        let mut missing = Self::default();
        for directory in &self.directories {
            let current = resolve(directory)?;
            if !valid_directory(&current) || !granted(&current) {
                missing.directory(&current);
            }
        }
        Ok(missing)
    }

    pub fn is_empty(&self) -> bool {
        self.directories.is_empty()
    }

    pub fn require(&self) -> Result<(), String> {
        if self.missing()?.is_empty() { Ok(()) } else { Err(FOLDER_REQUIRED.into()) }
    }

    /// A panel answer must cover a pending target, then pass the real registry again. Separate
    /// volumes can be granted one at a time; require() still checks the entire batch before IO.
    /// Never turn an inferred parent into a grant or trust the spelling of the answer alone.
    pub fn accept_folder(&self, chosen: &Path) -> Result<(), String> {
        let chosen = crate::macos_mas::resolve(chosen)?;
        self.accept_with(&chosen, |path| crate::macos_mas::resolve(path), |path| crate::macos_mas::directory_granted(path))
    }

    fn accept_with(
        &self,
        chosen: &Path,
        mut resolve: impl FnMut(&Path) -> Result<PathBuf, String>,
        mut granted: impl FnMut(&Path) -> bool,
    ) -> Result<(), String> {
        if !valid_directory(chosen) || !granted(chosen) {
            return Err(WRONG_FOLDER.into());
        }
        let mut covered = false;
        for directory in &self.directories {
            let current = resolve(directory)?;
            if !valid_directory(&current) {
                return Err(WRONG_FOLDER.into());
            }
            if current.starts_with(chosen) {
                if !granted(&current) {
                    return Err(WRONG_FOLDER.into());
                }
                covered = true;
            }
        }
        if covered { Ok(()) } else { Err(WRONG_FOLDER.into()) }
    }
}

fn valid_directory(path: &Path) -> bool {
    path.is_absolute() && !path.components().any(|part| matches!(part, Component::ParentDir))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        std::env::current_dir().unwrap().join("effects-output-permissions")
    }

    #[test]
    fn complete_requirements_check_every_destination_before_any_output() {
        let root = root();
        let mut access = OutputAccess::default();
        access.directory(&root.join("first"));
        access.directory(&root.join("other"));
        let mut checked = Vec::new();
        let missing = access
            .missing_with(
                |path| Ok(path.to_path_buf()),
                |path| {
                    checked.push(path.to_path_buf());
                    path.ends_with("first")
                },
            )
            .unwrap();
        assert_eq!(checked.len(), 2);
        assert_eq!(missing.directories, vec![root.join("other")]);
        assert!(missing.accept_with(&root.join("wrong"), |p| Ok(p.into()), |_| true).is_err());
        assert!(missing.accept_with(&root, |p| Ok(p.into()), |_| false).is_err());
        assert!(missing.accept_with(&root, |p| Ok(p.into()), |_| true).is_ok());
        assert!(access.missing_with(|_| Err("stored bookmark denied".into()), |_| true).is_err());
    }

    #[test]
    fn moved_folders_are_validated_at_their_current_location_without_inferred_grants() {
        let root = root();
        let old = root.join("old");
        let current = root.join("moved");
        let mut access = OutputAccess::default();
        access.directory(&old);
        assert!(access.accept_with(&old, |_| Ok(current.clone()), |_| true).is_err());
        assert!(access.accept_with(&current, |_| Ok(current.clone()), |p| p.starts_with(&current)).is_ok());
        assert!(access.accept_with(&current.join(".."), |_| Ok(current.clone()), |_| true).is_err());
    }
    #[test]
    fn separate_destinations_collect_scopes_without_releasing_the_complete_batch() {
        let root = root();
        let first = root.join("volume-one");
        let second = root.join("volume-two");
        let mut access = OutputAccess::default();
        access.directory(&first);
        access.directory(&second);
        assert!(access.accept_with(&first, |p| Ok(p.into()), |p| p.starts_with(&first)).is_ok());
        let remaining = access.missing_with(|p| Ok(p.into()), |p| p.starts_with(&first)).unwrap();
        assert_eq!(remaining.directories, vec![second.clone()]);
        assert!(remaining.accept_with(&first, |p| Ok(p.into()), |_| true).is_err());
        assert!(remaining.accept_with(&second, |p| Ok(p.into()), |p| p.starts_with(&second)).is_ok());
        assert!(access.missing_with(|p| Ok(p.into()), |p| p.starts_with(&first) || p.starts_with(&second)).unwrap().is_empty());
    }

    #[test]
    fn moved_plain_file_remains_file_only_at_the_real_host_boundary() {
        let root = root();
        let old = root.join("old/movie.mov");
        let moved = root.join("new/movie.mov");
        let mut item = RenderQueueItem::new(1, effectcraft_project::ItemId(1));
        item.output.output = old.to_string_lossy().into_owned();
        let resolve = |p: &Path| Ok(if p == old.as_path() { moved.clone() } else { p.to_path_buf() });
        assert!(OutputAccess::for_output_with(&item, &moved.to_string_lossy(), resolve).unwrap().is_empty());
        item.log = RenderLog::PlusSettings;
        assert_eq!(OutputAccess::for_output_with(&item, &moved.to_string_lossy(), resolve).unwrap().directories, vec![root.join("new")]);
        assert!(OutputAccess::for_output_with(&item, &moved.to_string_lossy(), |_| Err("bookmark denied".into())).is_err());
    }

    fn session() -> crate::Session {
        let mut session = crate::Session::default();
        session.execute("comp.new", serde_json::json!({"name": "Permission plan", "width": 64, "height": 36, "duration": 0.125, "frameRate": 24})).unwrap();
        session.exporter = Some(std::sync::Arc::new(crate::rq_tests::MockExporter { log: Default::default() }));
        let comp = session.active_comp_id().unwrap();
        let mut item = RenderQueueItem::new(1, comp);
        item.output.format = effectcraft_project::render_queue::OutputFormat::Uncompressed;
        item.output.output = root().join("movie.mov").to_string_lossy().into_owned();
        std::sync::Arc::make_mut(&mut session.project).render_queue.push(item);
        session
    }

    #[test]
    fn plain_file_stays_file_only_but_requested_success_logs_require_a_folder() {
        let mut session = session();
        assert!(session.plan_render().unwrap().access.is_empty());
        for log in [RenderLog::PlusSettings, RenderLog::PlusPerFrameInfo] {
            std::sync::Arc::make_mut(&mut session.project).render_queue[0].log = log;
            let plan = session.plan_render().unwrap();
            assert_eq!(plan.items.len(), 1);
            assert_eq!(plan.access.directories, vec![root()]);
            assert!(!plan.access.missing_with(|p| Ok(p.into()), |_| false).unwrap().is_empty());
        }
    }

    #[test]
    fn sequence_segments_modules_and_name_changes_use_the_actual_worker_plan() {
        use effectcraft_project::render_queue::{OutputFormat, sequence_path};
        let mut session = session();
        {
            let item = &mut std::sync::Arc::make_mut(&mut session.project).render_queue[0];
            item.output.format = OutputFormat::PngSequence;
            item.output.output = root().join("frames_[#####].png").to_string_lossy().into_owned();
            let mut extra = item.output.clone();
            extra.format = OutputFormat::Uncompressed;
            extra.output = root().join("elsewhere/second.mov").to_string_lossy().into_owned();
            item.extra_outputs.push(extra);
        }
        session.prefs.export.segment_sequences = true;
        session.prefs.export.segment_sequence_files = 2;
        session.prefs.export.append_bits_to_name = true;
        let plan = session.plan_render().unwrap();
        assert!(plan.items[0].len() >= 3, "two image segments and the extra movie");
        for (item, path) in &plan.items[0] {
            let actual = if item.output.format.is_sequence() {
                sequence_path(path, item.settings.first_frame(session.project.comp(item.comp).unwrap()))
            } else {
                path.clone()
            };
            let parent = Path::new(&actual).parent().unwrap();
            assert!(plan.access.directories.iter().any(|directory| parent.starts_with(directory)), "{actual}");
        }
        assert!(plan.access.accept_with(&root(), |p| Ok(p.into()), |_| false).is_err());
        assert!(plan.access.accept_with(&root(), |p| Ok(p.into()), |_| true).is_ok());
        assert!(
            session.project.render_queue.iter().all(|item| item.started.is_none() && item.status == effectcraft_project::render_queue::RenderStatus::Queued)
        );
        assert!(session.render_job.is_none(), "planning never starts output");
    }

    #[test]
    fn movie_segments_and_separate_queue_items_are_preflighted_as_one_complete_batch() {
        let mut segmented = session();
        let comp_id = segmented.active_comp_id().unwrap();
        let comp = std::sync::Arc::make_mut(&mut segmented.project).comp_mut(comp_id).unwrap();
        comp.duration = effectcraft_time::Tick::from_seconds_f64(10.0);
        comp.work_area = (effectcraft_time::Tick::ZERO, comp.duration);
        segmented.prefs.export.segment_movies = true;
        segmented.prefs.export.segment_movie_mb = 1;
        let plan = segmented.plan_render().unwrap();
        assert!(plan.items[0].len() > 1);
        assert!(plan.items[0][0].1.ends_with("movie_001.mov"));
        assert_eq!(plan.access.directories, vec![root()]);
        assert!(segmented.render_job.is_none());

        let mut queued = session();
        let mut second = queued.project.render_queue[0].clone();
        second.id = 2;
        second.output.output = root().join("second-folder/movie.mov").to_string_lossy().into_owned();
        std::sync::Arc::make_mut(&mut queued.project).render_queue.push(second);
        let plan = queued.plan_render().unwrap();
        assert_eq!(plan.items.len(), 2);
        assert_eq!(plan.access.directories, vec![root()], "one explicitly chosen parent can cover both queued destinations");
        assert!(!plan.access.missing_with(|p| Ok(p.into()), |_| false).unwrap().is_empty());
        assert!(queued.render_job.is_none());
    }

    #[test]
    fn overflow_roots_are_preflighted_only_when_a_quota_can_select_them() {
        struct Full;
        impl effectcraft_project::render_queue::StorageQuota for Full {
            fn has_room(&self, _: &str, _: u64) -> bool {
                false
            }
        }
        let mut session = session();
        let overflow = root().join("overflow");
        std::sync::Arc::make_mut(&mut session.project).render_prefs.overflow_folders.push(overflow.to_string_lossy().into_owned());
        assert!(session.plan_render().unwrap().access.is_empty());
        session.storage_quota = Some(std::sync::Arc::new(Full));
        assert_eq!(session.plan_render().unwrap().access.directories, vec![overflow]);
    }
}
