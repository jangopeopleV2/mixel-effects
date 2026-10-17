//! File dialogs that don't block the frame (macOS), for `Hooks::file_dialog_async`.
//!
//! A blocking `rfd::FileDialog` runs a nested modal event loop inside eframe's frame, that is
//! inside winit's event handler. On macOS a file dragged over the window while that loop ran
//! re-entered winit's handler, which panics across the Objective-C boundary and aborts the app
//! (#511). `rfd::AsyncFileDialog` shows the dialog as a sheet on the window and returns at once;
//! the answer arrives when the sheet closes, outside the frame, and the app runs the command then.

use effectcraft_ui_egui::{DialogDone, FileDialog};
use rfd::{AsyncFileDialog, FileHandle};
use std::future::Future;
use std::pin::Pin;

type Answer = Pin<Box<dyn Future<Output = Vec<String>> + Send>>;

/// Show `dialog` (on the main thread: the sheet opens here) and hand its paths to `done` once it
/// closes, then wake the UI.
#[cfg(not(all(target_os = "macos", feature = "mac-app-store")))]
pub fn show(dialog: FileDialog, done: DialogDone, ctx: egui::Context) {
    let answer = open(dialog, AsyncFileDialog::new());
    let spawned = std::thread::Builder::new().name("ec-file-dialog".into()).spawn(move || {
        done(pollster::block_on(answer));
        ctx.request_repaint();
    });
    if let Err(e) = spawned {
        log::error!("file dialog: {e}");
    }
}

/// App Store panels must begin on the running AppKit main thread with a real
/// parent. Constructing an unparented rfd future can itself enter runModal.
#[cfg(all(target_os = "macos", feature = "mac-app-store"))]
pub fn show_mas(dialog: FileDialog, done: DialogDone, parent: Option<&AsyncFileDialog>, ctx: egui::Context) -> Result<(), String> {
    let mtm = objc2::MainThreadMarker::new().ok_or("File selection must start from the app window")?;
    if !objc2_app_kit::NSApplication::sharedApplication(mtm).isRunning() {
        return Err("The app window is not ready for file selection".into());
    }
    let parent = parent.cloned().ok_or("No app window is available for file selection")?;
    // Start the waiter before opening the sheet. Failure drops `done`, which the
    // UI reports as a disconnected chooser; it never retries synchronously.
    let (tx, rx) = std::sync::mpsc::channel::<Answer>();
    std::thread::Builder::new()
        .name("ec-file-sheet".into())
        .spawn(move || {
            if let Ok(answer) = rx.recv() {
                done(pollster::block_on(answer));
                ctx.request_repaint();
            }
        })
        .map_err(|error| format!("Could not start the file chooser: {error}"))?;
    // rfd starts the sheet while constructing these futures on main. Only
    // waiting for the answer moves to the worker.
    tx.send(open(dialog, parent)).map_err(|_| "The file chooser stopped before it could answer".to_owned())
}

fn paths(files: Option<Vec<FileHandle>>, save: bool) -> Vec<String> {
    files
        .unwrap_or_default()
        .into_iter()
        .filter_map(|f| effectcraft_engine::macos_mas::picked(f.path(), save))
        .map(|p| p.to_string_lossy().into_owned())
        .collect()
}

fn path(file: Option<FileHandle>, save: bool) -> Vec<String> {
    paths(file.map(|f| vec![f]), save)
}

/// The dialog, as the blocking hooks in `main.rs` show it.
fn open(dialog: FileDialog, parent: AsyncFileDialog) -> Answer {
    match dialog {
        FileDialog::Open { exts, many } => {
            let d = parent.add_filter("Media", &exts);
            if many {
                let f = d.pick_files();
                Box::pin(async move { paths(f.await, false) })
            } else {
                let f = d.pick_file();
                Box::pin(async move { path(f.await, false) })
            }
        }
        FileDialog::OpenProject => {
            let f = parent.add_filter("MIXEL Effects Project", &["ecproj", "ecprojx"]).pick_file();
            Box::pin(async move { path(f.await, false) })
        }
        FileDialog::Folder => {
            let f = parent.pick_folder();
            Box::pin(async move { path(f.await, false) })
        }
        FileDialog::ProjectFolder { project } => {
            let mut d = parent.set_title("Choose the project folder to allow linked footage");
            if let Some(parent) = std::path::Path::new(&project).parent() {
                d = d.set_directory(parent);
            }
            let f = d.pick_folder();
            Box::pin(async move { path(f.await, false) })
        }
        FileDialog::Save { default, ext, project } => {
            let at = std::path::Path::new(&default);
            let name = at.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let mut d = if project {
                parent.add_filter("MIXEL Effects Project", &["ecproj"])
            } else if ext.is_empty() {
                parent
            } else {
                parent.add_filter(&ext, &[ext.as_str()])
            };
            // A default with a folder that exists opens the dialog there.
            if let Some(dir) = at.parent().filter(|d| d.is_dir()) {
                d = d.set_directory(dir);
            }
            let f = d.set_file_name(name).save_file();
            Box::pin(async move { path(f.await, true) })
        }
    }
}
