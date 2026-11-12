//! O ícone no formato que o StatusNotifierItem pede.

use elgin_application::tray::IconSpec;
use elgin_icons::Rgba;

/// O ícone em todos os tamanhos de [`elgin_icons::SIZES`] (o Plasma escolhe o melhor para o DPI).
pub(crate) fn pixmaps(spec: &IconSpec) -> Vec<ksni::Icon> {
    elgin_icons::SIZES
        .iter()
        .map(|&size| argb32(&elgin_icons::render(spec, size)))
        .collect()
}

/// RGBA → ARGB32 em ordem de bytes de rede (big-endian), sem alfa pré-multiplicado.
pub(crate) fn argb32(image: &Rgba) -> ksni::Icon {
    let data = image
        .data
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|&[r, g, b, a]| [a, r, g, b])
        .collect();
    ksni::Icon {
        width: image.width as i32,
        height: image.height as i32,
        data,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgba_pixels_become_argb_in_network_order() {
        let image = Rgba {
            width: 2,
            height: 1,
            data: vec![10, 20, 30, 255, 1, 2, 3, 0],
        };

        let icon = argb32(&image);

        assert_eq!((icon.width, icon.height), (2, 1));
        assert_eq!(icon.data, vec![255, 10, 20, 30, 0, 1, 2, 3]);
    }

    #[test]
    fn every_tray_size_is_offered() {
        let icons = pixmaps(&IconSpec::Off);

        let sizes: Vec<i32> = icons.iter().map(|icon| icon.width).collect();
        let expected: Vec<i32> = elgin_icons::SIZES.iter().map(|&size| size as i32).collect();
        assert_eq!(sizes, expected);
        assert!(
            icons
                .iter()
                .all(|icon| icon.data.len() == (icon.width * icon.height * 4) as usize)
        );
    }
}
