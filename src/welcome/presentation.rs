//! The welcome screen drawn with egui: the logo, then the ways to open images.

use egui::load::SizedTexture;
use egui::{
    Align, Align2, Color32, ColorImage, CornerRadius, Frame, Layout, Margin, RichText, Stroke,
    TextureHandle, TextureOptions, vec2,
};

/// The app icon, rendered from `assets/icon.svg` at twice `LOGO_SIZE` so it stays sharp at 200 %.
const LOGO_PNG: &[u8] = include_bytes!("../../assets/logo.png");
const LOGO_SIZE: f32 = 96.0;
const TITLE_SIZE: f32 = 30.0;
const TEXT_SIZE: f32 = 18.0;

/// The keys the welcome screen names, in the keymap in use. Only what a first run needs; the
/// help key lists the rest.
pub struct OpeningKeys {
    pub file: &'static str,
    pub folder: &'static str,
    pub help: &'static str,
}

/// Uploads the logo once, at startup; egui frees it when the handle drops.
pub fn load_logo(ctx: &egui::Context) -> TextureHandle {
    let logo = image::load_from_memory_with_format(LOGO_PNG, image::ImageFormat::Png)
        .expect("assets/logo.png is a valid PNG")
        .to_rgba8();
    let size = [logo.width() as usize, logo.height() as usize];
    let pixels = ColorImage::from_rgba_unmultiplied(size, logo.as_raw());
    ctx.load_texture("welcome logo", pixels, TextureOptions::LINEAR)
}

/// Centred on the black window. It takes no input, so the keys it names work as they read, and
/// notices draw over it.
pub fn show(ctx: &egui::Context, logo: &TextureHandle, keys: &OpeningKeys) {
    // Keys first, as on the help overlay.
    let openings = [
        (keys.file, "Open an image and the rest of its folder"),
        (keys.folder, "Open a folder and all its subfolders"),
        (keys.help, "List every key"),
    ];
    egui::Area::new(egui::Id::new("welcome"))
        .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
        .order(egui::Order::Background)
        .interactable(false)
        .fade_in(false)
        .show(ctx, |ui| {
            // Dark whatever the system theme, to match the notices and the help overlay.
            *ui.visuals_mut() = egui::Visuals::dark();
            ui.vertical_centered(|ui| {
                let size = vec2(LOGO_SIZE, LOGO_SIZE);
                ui.add(egui::Image::from_texture(SizedTexture::new(
                    logo.id(),
                    size,
                )));
                ui.add_space(8.0);
                ui.label(
                    RichText::new("SuperSlide-RS")
                        .size(TITLE_SIZE)
                        .strong()
                        .color(Color32::WHITE),
                );
                ui.add_space(TEXT_SIZE * 1.5);
                egui::Grid::new("welcome openings")
                    .num_columns(2)
                    .spacing([12.0, 10.0])
                    .show(ui, |ui| {
                        for (keys, action) in openings {
                            // Right-aligned, so every key sits against its action.
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                key_cap(ui, keys)
                            });
                            ui.label(RichText::new(action).size(TEXT_SIZE).color(Color32::WHITE));
                            ui.end_row();
                        }
                    });
                ui.add_space(TEXT_SIZE);
                ui.label(
                    RichText::new("Or drop images or folders onto this window")
                        .size(TEXT_SIZE - 2.0)
                        .color(Color32::GRAY),
                );
            });
        });
}

fn key_cap(ui: &mut egui::Ui, keys: &str) {
    Frame::new()
        .fill(Color32::from_gray(40))
        .stroke(Stroke::new(1.0, Color32::from_gray(90)))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(Margin::symmetric(8, 2))
        .show(ui, |ui| {
            // Proportional, not monospace: egui's monospace O reads as a zero.
            ui.label(
                RichText::new(keys)
                    .size(TEXT_SIZE - 2.0)
                    .color(Color32::WHITE),
            );
        });
}
