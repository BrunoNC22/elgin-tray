//! Janela de demonstração do disco (temporária, para revisar o visual).
use elgin_panel::disc::{DiscPoint, colour_at, point_of};
use elgin_panel::{DiscPreview, disc_slint_image};
use slint::ComponentHandle;

fn main() {
    let window = DiscPreview::new().unwrap();
    window.set_disc(disc_slint_image(440));
    let start = point_of(colour_at(DiscPoint { x: 0.5, y: -0.5 }));
    window.set_marker_x(start.x);
    window.set_marker_y(start.y);
    let weak = window.as_weak();
    window.on_picked(move |x, y| {
        let colour = colour_at(DiscPoint { x, y });
        let point = point_of(colour);
        let window = weak.unwrap();
        window.set_marker_x(point.x);
        window.set_marker_y(point.y);
        println!(
            "matiz {}°, saturação {}‰",
            colour.hue().degrees(),
            colour.saturation().permille()
        );
    });
    window.run().unwrap();
}
