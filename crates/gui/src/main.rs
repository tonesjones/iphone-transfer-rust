#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod backup;

use backup::{Event, Folders};
use clap::Parser;
use eframe::egui::{self, Color32, RichText};
use std::{path::PathBuf, sync::mpsc, time::Duration};

const INK: Color32 = Color32::from_rgb(32, 47, 63);
const TEAL: Color32 = Color32::from_rgb(19, 109, 99);

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
            ui.strong(label);
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
                .color(Color32::from_gray(100)),
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
        egui::CentralPanel::default().show(root, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add_space(12.0);
                ui.label(RichText::new("PHOTOXFER  /  PERSONAL BACKUP").small().color(TEAL));
                ui.add_space(8.0);
                ui.label(RichText::new("Keep your memories.").size(32.0).strong().color(INK));
                ui.label("Save new photos and videos, then check both copies on your computer.");
                ui.add_space(20.0);
                ui.horizontal(|ui| {
                    let button = egui::Button::new(RichText::new(if self.running { "Backing up…" } else { "Back Up Now" }).size(18.0).color(Color32::WHITE)).fill(TEAL);
                    if ui.add_enabled(!self.running, button).clicked() { self.start(); }
                    ui.add_space(12.0);
                    ui.label(if self.running { "Please keep this window open." } else { "Your phone and iCloud files stay untouched." });
                });
                ui.add_space(18.0);
                let titles = ["Save from iCloud", "Check saved library", "Check Photo Backups"];
                let hints = ["Only new or missing files need copying.", "Check every saved file against its recorded contents.", "Make and verify your second copy in OneDrive’s local folder."];
                for index in 0..3 {
                    egui::Frame::new().fill(Color32::WHITE).inner_margin(16.0).corner_radius(12).show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.horizontal(|ui| {
                            match self.steps[index] {
                                Status::Working => { ui.spinner(); }
                                Status::Done(_) => { ui.colored_label(TEAL, "✓"); }
                                Status::Failed => { ui.colored_label(Color32::DARK_RED, "!"); }
                                Status::Waiting => { ui.label(format!("{}", index + 1)); }
                            }
                            ui.label(RichText::new(titles[index]).size(17.0).strong());
                        });
                        ui.label(match &self.steps[index] { Status::Done(text) => text.as_str(), _ => hints[index] });
                    });
                    ui.add_space(8.0);
                }
                if let Some(error) = &self.error {
                    ui.colored_label(Color32::DARK_RED, RichText::new("Backup needs attention").strong());
                    ui.label("Keep your phone/iCloud copies. Resolve the problem below, then try again.");
                    egui::CollapsingHeader::new("Problem details").default_open(true).show(ui, |ui| { ui.label(error); });
                } else if self.finished {
                    ui.colored_label(TEAL, RichText::new("Both local copies passed their checks.").strong());
                    ui.label("Complete the phone and cloud checks below before deleting anything.");
                }
                ui.add_space(14.0);
                egui::Frame::new().fill(Color32::from_rgb(255, 245, 221)).inner_margin(16.0).corner_radius(12).show(ui, |ui| {
                    ui.strong("Before deleting photos");
                    ui.label("These checks are still unverified:");
                    ui.label("1. Confirm every phone item is included, including both parts of your Live Photo.");
                    ui.label("2. Wait for OneDrive to upload, then download a fresh backup and verify it against its recovery file list.");
                    ui.label(RichText::new("A local backup check does not confirm a cloud upload.").small());
                });
                ui.add_space(16.0);
                egui::CollapsingHeader::new("Your backup folders").default_open(true).show(ui, |ui| {
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
                .with_inner_size([760.0, 860.0])
                .with_min_inner_size([580.0, 620.0]),
            centered: true,
            ..Default::default()
        },
        Box::new(move |cc| {
            cc.egui_ctx.set_theme(egui::Theme::Light);
            cc.egui_ctx.global_style_mut(|style| {
                style.visuals.panel_fill = Color32::from_rgb(241, 245, 247);
                style.spacing.button_padding = egui::vec2(20.0, 12.0);
                style.spacing.item_spacing = egui::vec2(10.0, 6.0);
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
