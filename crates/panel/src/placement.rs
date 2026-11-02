//! Onde abrir o painel: colado no ícone da bandeja, do lado da barra de tarefas, dentro da tela.
//! Tudo em pixels físicos.

/// Retângulo na tela.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    fn right(self) -> i32 {
        self.x + self.width
    }

    fn bottom(self) -> i32 {
        self.y + self.height
    }
}

/// Em que borda da tela está a barra de tarefas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Bottom,
    Top,
    Left,
    Right,
}

/// Distância entre o painel e a barra/borda.
pub const MARGIN: i32 = 12;

/// A borda da barra de tarefas: o lado em que a área de trabalho é menor que o monitor. Com a barra
/// escondida automaticamente (área = monitor), supõe embaixo (o padrão do Windows).
pub fn taskbar_edge(monitor: Rect, work_area: Rect) -> Edge {
    if work_area.bottom() < monitor.bottom() {
        Edge::Bottom
    } else if work_area.y > monitor.y {
        Edge::Top
    } else if work_area.x > monitor.x {
        Edge::Left
    } else if work_area.right() < monitor.right() {
        Edge::Right
    } else {
        Edge::Bottom
    }
}

/// Canto superior esquerdo do painel de tamanho `size` (largura, altura).
///
/// Com o ícone (`anchor`), o painel fica centralizado nele, encostado na barra. Sem ícone (ele está na
/// área de ícones ocultos, ou o painel foi aberto por outro caminho), vai para o canto do relógio.
pub fn panel_position(
    anchor: Option<Rect>,
    size: (i32, i32),
    work_area: Rect,
    edge: Edge,
) -> (i32, i32) {
    let (width, height) = size;
    let near_right = work_area.right() - width - MARGIN;
    let near_bottom = work_area.bottom() - height - MARGIN;
    let near_left = work_area.x + MARGIN;
    let near_top = work_area.y + MARGIN;
    let (x, y) = match (anchor, edge) {
        (Some(icon), Edge::Bottom) => (icon.x + icon.width / 2 - width / 2, near_bottom),
        (Some(icon), Edge::Top) => (icon.x + icon.width / 2 - width / 2, near_top),
        (Some(icon), Edge::Left) => (near_left, icon.y + icon.height / 2 - height / 2),
        (Some(icon), Edge::Right) => (near_right, icon.y + icon.height / 2 - height / 2),
        (None, Edge::Bottom | Edge::Right) => (near_right, near_bottom),
        (None, Edge::Top) => (near_right, near_top),
        (None, Edge::Left) => (near_left, near_bottom),
    };
    // Dentro da área de trabalho, com a margem (se o painel for maior que a tela, fica no começo).
    (
        x.min(near_right).max(near_left),
        y.min(near_bottom).max(near_top),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const MONITOR: Rect = Rect {
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
    };
    const SIZE: (i32, i32) = (320, 450);

    fn work(edge: Edge) -> Rect {
        match edge {
            Edge::Bottom => Rect {
                x: 0,
                y: 0,
                width: 1920,
                height: 1040,
            },
            Edge::Top => Rect {
                x: 0,
                y: 40,
                width: 1920,
                height: 1040,
            },
            Edge::Left => Rect {
                x: 60,
                y: 0,
                width: 1860,
                height: 1080,
            },
            Edge::Right => Rect {
                x: 0,
                y: 0,
                width: 1860,
                height: 1080,
            },
        }
    }

    #[test]
    fn finds_the_taskbar_edge() {
        for edge in [Edge::Bottom, Edge::Top, Edge::Left, Edge::Right] {
            assert_eq!(taskbar_edge(MONITOR, work(edge)), edge);
        }
        assert_eq!(
            taskbar_edge(MONITOR, MONITOR),
            Edge::Bottom,
            "barra escondida"
        );
    }

    #[test]
    fn above_a_bottom_taskbar_centred_on_the_icon() {
        let icon = Rect {
            x: 1500,
            y: 1048,
            width: 24,
            height: 24,
        };

        assert_eq!(
            panel_position(Some(icon), SIZE, work(Edge::Bottom), Edge::Bottom),
            (1512 - 160, 1040 - 450 - MARGIN)
        );
    }

    #[test]
    fn below_a_top_taskbar() {
        let icon = Rect {
            x: 1500,
            y: 8,
            width: 24,
            height: 24,
        };

        assert_eq!(
            panel_position(Some(icon), SIZE, work(Edge::Top), Edge::Top),
            (1352, 40 + MARGIN)
        );
    }

    #[test]
    fn beside_side_taskbars_centred_vertically_on_the_icon() {
        let left_icon = Rect {
            x: 18,
            y: 900,
            width: 24,
            height: 24,
        };
        let right_icon = Rect {
            x: 1878,
            y: 900,
            width: 24,
            height: 24,
        };

        assert_eq!(
            panel_position(Some(left_icon), SIZE, work(Edge::Left), Edge::Left),
            (60 + MARGIN, 1080 - 450 - MARGIN),
            "centrado no ícone passaria da borda de baixo: fica dentro da tela"
        );
        assert_eq!(
            panel_position(Some(right_icon), SIZE, work(Edge::Right), Edge::Right),
            (1860 - 320 - MARGIN, 1080 - 450 - MARGIN)
        );
        let high_icon = Rect {
            x: 18,
            y: 300,
            width: 24,
            height: 24,
        };
        assert_eq!(
            panel_position(Some(high_icon), SIZE, work(Edge::Left), Edge::Left),
            (72, 312 - 225)
        );
    }

    #[test]
    fn stays_inside_the_screen_near_the_corner() {
        let corner_icon = Rect {
            x: 1890,
            y: 1048,
            width: 24,
            height: 24,
        };

        assert_eq!(
            panel_position(Some(corner_icon), SIZE, work(Edge::Bottom), Edge::Bottom),
            (1920 - 320 - MARGIN, 1040 - 450 - MARGIN)
        );
    }

    #[test]
    fn without_the_icon_goes_to_the_clock_corner() {
        assert_eq!(
            panel_position(None, SIZE, work(Edge::Bottom), Edge::Bottom),
            (1920 - 320 - MARGIN, 1040 - 450 - MARGIN)
        );
        assert_eq!(
            panel_position(None, SIZE, work(Edge::Top), Edge::Top),
            (1920 - 320 - MARGIN, 40 + MARGIN)
        );
        assert_eq!(
            panel_position(None, SIZE, work(Edge::Left), Edge::Left),
            (60 + MARGIN, 1080 - 450 - MARGIN)
        );
    }

    #[test]
    fn works_on_a_second_monitor_with_scaling() {
        // Monitor à esquerda, com escala 150% (coordenadas negativas, área de trabalho maior).
        let second = Rect {
            x: -2880,
            y: 0,
            width: 2880,
            height: 1560,
        };
        let icon = Rect {
            x: -400,
            y: 1572,
            width: 36,
            height: 36,
        };

        assert_eq!(
            panel_position(Some(icon), (480, 675), second, Edge::Bottom),
            (-382 - 240, 1560 - 675 - MARGIN)
        );
    }
}
