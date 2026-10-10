#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod backup;
mod stage;
mod theme;

use anyhow::Context;
use backup::{Event, Folders};
use clap::Parser;
use eframe::egui::{self, Align2, Color32, CursorIcon, RichText, Sense, Stroke, Vec2};
use std::{path::PathBuf, sync::mpsc};
use theme::*;

const SIDEBAR_WIDTH: f32 = 300.0;

struct Step {
    hint: &'static str,
    working: &'static str,
}

const STEPS: [Step; 3] = [
    Step {
        hint: "Copy new or missing photos and videos.",
        working: "Saving from iCloud",
    },
    Step {
        hint: "Verify the contents of every saved file.",
        working: "Checking library",
    },
    Step {
        hint: "Update and verify your second copy on this computer.",
        working: "Checking backups",
    },
];

#[derive(Parser)]
struct Args {
    #[arg(long)]
    from: Option<PathBuf>,
    #[arg(long)]
    to: Option<PathBuf>,
    #[arg(long)]
    archive: Option<PathBuf>,
}

#[derive(Default)]
enum Status {
    #[default]
    Waiting,
    Working,
    Done(String, u64),
    Failed,
}

struct App {
    folders: Folders,
    steps: [Status; 3],
    receiver: Option<mpsc::Receiver<Event>>,
    running: bool,
    finished: bool,
    close_blocked: bool,
    error: Option<String>,
    folder_error: Option<String>,
    /// The worker's latest overall progress, and the eased value the progress bar shows.
    progress: f32,
    shown_progress: f32,
}

impl App {
    fn new(folders: Folders) -> Self {
        Self {
            folders,
            steps: Default::default(),
            receiver: None,
            running: false,
            finished: false,
            close_blocked: false,
            error: None,
            folder_error: None,
            progress: 0.0,
            shown_progress: 0.0,
        }
    }

    fn start(&mut self) {
        if self.running {
            return;
        }
        self.steps = Default::default();
        self.finished = false;
        self.close_blocked = false;
        self.error = None;
        self.progress = 0.0;
        self.shown_progress = 0.0;
        self.running = true;
        let (sender, receiver) = mpsc::channel();
        self.receiver = Some(receiver);
        let folders = self.folders.clone();
        std::thread::spawn(move || {
            backup::run(&folders, |event| {
                let _ = sender.send(event);
            })
        });
    }

    fn fail(&mut self, message: String) {
        for step in &mut self.steps {
            if matches!(step, Status::Working) {
                *step = Status::Failed;
            }
        }
        self.error = Some(message);
        self.running = false;
    }

    fn poll(&mut self) {
        let Some(receiver) = self.receiver.take() else {
            return;
        };
        self.receiver = loop {
            match receiver.try_recv() {
                Ok(Event::Started(index)) => self.steps[index] = Status::Working,
                Ok(Event::Progress(done)) => self.progress = done,
                Ok(Event::Completed(index, text, files)) => {
                    self.steps[index] = Status::Done(text, files)
                }
                Ok(Event::Failed(message)) => self.fail(message),
                Ok(Event::Finished) => {
                    self.finished = true;
                    self.running = false;
                }
                Err(mpsc::TryRecvError::Empty) => break Some(receiver),
                Err(mpsc::TryRecvError::Disconnected) => {
                    if self.running {
                        self.fail(
                            "The backup worker stopped unexpectedly. \
                             Keep your phone/iCloud copies and try again."
                                .into(),
                        );
                    }
                    break None;
                }
            }
        };
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(36.0), Sense::hover());
            logo(ui.painter(), rect);
            ui.add_space(4.0);
            ui.vertical(|ui| {
                ui.add_space(1.0);
                ui.label(RichText::new("PhotoXfer").font(bold(18.0)));
                ui.label(RichText::new("iPhone photo backup").size(12.5).color(MUTED));
            });
        });
        ui.add_space(30.0);
        ui.label(eyebrow("YOUR FOLDERS"));
        ui.add_space(4.0);
        let folders = self.folders.clone();
        let tiles = [
            ("iCloud Photos", "found", &folders.source, CYAN),
            ("Saved library", "verified", &folders.library, VIOLET),
            ("Photo Backups", "verified", &folders.archive, AMBER),
        ];
        for (index, (label, verb, path, color)) in tiles.into_iter().enumerate() {
            self.folder_tile(ui, index, label, verb, path, color);
            ui.add_space(4.0);
        }
        if let Some(error) = &self.folder_error {
            ui.add_space(4.0);
            ui.label(RichText::new(error).size(12.5).color(ERROR));
        }
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            ui.label(
                RichText::new("Your phone and iCloud files are never changed or deleted.")
                    .size(12.0)
                    .color(FAINT),
            );
        });
    }

    fn folder_tile(
        &mut self,
        ui: &mut egui::Ui,
        index: usize,
        label: &str,
        verb: &str,
        path: &std::path::Path,
        color: Color32,
    ) {
        let path: PathBuf = path.components().collect();
        let mut tile = egui::Frame::new()
            .fill(CARD)
            .stroke(Stroke::new(1.0, LINE))
            .corner_radius(12)
            .inner_margin(egui::Margin::symmetric(14, 12))
            .begin(ui);
        {
            let ui = &mut tile.content_ui;
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                let (icon, _) = ui.allocate_exact_size(Vec2::new(18.0, 14.0), Sense::hover());
                folder_icon(ui.painter(), icon, color);
                ui.label(RichText::new(label).font(semibold(14.5)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new("Open ›").size(12.5).color(MUTED));
                });
            });
            ui.add_space(2.0);
            let row = egui::Layout::left_to_right(egui::Align::Max);
            let size = Vec2::new(ui.available_width(), 30.0);
            ui.allocate_ui_with_layout(size, row, |ui| match &self.steps[index] {
                Status::Done(_, files) => {
                    ui.label(RichText::new(files.to_string()).font(bold(22.0)));
                    ui.label(
                        RichText::new(format!("files {verb}"))
                            .size(13.0)
                            .color(MUTED),
                    );
                }
                Status::Working => {
                    ui.add(egui::Spinner::new().size(16.0).color(color));
                    ui.label(RichText::new("Counting…").size(13.0).color(MUTED));
                }
                Status::Failed => {
                    ui.label(RichText::new("Needs attention").size(13.0).color(ERROR));
                }
                Status::Waiting => {
                    ui.label(RichText::new("—").font(bold(22.0)).color(FAINT));
                    ui.label(RichText::new("not checked yet").size(13.0).color(FAINT));
                }
            });
            ui.label(RichText::new(short_path(&path)).size(12.0).color(FAINT));
        }
        let response = tile
            .allocate_space(ui)
            .interact(Sense::click())
            .on_hover_cursor(CursorIcon::PointingHand)
            .on_hover_text(format!("Open {} in File Explorer", path.display()));
        if response.hovered() {
            tile.frame.fill = CARD_HOVER;
            tile.frame.stroke = Stroke::new(1.0, color.gamma_multiply(0.6));
        }
        tile.paint(ui);
        if response.clicked() {
            self.folder_error = open_folder(&path).err();
        }
    }

    fn header(&self, ui: &mut egui::Ui) {
        let (title, detail, detail_color) = if self.running {
            if self.close_blocked {
                (
                    "Backing up…",
                    "Still running. Close the window after the backup finishes.",
                    AMBER,
                )
            } else {
                (
                    "Backing up…",
                    "Keep this window open until it finishes.",
                    MUTED,
                )
            }
        } else if self.error.is_some() {
            (
                "Backup needs attention",
                "Nothing was deleted. Fix the problem below, then try again.",
                MUTED,
            )
        } else if self.finished {
            (
                "Local backup checked",
                "Both local copies match your iCloud photos and videos.",
                MUTED,
            )
        } else {
            (
                "Ready when you are",
                "Save new photos and videos, then verify both local copies.",
                MUTED,
            )
        };
        ui.label(RichText::new(title).font(bold(30.0)));
        ui.label(RichText::new(detail).size(15.0).color(detail_color));
    }

    fn result(&self, ui: &mut egui::Ui) {
        if let Some(error) = &self.error {
            banner(ui, ERROR, |ui| {
                ui.label(
                    RichText::new("Keep your phone and iCloud copies")
                        .font(semibold(15.5))
                        .color(ERROR),
                );
                ui.label(
                    RichText::new("The backup stopped before finishing. Nothing was deleted.")
                        .color(MUTED),
                );
                ui.add_space(4.0);
                egui::CollapsingHeader::new(
                    RichText::new("Technical details").size(13.5).color(TEXT),
                )
                .default_open(true)
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(110.0)
                        .show(ui, |ui| {
                            ui.add(
                                egui::Label::new(RichText::new(error).monospace().color(MUTED))
                                    .selectable(true),
                            );
                        });
                });
            });
        } else if self.finished {
            banner(ui, SUCCESS, |ui| {
                ui.label(
                    RichText::new("Both local copies passed their checks")
                        .font(semibold(15.5))
                        .color(SUCCESS),
                );
                ui.label(
                    RichText::new("Before deleting anything, finish the two checks below.")
                        .color(MUTED),
                );
            });
        }
    }

    fn checklist(ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(eyebrow("BEFORE DELETING PHOTOS"));
            ui.label(
                RichText::new("· confirm these yourself")
                    .size(12.0)
                    .color(FAINT),
            );
        });
        ui.add_space(2.0);
        let items = [
            (
                "Compare with your phone",
                "Count every item, including both parts of your Live Photo.",
            ),
            (
                "Test a fresh cloud download",
                "Once OneDrive uploads, download a fresh copy and verify its recovery file list.",
            ),
        ];
        ui.columns(2, |columns| {
            for (column, (number, (title, detail))) in columns.iter_mut().zip((1..).zip(items)) {
                egui::Frame::new()
                    .fill(CARD)
                    .stroke(Stroke::new(1.0, LINE))
                    .corner_radius(12)
                    .inner_margin(14)
                    .show(column, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.horizontal(|ui| {
                            let (badge, _) =
                                ui.allocate_exact_size(Vec2::splat(22.0), Sense::hover());
                            ui.painter().circle_stroke(
                                badge.center(),
                                10.0,
                                Stroke::new(1.5, AMBER),
                            );
                            ui.painter().text(
                                badge.center(),
                                Align2::CENTER_CENTER,
                                number.to_string(),
                                semibold(12.0),
                                AMBER,
                            );
                            ui.label(RichText::new(title).font(semibold(14.5)));
                        });
                        // Columns justify text by default; keep it ragged-right.
                        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                            ui.label(RichText::new(detail).size(13.0).color(MUTED));
                        });
                    });
            }
        });
        ui.add_space(6.0);
        ui.label(
            RichText::new("Local checks can't confirm that OneDrive finished uploading.")
                .size(12.0)
                .color(FAINT),
        );
    }
}

fn eyebrow(text: &str) -> RichText {
    RichText::new(text)
        .font(semibold(11.5))
        .color(FAINT)
        .extra_letter_spacing(1.2)
}

/// A card with a colored stripe down its left edge.
fn banner(ui: &mut egui::Ui, accent: Color32, add: impl FnOnce(&mut egui::Ui)) {
    let response = egui::Frame::new()
        .fill(accent.gamma_multiply(0.08))
        .stroke(Stroke::new(1.0, accent.gamma_multiply(0.35)))
        .corner_radius(12)
        .inner_margin(egui::Margin {
            left: 20,
            right: 16,
            top: 14,
            bottom: 14,
        })
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            add(ui);
        })
        .response;
    let rect = response.rect;
    let stripe = egui::Rect::from_min_max(
        rect.left_top() + Vec2::new(6.0, 12.0),
        rect.left_bottom() + Vec2::new(9.0, -12.0),
    );
    ui.painter().rect_filled(stripe, 2.0, accent);
}

/// Keeps a path to one sidebar line by dropping leading folders, since the end names the folder.
fn short_path(path: &std::path::Path) -> String {
    const MAX_CHARS: usize = 36;
    let full = path.display().to_string();
    if full.chars().count() <= MAX_CHARS {
        return full;
    }
    let mut tail = String::new();
    for part in path.iter().rev() {
        let part = part.to_string_lossy();
        if !tail.is_empty() && tail.chars().count() + part.chars().count() + 3 > MAX_CHARS {
            break;
        }
        tail = if tail.is_empty() {
            part.into_owned()
        } else {
            format!("{part}\\{tail}")
        };
    }
    format!("…\\{tail}")
}

fn open_folder(path: &std::path::Path) -> Result<(), String> {
    if !path.is_dir() {
        return Err(format!(
            "{} doesn't exist yet. Run a backup first.",
            path.display()
        ));
    }
    std::process::Command::new("explorer.exe")
        .arg(path)
        .spawn()
        .map(drop)
        .map_err(|error| format!("Could not open {}: {error}", path.display()))
}

impl eframe::App for App {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll();
        let ctx = root.ctx().clone();
        if self.running {
            let dt = ctx.input(|i| i.stable_dt).min(0.1);
            self.shown_progress +=
                (self.progress - self.shown_progress) * (1.0 - (-dt * 6.0).exp());
            ctx.request_repaint();
            if ctx.input(|i| i.viewport().close_requested()) {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.close_blocked = true;
            }
        }
        egui::Panel::left("folders")
            .exact_size(SIDEBAR_WIDTH)
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(SIDEBAR).inner_margin(egui::Margin {
                left: 22,
                right: 22,
                top: 26,
                bottom: 20,
            }))
            .show(root, |ui| self.sidebar(ui));
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(BACKGROUND)
                    .inner_margin(egui::Margin::symmetric(36, 28)),
            )
            .show(root, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.header(ui);
                    ui.add_space(14.0);
                    self.stage(ui);
                    ui.add_space(14.0);
                    self.result(ui);
                    ui.add_space(18.0);
                    Self::checklist(ui);
                });
            });
    }
}

fn default_folders(args: Args) -> anyhow::Result<Folders> {
    let home = PathBuf::from(
        std::env::var_os("USERPROFILE")
            .context("USERPROFILE is not set; pass --from, --to, and --archive")?,
    );
    let onedrive = std::env::var_os("OneDrive")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("OneDrive"));
    Ok(Folders {
        source: args.from.unwrap_or_else(|| home.join("iCloudPhotos")),
        library: args
            .to
            .unwrap_or_else(|| home.join("Pictures").join("iPhone Backup")),
        archive: args
            .archive
            .unwrap_or_else(|| onedrive.join("Photo Backups")),
    })
}

/// Asks Windows for a dark title bar so the frame matches the app.
#[cfg(windows)]
fn dark_title_bar(cc: &eframe::CreationContext) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::{Foundation::HWND, Graphics::Dwm};
    let Ok(handle) = cc.window_handle() else {
        return;
    };
    if let RawWindowHandle::Win32(handle) = handle.as_raw() {
        let enabled: i32 = 1;
        // SAFETY: the handle is the live window eframe just created, and the attribute value is
        // a BOOL that outlives the call. Failure (older Windows) just keeps the default frame.
        unsafe {
            let _ = Dwm::DwmSetWindowAttribute(
                HWND(handle.hwnd.get() as _),
                Dwm::DWMWA_USE_IMMERSIVE_DARK_MODE,
                std::ptr::from_ref(&enabled).cast(),
                size_of::<i32>() as u32,
            );
        }
    }
}

fn launch() -> anyhow::Result<()> {
    let folders = default_folders(Args::try_parse()?)?;
    eframe::run_native(
        "PhotoXfer",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([980.0, 660.0])
                .with_min_inner_size([880.0, 600.0]),
            centered: true,
            ..Default::default()
        },
        Box::new(move |cc| {
            #[cfg(windows)]
            dark_title_bar(cc);
            theme::install(&cc.egui_ctx);
            Ok(Box::new(App::new(folders)))
        }),
    )
    .map_err(|error| anyhow::anyhow!("Could not open the PhotoXfer window: {error}"))
}

/// Release builds have no console, so startup errors need a dialog to be seen.
fn show_startup_error(message: &str) {
    eprintln!("{message}");
    #[cfg(windows)]
    {
        use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
        use windows::core::HSTRING;
        // SAFETY: both strings outlive the call, and no owner window is needed.
        unsafe {
            MessageBoxW(
                None,
                &HSTRING::from(message),
                &HSTRING::from("PhotoXfer could not start"),
                MB_OK | MB_ICONERROR,
            );
        }
    }
}

fn main() {
    if let Err(error) = launch() {
        show_startup_error(&format!("{error:#}"));
        std::process::exit(1);
    }
}
