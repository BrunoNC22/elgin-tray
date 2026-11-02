//! O QR code do login, como imagem: um pixel por módulo, com a margem branca que os leitores pedem.
//! O Slint amplia sem suavizar (`image-rendering: pixelated`).

use qrcode::{Color, QrCode};

/// Módulos de margem branca em volta do código (o padrão pede 4).
pub const QUIET_ZONE: usize = 4;

/// Imagem quadrada em tons de cinza (0 = preto, 255 = branco), linha por linha.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QrImage {
    pub size: u32,
    pub luma: Vec<u8>,
}

impl QrImage {
    /// `None` se o texto não cabe num QR code.
    pub fn of(content: &str) -> Option<Self> {
        let code = QrCode::new(content.as_bytes()).ok()?;
        let width = code.width();
        let size = width + 2 * QUIET_ZONE;
        let mut luma = vec![u8::MAX; size * size];
        for (index, colour) in code.to_colors().into_iter().enumerate() {
            if colour == Color::Dark {
                let (row, column) = (index / width + QUIET_ZONE, index % width + QUIET_ZONE);
                luma[row * size + column] = 0;
            }
        }
        Some(Self {
            size: u32::try_from(size).ok()?,
            luma,
        })
    }

    /// Os mesmos pixels em RGBA.
    pub fn rgba(&self) -> Vec<u8> {
        self.luma
            .iter()
            .flat_map(|&value| [value, value, value, u8::MAX])
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(image: &QrImage, row: usize, column: usize) -> u8 {
        image.luma[row * image.size as usize + column]
    }

    #[test]
    fn has_a_white_quiet_zone_around_the_code() {
        let image = QrImage::of("tuyaSmart--qrLogin?token=abc").unwrap();
        let size = image.size as usize;

        // Um QR code tem 17 + 4 × versão módulos de lado, mais a margem dos dois lados.
        let modules = size - 2 * QUIET_ZONE;
        assert!(
            modules > 17 && (modules - 17).is_multiple_of(4),
            "{modules}"
        );
        assert_eq!(image.luma.len(), size * size);
        for index in 0..size {
            for edge in [0, QUIET_ZONE - 1, size - QUIET_ZONE, size - 1] {
                assert_eq!(at(&image, edge, index), u8::MAX);
                assert_eq!(at(&image, index, edge), u8::MAX);
            }
        }
    }

    #[test]
    fn draws_the_finder_pattern_in_the_corner() {
        let image = QrImage::of("abc").unwrap();
        let q = QUIET_ZONE;

        // Quadrado de 7×7: borda preta, anel branco, miolo 3×3 preto.
        assert_eq!(at(&image, q, q), 0);
        assert_eq!(at(&image, q, q + 6), 0);
        assert_eq!(at(&image, q + 1, q + 1), u8::MAX);
        assert_eq!(at(&image, q + 3, q + 3), 0);
    }

    #[test]
    fn rgba_repeats_the_grey_and_is_opaque() {
        let image = QrImage::of("abc").unwrap();
        let rgba = image.rgba();

        assert_eq!(rgba.len(), image.luma.len() * 4);
        assert_eq!(&rgba[..4], &[255, 255, 255, 255]);
    }
}
