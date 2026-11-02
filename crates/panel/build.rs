//! Compila a interface Slint do painel (`ui/panel.slint`).

fn main() {
    slint_build::compile("ui/panel.slint").expect("interface Slint inválida");
}
