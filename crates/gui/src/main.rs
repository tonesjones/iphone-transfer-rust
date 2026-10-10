#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod backup;

use anyhow::Context;
use backup::{Event, Folders};
use clap::Parser;
use eframe::egui::{self, Color32, RichText};
use std::{path::PathBuf, sync::mpsc, time::Duration};

const INK: Color32 = Color32::from_rgb(32, 47, 63);
const TEAL: Color32 = Color32::from_rgb(19, 109, 99);
const MUTED: Color32 = Color32::from_rgb(87, 104, 119);
const LINE: Color32 = Color32::from_rgb(220, 229, 233);
const BACKGROUND: Color32 = Color32::from_rgb(244, 247, 249);
const SUCCESS_FILL: Color32 = Color32::from_rgb(224, 243, 237);
const ERROR_FILL: Color32 = Color32::from_rgb(255, 236, 235);
const PENDING_FILL: Color32 = Color32::from_rgb(236, 242, 245);
const NOTE_FILL: Color32 = Color32::from_rgb(255, 248, 230);
const NOTE_LINE: Color32 = Color32::from_rgb(237, 219, 175);
const NOTE_TEXT: Color32 = Color32::from_rgb(126, 91, 31);

const STEP_TITLES: [&str; 3] = [
    "Save from iCloud",
    "Check saved library",
    "Check Photo Backups",
];
const STEP_HINTS: [&str; 3] = [
    "Copy new or missing photos and videos.",
    "Verify the contents of every saved file.",
    "Update and verify your second copy on this computer.",
];

fn semibold(size: f32) -> egui::FontId {
    egui::FontId::new(size, egui::FontFamily::Name("semibold".into()))
}

fn heading(text: &str, size: f32) -> RichText {
    RichText::new(text).font(semibold(size)).color(INK)
}

fn note(text: &str, size: f32) -> RichText {
    RichText::new(text).size(size).color(MUTED)
}

fn card(fill: Color32, margin: f32) -> egui::Frame {
    egui::Frame::new()
        .fill(fill)
        .inner_margin(margin)
        .corner_radius(10)
}

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let windows =
        PathBuf::from(std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into()));
    for (name, file) in [
        ("Segoe UI", "segoeui.ttf"),
        ("Segoe UI Semibold", "seguisb.ttf"),
    ] {
        if let Ok(bytes) = std::fs::read(windows.join("Fonts").join(file)) {
            fonts
                .font_data
                .insert(name.into(), egui::FontData::from_owned(bytes).into());
        }
    }
    if fonts.font_data.contains_key("Segoe UI") {
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "Segoe UI".into());
    }
    let mut semibold = fonts.families[&egui::FontFamily::Proportional].clone();
    if fonts.font_data.contains_key("Segoe UI Semibold") {
        semibold.insert(0, "Segoe UI Semibold".into());
    }
    fonts
        .families
        .insert(egui::FontFamily::Name("semibold".into()), semibold);
    ctx.set_fonts(fonts);
}

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
    Done(String),
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
                Ok(Event::Completed(index, text)) => self.steps[index] = Status::Done(text),
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

    fn header(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("PHOTOXFER").size(12.0).color(TEAL));
        ui.add_space(6.0);
        ui.label(heading("Your photo backups", 30.0));
        ui.label(note(
            "Save new photos and videos, then verify both local copies.",
            16.0,
        ));
        ui.add_space(22.0);
        let (label, title, detail) = if self.running {
            let detail = if self.close_blocked {
                "Still running. Close the window after the backup finishes."
            } else {
                "Keep this window open until it finishes."
            };
            ("Backing up…", "Backup in progress", detail)
        } else if self.finished {
            (
                "Back Up Now",
                "Local backup checked",
                "Your phone and iCloud files stay untouched.",
            )
        } else {
            (
                "Back Up Now",
                "Ready when you are",
                "Your phone and iCloud files stay untouched.",
            )
        };
        ui.horizontal(|ui| {
            let button = egui::Button::new(heading(label, 17.0).color(Color32::WHITE))
                .fill(TEAL)
                .corner_radius(8)
                .min_size(egui::vec2(170.0, 48.0));
            if ui.add_enabled(!self.running, button).clicked() {
                self.start();
            }
            ui.add_space(10.0);
            ui.vertical(|ui| {
                ui.label(heading(title, 15.0));
                let detail = RichText::new(detail).small();
                ui.label(if self.close_blocked {
                    detail.color(Color32::DARK_RED)
                } else {
                    detail.color(MUTED)
                });
            });
        });
    }

    fn steps(&self, ui: &mut egui::Ui) {
        ui.label(heading("Three steps to a checked backup", 17.0));
        ui.add_space(10.0);
        for (index, step) in self.steps.iter().enumerate() {
            card(Color32::WHITE, 12.0)
                .stroke(egui::Stroke::new(1.0, LINE))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let (circle, _) =
                            ui.allocate_exact_size(egui::vec2(34.0, 34.0), egui::Sense::hover());
                        let (fill, badge, color) = match step {
                            Status::Done(_) => (SUCCESS_FILL, "✓".into(), TEAL),
                            Status::Failed => (ERROR_FILL, "!".into(), Color32::DARK_RED),
                            _ => (PENDING_FILL, (index + 1).to_string(), MUTED),
                        };
                        ui.painter().circle_filled(circle.center(), 17.0, fill);
                        ui.painter().text(
                            circle.center(),
                            egui::Align2::CENTER_CENTER,
                            badge,
                            egui::FontId::proportional(16.0),
                            color,
                        );
                        ui.add_space(6.0);
                        ui.vertical(|ui| {
                            ui.label(heading(STEP_TITLES[index], 16.0));
                            let detail = match step {
                                Status::Done(text) => text.as_str(),
                                _ => STEP_HINTS[index],
                            };
                            ui.label(note(detail, 14.0));
                        });
                        if matches!(step, Status::Working) {
                            ui.spinner();
                        }
                    });
                });
            ui.add_space(8.0);
        }
    }

    fn result(&self, ui: &mut egui::Ui) {
        if let Some(error) = &self.error {
            card(ERROR_FILL, 16.0).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.label(heading("Backup needs attention", 16.0).color(Color32::DARK_RED));
                ui.label(
                    "Keep your phone/iCloud copies. Resolve the problem below, then try again.",
                );
                egui::CollapsingHeader::new("Problem details")
                    .default_open(true)
                    .show(ui, |ui| ui.label(error));
            });
        } else if self.finished {
            card(SUCCESS_FILL, 16.0).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.label(heading("Both local copies passed their checks", 16.0).color(TEAL));
                ui.label(note(
                    "Before deleting anything, complete the phone and cloud checks below.",
                    14.0,
                ));
            });
        }
    }

    fn checklist(ui: &mut egui::Ui) {
        card(NOTE_FILL, 18.0)
            .stroke(egui::Stroke::new(1.0, NOTE_LINE))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.label(heading("Before deleting photos", 17.0));
                ui.label(
                    RichText::new("Two checks still need your confirmation")
                        .size(13.0)
                        .color(NOTE_TEXT),
                );
                ui.add_space(8.0);
                ui.label(heading("1. Compare with your phone", 14.0));
                ui.label(note(
                    "Include every item and both parts of your Live Photo.",
                    14.0,
                ));
                ui.add_space(6.0);
                ui.label(heading("2. Test a fresh cloud download", 14.0));
                ui.label(note(
                    "Wait for OneDrive to upload. \
                     Download a fresh backup and verify its recovery file list.",
                    14.0,
                ));
                ui.add_space(6.0);
                ui.label(note("Local checks cannot confirm a OneDrive upload.", 12.0));
            });
    }

    fn folders(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new(heading("Your backup folders", 15.0))
            .default_open(false)
            .show(ui, |ui| {
                let folders = self.folders.clone();
                self.folder(ui, "Read from iCloud", &folders.source);
                ui.add_space(8.0);
                self.folder(ui, "Saved library", &folders.library);
                ui.add_space(8.0);
                self.folder(ui, "Photo Backups", &folders.archive);
                if let Some(error) = &self.folder_error {
                    ui.add_space(8.0);
                    ui.label(RichText::new(error).small().color(Color32::DARK_RED));
                }
            });
    }

    fn folder(&mut self, ui: &mut egui::Ui, label: &str, path: &std::path::Path) {
        let path: PathBuf = path.components().collect();
        ui.horizontal(|ui| {
            ui.label(heading(label, 15.0));
            if ui.small_button("Open folder").clicked() {
                self.folder_error = if !path.is_dir() {
                    Some(format!(
                        "{} doesn't exist yet. Run a backup first.",
                        path.display()
                    ))
                } else if let Err(error) = std::process::Command::new("explorer.exe")
                    .arg(&path)
                    .spawn()
                {
                    Some(format!("Could not open {}: {error}", path.display()))
                } else {
                    None
                };
            }
        });
        ui.label(
            RichText::new(path.display().to_string())
                .small()
                .color(MUTED),
        );
    }
}

impl eframe::App for App {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll();
        let ctx = root.ctx().clone();
        if self.running {
            ctx.request_repaint_after(Duration::from_millis(100));
            if ctx.input(|i| i.viewport().close_requested()) {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.close_blocked = true;
            }
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BACKGROUND).inner_margin(28.0))
            .show(root, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.header(ui);
                    ui.add_space(24.0);
                    self.steps(ui);
                    self.result(ui);
                    ui.add_space(16.0);
                    Self::checklist(ui);
                    ui.add_space(16.0);
                    self.folders(ui);
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

fn launch() -> anyhow::Result<()> {
    let folders = default_folders(Args::try_parse()?)?;
    eframe::run_native(
        "PhotoXfer — Photo Backups",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([820.0, 860.0])
                .with_min_inner_size([640.0, 620.0]),
            centered: true,
            ..Default::default()
        },
        Box::new(move |cc| {
            install_fonts(&cc.egui_ctx);
            cc.egui_ctx.set_theme(egui::Theme::Light);
            cc.egui_ctx.global_style_mut(|style| {
                style.visuals.panel_fill = BACKGROUND;
                style.spacing.button_padding = egui::vec2(20.0, 12.0);
                style.spacing.item_spacing = egui::vec2(10.0, 6.0);
                style.visuals.override_text_color = Some(INK);
                style
                    .text_styles
                    .insert(egui::TextStyle::Body, egui::FontId::proportional(16.0));
                style
                    .text_styles
                    .insert(egui::TextStyle::Button, egui::FontId::proportional(16.0));
                style
                    .text_styles
                    .insert(egui::TextStyle::Small, egui::FontId::proportional(13.0));
            });
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
