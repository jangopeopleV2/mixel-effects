//! Continuations for App Store file sheets. Native answers only cross a channel;
//! document and transient UI changes stay on the UI thread.

use crate::{Dialog, EffectcraftApp, FileDialog};
use effectcraft_engine::project::{ItemId, LayerId};
use effectcraft_engine::shortcuts::Keymaps;
use std::sync::mpsc::{Receiver, TryRecvError};

#[derive(PartialEq)]
struct Origin {
    epoch: u64,
    revision: u64,
    prefs_revision: u64,
    path: Option<String>,
    comp: Option<ItemId>,
    layers: Vec<LayerId>,
    props: Vec<(LayerId, u64)>,
    keys: Vec<effectcraft_engine::KeyRef>,
    vertices: Vec<effectcraft_engine::VertexRef>,
    camera_points: Vec<u32>,
    track: Option<(LayerId, u64)>,
    project_selection: Vec<ItemId>,
    times: std::collections::BTreeMap<ItemId, effectcraft_engine::time::Tick>,
    keymaps: Keymaps,
    dialog: Option<Dialog>,
}

impl Origin {
    fn of(app: &EffectcraftApp) -> Self {
        let session = &app.session;
        Self {
            epoch: session.script_ui.project_epoch,
            revision: session.revision,
            prefs_revision: session.prefs_revision,
            path: session.path.clone(),
            comp: session.state.active_comp,
            layers: session.state.selected_layers.clone(),
            props: session.state.selected_props.clone(),
            keys: session.state.selected_keys.clone(),
            vertices: session.state.selected_vertices.clone(),
            camera_points: session.state.camera_points.clone(),
            track: session.state.current_track,
            project_selection: session.state.project_selection.clone(),
            times: session.state.times.clone(),
            keymaps: session.keymaps.clone(),
            dialog: app.dialog,
        }
    }
}

pub(crate) struct Pending {
    origin: Origin,
    rx: Receiver<Vec<String>>,
    begin: Option<(FileDialog, crate::DialogDone)>,
    continuation: Box<dyn FnOnce(&mut EffectcraftApp, Vec<String>)>,
}

/// Queue startup for the next UI frame. `false` preserves the caller's ordinary
/// synchronous host behavior. A strict host always returns `true`, including
/// failure: it must never enter runModal.
pub(crate) fn defer(app: &mut EffectcraftApp, request: FileDialog, continuation: impl FnOnce(&mut EffectcraftApp, Vec<String>) + 'static) -> bool {
    if !app.async_dialogs_required {
        return false;
    }
    if app.pending_file_dialog.is_some() {
        app.ui.status = "Finish the open file chooser before opening another one".into();
        return true;
    }
    if app.hooks.file_dialog_async.is_none() {
        app.ui.status = "No file chooser is available. Try again from the app window.".into();
        return true;
    }
    let origin = Origin::of(app);
    let (tx, rx) = std::sync::mpsc::channel();
    app.pending_file_dialog = Some(Pending {
        origin,
        rx,
        begin: Some((
            request,
            Box::new(move |paths| {
                let _ = tx.send(paths);
            }),
        )),
        continuation: Box::new(continuation),
    });
    if let Some(ctx) = app.frames.context() {
        ctx.request_repaint();
    }
    true
}

pub(crate) fn poll(app: &mut EffectcraftApp) {
    let Some(mut pending) = app.pending_file_dialog.take() else { return };
    if let Some((request, done)) = pending.begin.take() {
        if pending.origin != Origin::of(app) {
            app.ui.status = "The document or selection changed while choosing a file. Choose it again.".into();
            return;
        }
        let Some(start) = app.hooks.file_dialog_async.as_ref() else {
            app.ui.status = "No file chooser is available. Try again from the app window.".into();
            return;
        };
        // Startup Finder requests reach here only after AppKit's event loop is
        // running. No panel is constructed during the app-creation callback.
        app.pending_file_dialog = Some(pending);
        start(request, done);
        if let Some(ctx) = app.frames.context() {
            ctx.request_repaint();
        }
        return;
    }
    match pending.rx.try_recv() {
        Ok(paths) if paths.is_empty() => {}
        Ok(paths) if pending.origin == Origin::of(app) => (pending.continuation)(app, paths),
        Ok(_) => app.ui.status = "The document or selection changed while choosing a file. Choose it again.".into(),
        Err(TryRecvError::Empty) => {
            app.pending_file_dialog = Some(pending);
            if let Some(ctx) = app.frames.context() {
                ctx.request_repaint_after(std::time::Duration::from_millis(250));
            }
        }
        Err(TryRecvError::Disconnected) => {
            app.ui.status = "The file chooser stopped before answering. Try again from the app window.".into();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DialogDone;
    use std::sync::{Arc, Mutex};

    fn strict() -> (EffectcraftApp, Arc<Mutex<Vec<(FileDialog, DialogDone)>>>) {
        let mut app = EffectcraftApp::new(effectcraft_engine::Session::default());
        app.require_async_file_dialogs();
        let queue = Arc::new(Mutex::new(Vec::new()));
        let pending = queue.clone();
        app.hooks.file_dialog_async = Some(Box::new(move |request, done| pending.lock().unwrap().push((request, done))));
        app.hooks.pick_files = Some(Box::new(|_| panic!("strict hosts must not run a synchronous chooser")));
        (app, queue)
    }

    #[test]
    fn callback_waits_for_a_later_ui_poll_and_cancel_disconnect_do_nothing() {
        let (mut app, queue) = strict();
        assert!(defer(&mut app, FileDialog::Folder, |app, paths| app.ui.status = paths[0].clone()));
        assert!(app.ui.status.is_empty());
        assert!(queue.lock().unwrap().is_empty(), "start on a later frame, outside app creation");
        poll(&mut app);
        let (_, done) = queue.lock().unwrap().pop().unwrap();
        std::thread::spawn(move || done(vec!["chosen".into()])).join().unwrap();
        assert!(app.ui.status.is_empty());
        poll(&mut app);
        assert_eq!(app.ui.status, "chosen");
        for cancel in [true, false] {
            assert!(defer(&mut app, FileDialog::Folder, |_, _| panic!("no accepted answer")));
            poll(&mut app);
            let (_, done) = queue.lock().unwrap().pop().unwrap();
            if cancel {
                done(Vec::new());
            } else {
                drop(done);
            }
            poll(&mut app);
            assert!(app.pending_file_dialog.is_none());
        }
        assert!(app.ui.status.contains("stopped before answering"));
    }

    #[test]
    fn changed_origin_rejects_answer_without_mutating_the_new_target() {
        for change in 0..10 {
            let (mut app, queue) = strict();
            assert!(defer(&mut app, FileDialog::Folder, |_, _| panic!("stale answer")));
            poll(&mut app);
            match change {
                0 => app.session.revision += 1,
                1 => app.session.script_ui.project_epoch += 1,
                2 => app.session.state.active_comp = Some(ItemId(99)),
                3 => app.session.state.selected_layers.push(LayerId(99)),
                4 => app.session.prefs_revision += 1,
                5 => app.session.keymaps.active = "Different preset".into(),
                6 => app.session.state.selected_vertices.push(effectcraft_engine::VertexRef { layer: LayerId(99), mask: 99, index: 0 }),
                7 => app.session.state.camera_points.push(99),
                8 => app.session.state.current_track = Some((LayerId(99), 99)),
                _ => app.dialog = Some(Dialog::Settings),
            }
            let (_, done) = queue.lock().unwrap().pop().unwrap();
            done(vec!["chosen".into()]);
            poll(&mut app);
            assert!(app.ui.status.contains("changed while choosing"));
        }
    }

    #[test]
    fn missing_service_never_falls_back_and_only_one_sheet_is_started() {
        let (mut app, queue) = strict();
        assert!(defer(&mut app, FileDialog::Folder, |_, _| {}));
        assert!(defer(&mut app, FileDialog::OpenProject, |_, _| panic!("second chooser")));
        poll(&mut app);
        assert_eq!(queue.lock().unwrap().len(), 1);
        drop(queue.lock().unwrap().pop());
        poll(&mut app);
        app.hooks.file_dialog_async = None;
        assert!(defer(&mut app, FileDialog::Folder, |_, _| panic!("missing chooser")));
        assert!(app.ui.status.contains("No file chooser"));
        assert!(app.pending_file_dialog.is_none());
        app.async_dialogs_required = false;
        assert!(!defer(&mut app, FileDialog::Folder, |_, _| {}));
    }
}
