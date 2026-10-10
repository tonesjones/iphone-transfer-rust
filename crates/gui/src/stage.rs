//! The animated backup scene: iCloud, the saved library, and Photo Backups as three nodes.
//! Photos fly along the path of whichever step is copying, and the button below doubles as a
//! progress bar.

use crate::theme::*;
use crate::{App, STEPS, Status};
use eframe::egui::{
    self, Align2, Color32, CursorIcon, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2, emath, epaint,
};
use std::f32::consts::PI;

const STAGE_HEIGHT: f32 = 300.0;
const PLATE: f32 = 42.0;
const FLYERS: usize = 4;
const NODE_NAMES: [&str; 3] = ["iCloud Photos", "Saved library", "Photo Backups"];
const NODE_COLORS: [Color32; 3] = [CYAN, VIOLET, AMBER];

/// Points on the arc between two nodes. Photos and the dotted path share it.
fn bezier(from: Pos2, to: Pos2, t: f32) -> Pos2 {
    let from = from + Vec2::new(PLATE + 10.0, 0.0);
    let to = to - Vec2::new(PLATE + 10.0, 0.0);
    let control = from.lerp(to, 0.5) - Vec2::new(0.0, 46.0);
    let a = from.lerp(control, t);
    let b = control.lerp(to, t);
    a.lerp(b, t)
}

fn smoothstep(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

impl App {
    pub(crate) fn stage(&mut self, ui: &mut egui::Ui) {
        let width = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(Vec2::new(width, STAGE_HEIGHT), Sense::hover());
        let painter = ui.painter_at(rect.expand(2.0));
        let time = ui.input(|i| i.time) as f32;
        painter.rect(rect, 16, CARD, Stroke::new(1.0, LINE), StrokeKind::Inside);

        let y = rect.top() + 80.0;
        let nodes = [
            Pos2::new(rect.left() + width / 6.0, y),
            Pos2::new(rect.center().x, y),
            Pos2::new(rect.right() - width / 6.0, y),
        ];
        let working = self.steps.iter().position(|s| matches!(s, Status::Working));

        // Step 0 copies iCloud -> library and step 2 copies library -> Photo Backups.
        let mut arrival = 0.0_f32;
        for (path, step) in [(0, 0), (1, 2)] {
            let (from, to) = (nodes[path], nodes[path + 1]);
            let active = working == Some(step);
            let done = matches!(self.steps[step], Status::Done(..));
            for i in 0..=24 {
                let t = i as f32 / 24.0;
                let color = if active {
                    let wave = (t - time * 0.9).rem_euclid(1.0);
                    CYAN.gamma_multiply(0.25 + 0.75 * wave.powi(3))
                } else if done {
                    SUCCESS.gamma_multiply(0.45)
                } else {
                    LINE
                };
                painter.circle_filled(bezier(from, to, t), 1.8, color);
            }
            if active {
                for k in 0..FLYERS {
                    let t = (time * 0.45 + k as f32 / FLYERS as f32).rem_euclid(1.0);
                    let eased = smoothstep(t);
                    let alpha = (t.min(1.0 - t) * 7.0).min(1.0);
                    let scale = 0.8 + 0.3 * (PI * t).sin();
                    photo_card(
                        &painter,
                        bezier(from, to, eased),
                        scale,
                        -0.4 + 0.8 * eased,
                        alpha,
                        NODE_COLORS[path],
                    );
                    if t > 0.9 {
                        arrival = arrival.max(((t - 0.9) / 0.1 * PI).sin());
                    }
                }
            }
        }

        for (index, &center) in nodes.iter().enumerate() {
            let status = &self.steps[index];
            let active = working == Some(index);
            // The node photos are flying into.
            let receiving = matches!((index, working), (1, Some(0)) | (2, Some(2)));
            let bump = if receiving { arrival } else { 0.0 };
            plate(
                &painter,
                center,
                index,
                status,
                active || receiving,
                bump,
                time,
            );
            self.node_label(&painter, center, index, width / 3.0 - 20.0);
        }

        self.button(ui, &painter, rect);
    }

    fn node_label(&self, painter: &egui::Painter, center: Pos2, index: usize, wrap: f32) {
        let top = center.y + PLATE + 16.0;
        painter.text(
            Pos2::new(center.x, top),
            Align2::CENTER_TOP,
            NODE_NAMES[index],
            semibold(14.5),
            TEXT,
        );
        let (detail, color) = match &self.steps[index] {
            Status::Done(text, _) => (text.clone(), MUTED),
            Status::Failed => ("Stopped here. See the details below.".into(), ERROR),
            Status::Working => (format!("{}…", STEPS[index].working), CYAN),
            Status::Waiting => (STEPS[index].hint.into(), FAINT),
        };
        let mut job =
            egui::text::LayoutJob::simple(detail, egui::FontId::proportional(12.5), color, wrap);
        job.halign = egui::Align::Center;
        let galley = painter.layout_job(job);
        painter.galley(Pos2::new(center.x, top + 22.0), galley, color);
    }

    fn button(&mut self, ui: &mut egui::Ui, painter: &egui::Painter, stage: Rect) {
        let size = Vec2::new(stage.width().min(320.0) - 40.0, 50.0);
        let rect = Rect::from_center_size(Pos2::new(stage.center().x, stage.bottom() - 46.0), size);
        let response = ui.interact(rect, ui.id().with("backup"), Sense::click());
        let hover = ui
            .ctx()
            .animate_bool(response.id, response.hovered() && !self.running);
        if !self.running {
            response.clone().on_hover_cursor(CursorIcon::PointingHand);
        }
        let radius = (size.y / 2.0) as u8;
        let center = rect.center();

        if self.running {
            painter.rect(
                rect,
                radius,
                BACKGROUND,
                Stroke::new(1.0, LINE),
                StrokeKind::Inside,
            );
            let fill = rect.width() * self.shown_progress;
            // Clip a full-width pill so the bar keeps its rounded left end at any width.
            let filled = Rect::from_min_size(rect.min, Vec2::new(fill, size.y));
            gradient_pill(&painter.with_clip_rect(filled), rect, CYAN, VIOLET);
            let percent = (self.shown_progress * 100.0).floor() as u32;
            painter.text(
                center,
                Align2::CENTER_CENTER,
                format!("{percent}%"),
                bold(19.0),
                TEXT,
            );
        } else {
            let (accent, label) = if self.error.is_some() {
                (Some(ERROR), "Try again")
            } else if self.finished {
                (Some(SUCCESS), "Back up again")
            } else {
                (None, "Back Up Now")
            };
            let shadow = epaint::Shadow {
                offset: [0, 6],
                blur: (18.0 + 14.0 * hover) as u8,
                spread: 0,
                color: accent.unwrap_or(VIOLET).gamma_multiply(0.35 + 0.3 * hover),
            };
            painter.add(shadow.as_shape(rect, radius));
            match accent {
                Some(accent) => {
                    let fill = CARD.lerp_to_gamma(accent, 0.12 + 0.1 * hover);
                    painter.rect(
                        rect,
                        radius,
                        fill,
                        Stroke::new(1.5, accent),
                        StrokeKind::Inside,
                    );
                }
                None => {
                    gradient_pill(
                        painter,
                        rect,
                        CYAN.lerp_to_gamma(Color32::WHITE, 0.12 * hover),
                        VIOLET.lerp_to_gamma(Color32::WHITE, 0.12 * hover),
                    );
                }
            }
            painter.text(
                center,
                Align2::CENTER_CENTER,
                label,
                bold(17.0),
                Color32::WHITE,
            );
        }
        if response.clicked() && !self.running {
            self.start();
        }
    }
}

/// A node's round plate, its icon, and a status badge.
fn plate(
    painter: &egui::Painter,
    center: Pos2,
    index: usize,
    status: &Status,
    lit: bool,
    bump: f32,
    time: f32,
) {
    let color = NODE_COLORS[index];
    let radius = PLATE * (1.0 + 0.06 * bump);
    if lit {
        glow(painter, center, radius, 16.0, color, 0.7 + 0.5 * bump);
    }
    painter.circle_filled(center, radius, BACKGROUND);
    let ring = if lit { color } else { LINE };
    painter.circle_stroke(center, radius, Stroke::new(1.5, ring));

    let icon = Rect::from_center_size(center, Vec2::new(40.0, 32.0) * (1.0 + 0.08 * bump));
    match index {
        0 => cloud_icon(painter, icon, color),
        1 => folder_icon(painter, icon, color),
        _ => {
            folder_icon(
                painter,
                icon.translate(Vec2::new(5.0, -5.0)),
                color.gamma_multiply(0.45),
            );
            folder_icon(painter, icon.translate(Vec2::new(-2.0, 2.0)), color);
        }
    }
    if matches!(status, Status::Working) && index == 1 {
        // Checking the library: a scan line sweeps the folder.
        let sweep = (time * 2.4).sin() * 0.5 + 0.5;
        let y = icon.top() + 4.0 + (icon.height() - 4.0) * sweep;
        painter.line_segment(
            [
                Pos2::new(icon.left() - 6.0, y),
                Pos2::new(icon.right() + 6.0, y),
            ],
            Stroke::new(2.0, Color32::WHITE.gamma_multiply(0.85)),
        );
        painter.line_segment(
            [
                Pos2::new(icon.left() - 6.0, y),
                Pos2::new(icon.right() + 6.0, y),
            ],
            Stroke::new(6.0, CYAN.gamma_multiply(0.25)),
        );
    }

    let badge = center + Vec2::angled(-PI / 4.0) * PLATE;
    match status {
        Status::Done(..) => {
            painter.circle_filled(badge, 11.0, SUCCESS);
            painter.circle_stroke(badge, 11.0, Stroke::new(2.5, CARD));
            check_mark(painter, badge, 11.0, Stroke::new(2.2, CARD));
        }
        Status::Failed => {
            painter.circle_filled(badge, 11.0, ERROR);
            painter.circle_stroke(badge, 11.0, Stroke::new(2.5, CARD));
            painter.text(badge, Align2::CENTER_CENTER, "!", bold(14.0), CARD);
        }
        _ => {}
    }
}

/// A tiny photo print: white border, tinted image, mountain and sun. Rotated by `angle`.
fn photo_card(
    painter: &egui::Painter,
    center: Pos2,
    scale: f32,
    angle: f32,
    alpha: f32,
    tint: Color32,
) {
    let rotation = emath::Rot2::from_angle(angle);
    let quad = |half: Vec2| -> Vec<Pos2> {
        [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
            .map(|(x, y)| center + rotation * Vec2::new(x * half.x, y * half.y))
            .to_vec()
    };
    let at = |x: f32, y: f32| center + rotation * (Vec2::new(x, y) * scale);
    let half = Vec2::new(10.0, 12.5) * scale;
    painter.add(egui::Shape::convex_polygon(
        quad(half),
        Color32::WHITE.gamma_multiply(alpha),
        Stroke::NONE,
    ));
    let image = quad(half - Vec2::splat(2.2 * scale));
    painter.add(egui::Shape::convex_polygon(
        image,
        tint.lerp_to_gamma(VIOLET, 0.35).gamma_multiply(alpha),
        Stroke::NONE,
    ));
    painter.add(egui::Shape::convex_polygon(
        vec![at(-7.8, 10.3), at(-1.5, 1.0), at(4.5, 10.3)],
        Color32::WHITE.gamma_multiply(0.8 * alpha),
        Stroke::NONE,
    ));
    painter.circle_filled(
        at(3.5, -4.5),
        2.4 * scale,
        Color32::WHITE.gamma_multiply(0.9 * alpha),
    );
}
