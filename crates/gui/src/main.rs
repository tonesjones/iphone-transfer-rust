#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod backup;
mod theme;

use anyhow::Context;
use backup::{Event, Folders};
use clap::Parser;
use eframe::egui::{self, Align2, Color32, CursorIcon, RichText, Sense, Stroke, Vec2};
use std::{path::PathBuf, sync::mpsc};
use theme::*;

const SIDEBAR_WIDTH: f32 = 300.0;
const ORB_RADIUS: f32 = 78.0;

struct Step {
    title: &'static str,
    hint: &'static str,
    working: &'static str,
}

const STEPS: [Step; 3] = [
    Step {
        title: "Save from iCloud",
        hint: "Copy new or missing photos and videos.",
        working: "Saving from iCloud",
    },
    Step {
        title: "Check saved library",
        hint: "Verify the contents of every saved file.",
        working: "Checking library",
    },
    Step {
        title: "Check Photo Backups",
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

    fn orb(&mut self, ui: &mut egui::Ui) {
        let size = Vec2::splat(ORB_RADIUS * 2.0 + 48.0);
        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
        let response = if self.running {
            response
        } else {
            response.on_hover_cursor(CursorIcon::PointingHand)
        };
        let hover = ui
            .ctx()
            .animate_bool(response.id, response.hovered() && !self.running);
        let painter = ui.painter_at(rect);
        let center = rect.center();
        let time = ui.input(|i| i.time) as f32;
        let r = ORB_RADIUS;
        let label = |painter: &egui::Painter, text: &str, dy: f32, font, color| {
            painter.text(
                center + Vec2::new(0.0, dy),
                Align2::CENTER_CENTER,
                text,
                font,
                color,
            );
        };

        if self.running {
            glow(
                &painter,
                center,
                r,
                22.0,
                VIOLET,
                0.5 + 0.2 * (time * 2.0).sin(),
            );
            painter.circle_filled(center, r, CARD);
            painter.circle_stroke(center, r - 6.0, Stroke::new(5.0, LINE));
            comet_arc(
                &painter,
                center,
                r - 6.0,
                time * 3.6,
                2.2,
                Stroke::new(5.0, CYAN),
            );
            let current = self
                .steps
                .iter()
                .position(|s| matches!(s, Status::Working))
                .unwrap_or(0);
            label(
                &painter,
                &format!("{} of 3", current + 1),
                -10.0,
                bold(24.0),
                TEXT,
            );
            label(
                &painter,
                STEPS[current].working,
                18.0,
                egui::FontId::proportional(12.5),
                MUTED,
            );
        } else if self.error.is_some() {
            glow(
                &painter,
                center,
                r,
                18.0 + 6.0 * hover,
                ERROR,
                0.4 + 0.4 * hover,
            );
            painter.circle_filled(center, r, CARD.lerp_to_gamma(CARD_HOVER, hover));
            painter.circle_stroke(center, r - 6.0, Stroke::new(5.0, ERROR));
            label(&painter, "!", -12.0, bold(40.0), ERROR);
            label(&painter, "Try again", 24.0, semibold(14.0), TEXT);
        } else if self.finished {
            glow(
                &painter,
                center,
                r,
                18.0 + 6.0 * hover,
                SUCCESS,
                0.5 + 0.4 * hover,
            );
            painter.circle_filled(center, r, CARD.lerp_to_gamma(CARD_HOVER, hover));
            painter.circle_stroke(center, r - 6.0, Stroke::new(5.0, SUCCESS));
            check_mark(
                &painter,
                center + Vec2::new(0.0, -12.0),
                34.0,
                Stroke::new(5.0, SUCCESS),
            );
            label(&painter, "Back up again", 26.0, semibold(14.0), TEXT);
        } else {
            let lift = 1.0 + 0.04 * hover;
            glow(
                &painter,
                center,
                r * lift,
                20.0 + 10.0 * hover,
                VIOLET,
                0.7 + 0.5 * hover,
            );
            let (from, to) = (
                CYAN.lerp_to_gamma(Color32::WHITE, 0.12 * hover),
                VIOLET.lerp_to_gamma(Color32::WHITE, 0.12 * hover),
            );
            gradient_disc(&painter, center, r * lift, from, to);
            painter.circle_stroke(
                center,
                r * lift - 1.0,
                Stroke::new(1.5, Color32::WHITE.gamma_multiply(0.25)),
            );
            label(&painter, "Back Up", -12.0, bold(24.0), Color32::WHITE);
            label(&painter, "Now", 16.0, bold(24.0), Color32::WHITE);
        }
        if response.clicked() && !self.running {
            self.start();
        }
    }

    fn timeline(&self, ui: &mut egui::Ui) {
        let mut nodes = Vec::new();
        for (index, step) in self.steps.iter().enumerate() {
            ui.horizontal(|ui| {
                let (node, _) = ui.allocate_exact_size(Vec2::splat(30.0), Sense::hover());
                nodes.push(node.center());
                self.node(ui, node.center(), index, step);
                ui.add_space(4.0);
                ui.vertical(|ui| {
                    ui.add_space(2.0);
                    let title_color = if matches!(step, Status::Waiting) && self.running {
                        MUTED
                    } else {
                        TEXT
                    };
                    ui.label(
                        RichText::new(STEPS[index].title)
                            .font(semibold(16.0))
                            .color(title_color),
                    );
                    let (detail, color) = match step {
                        Status::Done(text, _) => (text.as_str(), MUTED),
                        Status::Failed => ("Stopped here. See the details below.", ERROR),
                        Status::Working => ("Working…", CYAN),
                        Status::Waiting => (STEPS[index].hint, FAINT),
                    };
                    ui.label(RichText::new(detail).size(13.5).color(color));
                });
            });
            ui.add_space(14.0);
        }
        let painter = ui.painter();
        for (pair, step) in nodes.windows(2).zip(&self.steps) {
            let color = if matches!(step, Status::Done(..)) {
                SUCCESS.gamma_multiply(0.6)
            } else {
                LINE
            };
            painter.line_segment(
                [
                    pair[0] + Vec2::new(0.0, 18.0),
                    pair[1] - Vec2::new(0.0, 18.0),
                ],
                Stroke::new(2.0, color),
            );
        }
    }

    fn node(&self, ui: &egui::Ui, center: egui::Pos2, index: usize, step: &Status) {
        let painter = ui.painter();
        match step {
            Status::Done(..) => {
                painter.circle_filled(center, 14.0, SUCCESS.gamma_multiply(0.18));
                painter.circle_stroke(center, 14.0, Stroke::new(1.5, SUCCESS));
                check_mark(painter, center, 13.0, Stroke::new(2.2, SUCCESS));
            }
            Status::Failed => {
                painter.circle_filled(center, 14.0, ERROR.gamma_multiply(0.18));
                painter.circle_stroke(center, 14.0, Stroke::new(1.5, ERROR));
                painter.text(center, Align2::CENTER_CENTER, "!", bold(16.0), ERROR);
            }
            Status::Working => {
                let time = ui.input(|i| i.time) as f32;
                glow(
                    painter,
                    center,
                    12.0,
                    10.0,
                    CYAN,
                    0.6 + 0.4 * (time * 4.0).sin(),
                );
                painter.circle_filled(center, 14.0, CARD);
                comet_arc(
                    painter,
                    center,
                    12.0,
                    time * 5.0,
                    3.6,
                    Stroke::new(2.5, CYAN),
                );
            }
            Status::Waiting => {
                painter.circle_filled(center, 14.0, CARD);
                painter.circle_stroke(center, 14.0, Stroke::new(1.5, LINE));
                painter.text(
                    center,
                    Align2::CENTER_CENTER,
                    (index + 1).to_string(),
                    semibold(13.0),
                    MUTED,
                );
            }
        }
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
                    ui.horizontal_top(|ui| {
                        self.orb(ui);
                        ui.add_space(18.0);
                        ui.vertical(|ui| {
                            ui.add_space(26.0);
                            self.timeline(ui);
                        });
                    });
                    ui.add_space(10.0);
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
