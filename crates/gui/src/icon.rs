//! The app icon: the sidebar logo, rasterized for the window and for icons/photoxfer.ico.

use eframe::egui::{Color32, IconData};

use crate::theme::{CYAN, VIOLET};

/// Coverage of a shape at a pixel, from its signed distance in pixels (negative inside).
fn coverage(distance: f32) -> f32 {
    (0.5 - distance).clamp(0.0, 1.0)
}

fn rounded_square(x: f32, y: f32, half: f32, radius: f32) -> f32 {
    let qx = x.abs() - (half - radius);
    let qy = y.abs() - (half - radius);
    let outside = qx.max(0.0).hypot(qy.max(0.0));
    outside + qx.max(qy).min(0.0) - radius
}

pub fn logo() -> IconData {
    logo_at(256)
}

fn logo_at(size: u32) -> IconData {
    let half = size as f32 / 2.0;
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    for py in 0..size {
        for px in 0..size {
            let (x, y) = (px as f32 + 0.5 - half, py as f32 + 0.5 - half);
            let r = x.hypot(y);
            // Same layout as `theme::logo`, scaled so the square fills the icon.
            let square = coverage(rounded_square(x, y, half * 0.98, half * 0.46));
            let ring = coverage((r - half * 0.46).abs() - half * 0.065);
            let lens = coverage(r - half * 0.17);
            let flash = coverage((x - half * 0.5).hypot(y + half * 0.5) - half * 0.09);
            let white = ring.max(lens).max(flash);
            let t = ((x + y) / (4.0 * half) + 0.5).clamp(0.0, 1.0);
            let base = CYAN.lerp_to_gamma(VIOLET, t);
            let color = base.lerp_to_gamma(Color32::WHITE, white);
            rgba.extend_from_slice(&[color.r(), color.g(), color.b(), (square * 255.0) as u8]);
        }
    }
    IconData {
        rgba,
        width: size,
        height: size,
    }
}

/// The logo as a Windows .ico with the sizes Explorer and the taskbar pick from.
#[cfg(test)]
fn ico() -> Vec<u8> {
    const SIZES: [u32; 6] = [16, 24, 32, 48, 64, 256];
    let images: Vec<Vec<u8>> = SIZES.iter().map(|&size| bitmap(&logo_at(size))).collect();
    let mut out = Vec::new();
    out.extend_from_slice(&[0, 0, 1, 0]);
    out.extend_from_slice(&(SIZES.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * SIZES.len() as u32;
    for (size, image) in SIZES.iter().zip(&images) {
        // A dimension byte of 0 means 256.
        let side = (*size % 256) as u8;
        out.extend_from_slice(&[side, side, 0, 0, 1, 0, 32, 0]);
        out.extend_from_slice(&(image.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += image.len() as u32;
    }
    for image in images {
        out.extend_from_slice(&image);
    }
    out
}

/// A 32-bit bottom-up BGRA bitmap with an empty AND mask, as .ico entries store it.
#[cfg(test)]
fn bitmap(icon: &IconData) -> Vec<u8> {
    let (width, height) = (icon.width, icon.height);
    let mask_row = width.div_ceil(32) * 4;
    let mut out = Vec::new();
    for value in [40, width, height * 2] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&[0; 24]);
    for row in icon.rgba.chunks_exact(width as usize * 4).rev() {
        for &[r, g, b, a] in row.as_chunks::<4>().0 {
            out.extend_from_slice(&[b, g, r, a]);
        }
    }
    out.resize(out.len() + (mask_row * height) as usize, 0);
    out
}

#[cfg(test)]
mod tests {
    /// Set PHOTOXFER_WRITE_ICON=1 to regenerate the file after changing the logo.
    #[test]
    fn checked_in_ico_matches_logo() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/icons/photoxfer.ico");
        let expected = super::ico();
        if std::env::var_os("PHOTOXFER_WRITE_ICON").is_some() {
            std::fs::create_dir_all(std::path::Path::new(path).parent().unwrap()).unwrap();
            std::fs::write(path, &expected).unwrap();
        }
        assert!(std::fs::read(path).unwrap() == expected, "{path} is stale");
    }
}
