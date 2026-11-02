//! Painel (Slint) aberto pela bandeja e o seu view-model.

pub mod disc;
pub mod disc_image;
#[cfg(windows)]
mod monitor;
pub mod placement;
pub mod qr;
pub mod runner;
pub mod setup_view;
pub mod view_model;

/// Código gerado pelo Slint a partir de `ui/panel.slint`.
#[allow(unreachable_pub, clippy::all)]
mod ui {
    slint::include_modules!();
}

pub use ui::*;

/// A imagem do disco no formato do Slint.
pub fn disc_slint_image(size: u32) -> slint::Image {
    let image = disc_image::render_disc(size);
    let buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
        &image.rgba,
        image.size,
        image.size,
    );
    slint::Image::from_rgba8(buffer)
}

/// O QR code do login no formato do Slint (`None` se o texto não cabe num QR code).
pub fn qr_slint_image(content: &str) -> Option<slint::Image> {
    let image = qr::QrImage::of(content)?;
    let buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
        &image.rgba(),
        image.size,
        image.size,
    );
    Some(slint::Image::from_rgba8(buffer))
}
