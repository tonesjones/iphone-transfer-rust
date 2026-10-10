#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod backup;

use backup::{Event, Folders};
use clap::Parser;
use eframe::egui::{self, Color32, RichText};
use std::{path::PathBuf, sync::mpsc, time::Duration};

const INK: Color32 = Color32::from_rgb(32, 47, 63);
const TEAL: Color32 = Color32::from_rgb(19, 109, 99);
const MUTED: Color32 = Color32::from_rgb(87, 104, 119);
const LINE: Color32 = Color32::from_rgb(220, 229, 233);

fn heading(text: &str, size: f32) -> RichText {
    RichText::new(text)
        .font(egui::FontId::new(
            size,
            egui::FontFamily::Name("semibold".into()),
        ))
        .color(INK)
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
    error: Option<String>,
}

impl App {
    fn start(&mut self) {
        if self.running {
            return;
        }
        self.steps = Default::default();
        self.finished = false;
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

    fn poll(&mut self) {
        let Some(receiver) = &self.receiver else {
            return;
        };
        loop {
            match receiver.try_recv() {
                Ok(Event::Started(index)) => self.steps[index] = Status::Working,
                Ok(Event::Completed(index, text)) => self.steps[index] = Status::Done(text),
                Ok(Event::Failed(message)) => {
                    for step in &mut self.steps {
                        if matches!(step, Status::Working) {
                            *step = Status::Failed;
                        }
                    }
                    self.error = Some(message);
                    self.running = false;
                }
                Ok(Event::Finished) => {
                    self.finished = true;
                    self.running = false;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    if self.running {
                        self.error = Some("The backup worker stopped unexpectedly. Keep your phone/iCloud copies and try again.".into());
                        self.running = false;
                    }
                    break;
                }
            }
        }
    }

    fn folder(&mut self, ui: &mut egui::Ui, label: &str, path: PathBuf) {
        let path: PathBuf = path.components().collect();
        ui.horizontal(|ui| {
            ui.label(heading(label, 15.0));
            if ui.small_button("Open folder").clicked()
                && let Err(error) = std::process::Command::new("explorer.exe")
                    .arg(&path)
                    .spawn()
            {
                self.error = Some(format!("Could not open {}: {error}", path.display()));
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
            }
        }
        egui::CentralPanel::default().frame(egui::Frame::new().fill(Color32::from_rgb(244, 247, 249)).inner_margin(28.0)).show(root, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.label(RichText::new("PHOTOXFER").size(12.0).color(TEAL));
                ui.add_space(6.0);
                ui.label(heading("Your photo backups", 30.0));
                ui.label(RichText::new("Save new photos and videos, then verify both local copies.").color(MUTED));
                ui.add_space(22.0);
                ui.horizontal(|ui| {
                    let button = egui::Button::new(heading(if self.running { "Backing up…" } else { "Back Up Now" }, 17.0).color(Color32::WHITE)).fill(TEAL).corner_radius(8).min_size(egui::vec2(170.0, 48.0));
                    if ui.add_enabled(!self.running, button).clicked() { self.start(); }
                    ui.add_space(10.0);
                    ui.vertical(|ui| {
                        ui.label(heading(if self.running { "Backup in progress" } else if self.finished { "Local backup checked" } else { "Ready when you are" }, 15.0));
                        ui.label(RichText::new(if self.running { "Keep this window open until it finishes." } else { "Your phone and iCloud files stay untouched." }).small().color(MUTED));
                    });
                });
                ui.add_space(24.0);
                ui.label(heading("Three steps to a checked backup", 17.0));
                ui.add_space(10.0);
                let titles = ["Save from iCloud", "Check saved library", "Check Photo Backups"];
                let hints = ["Copy new or missing photos and videos.", "Verify the contents of every saved file.", "Update and verify your second copy on this computer."];
                for index in 0..3 {
                    egui::Frame::new().fill(Color32::WHITE).stroke(egui::Stroke::new(1.0, LINE)).inner_margin(12.0).corner_radius(10).show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.horizontal(|ui| {
                            let (circle, _) = ui.allocate_exact_size(egui::vec2(34.0, 34.0), egui::Sense::hover());
                            let (fill, text, color) = match self.steps[index] {
                                Status::Done(_) => (Color32::from_rgb(224, 243, 237), "✓".into(), TEAL),
                                Status::Failed => (Color32::from_rgb(255, 232, 232), "!".into(), Color32::DARK_RED),
                                _ => (Color32::from_rgb(236, 242, 245), (index + 1).to_string(), MUTED),
                            };
                            ui.painter().circle_filled(circle.center(), 17.0, fill);
                            ui.painter().text(circle.center(), egui::Align2::CENTER_CENTER, text, egui::FontId::proportional(16.0), color);
                            ui.add_space(6.0);
                            ui.vertical(|ui| {
                                ui.label(heading(titles[index], 16.0));
                                ui.label(RichText::new(match &self.steps[index] { Status::Done(text) => text.as_str(), _ => hints[index] }).size(14.0).color(MUTED));
                            });
                            if matches!(self.steps[index], Status::Working) { ui.spinner(); }
                        });
                    });
                    ui.add_space(8.0);
                }
                if let Some(error) = &self.error {
                    egui::Frame::new().fill(Color32::from_rgb(255, 236, 235)).inner_margin(16.0).corner_radius(10).show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.label(heading("Backup needs attention", 16.0).color(Color32::DARK_RED));
                        ui.label("Keep your phone/iCloud copies. Resolve the problem below, then try again.");
                        egui::CollapsingHeader::new("Problem details").default_open(true).show(ui, |ui| { ui.label(error); });
                    });
                } else if self.finished {
                    egui::Frame::new().fill(Color32::from_rgb(224, 243, 237)).inner_margin(16.0).corner_radius(10).show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.label(heading("Both local copies passed their checks", 16.0).color(TEAL));
                        ui.label(RichText::new("Before deleting anything, complete the phone and cloud checks below.").size(14.0).color(MUTED));
                    });
                }
                ui.add_space(16.0);
                egui::Frame::new().fill(Color32::from_rgb(255, 248, 230)).stroke(egui::Stroke::new(1.0, Color32::from_rgb(237, 219, 175))).inner_margin(18.0).corner_radius(10).show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.label(heading("Before deleting photos", 17.0));
                    ui.label(RichText::new("Two checks still need your confirmation").size(13.0).color(Color32::from_rgb(126, 91, 31)));
                    ui.add_space(8.0);
                    ui.label(RichText::new("1. Compare with your phone").font(egui::FontId::new(14.0, egui::FontFamily::Name("semibold".into()))));
                    ui.label(RichText::new("Include every item and both parts of your Live Photo.").size(14.0).color(MUTED));
                    ui.add_space(6.0);
                    ui.label(RichText::new("2. Test a fresh cloud download").font(egui::FontId::new(14.0, egui::FontFamily::Name("semibold".into()))));
                    ui.label(RichText::new("Wait for OneDrive to upload. Download a fresh backup and verify its recovery file list.").size(14.0).color(MUTED));
                    ui.add_space(6.0);
                    ui.label(RichText::new("Local checks cannot confirm a OneDrive upload.").size(12.0).color(MUTED));
                });
                ui.add_space(16.0);
                egui::CollapsingHeader::new(heading("Your backup folders", 15.0)).default_open(false).show(ui, |ui| {
                    self.folder(ui, "Read from iCloud", self.folders.source.clone());
                    ui.add_space(8.0);
                    self.folder(ui, "Saved library", self.folders.library.clone());
                    ui.add_space(8.0);
                    self.folder(ui, "Photo Backups", self.folders.archive.clone());
                });
            });
        });
    }
}

fn main() -> eframe::Result {
    let args = Args::parse();
    let home = PathBuf::from(std::env::var_os("USERPROFILE").unwrap_or_default());
    let onedrive = PathBuf::from(
        std::env::var_os("OneDrive").unwrap_or_else(|| home.join("OneDrive").into_os_string()),
    );
    let folders = Folders {
        source: args.from.unwrap_or_else(|| home.join("iCloudPhotos")),
        library: args
            .to
            .unwrap_or_else(|| home.join("Pictures").join("photoxfer")),
        archive: args
            .archive
            .unwrap_or_else(|| onedrive.join("Photo Backups")),
    };
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
                style.visuals.panel_fill = Color32::from_rgb(241, 245, 247);
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
            Ok(Box::new(App {
                folders,
                steps: Default::default(),
                receiver: None,
                running: false,
                finished: false,
                error: None,
            }))
        }),
    )
}
