//! Telas e barras do Plasma (KDE), para saber onde fica a área de trabalho e de que lado está a
//! barra com o ícone. Tudo em pixels lógicos (os mesmos do KWin e do clique no ícone).

use serde::Deserialize;

use crate::placement::{Edge, Rect};

/// Script do `org.kde.PlasmaShell.evaluateScript`: telas `[x, y, largura, altura]` e barras.
pub(crate) const LAYOUT_QUERY: &str = "print(JSON.stringify({\
    screens: Array.from({length: screenCount}, (_, i) => { const g = screenGeometry(i); \
        return [g.x, g.y, g.width, g.height]; }), \
    panels: panels().map(p => ({location: p.location, thickness: p.height, screen: p.screen}))}))";

#[derive(Debug, Deserialize)]
struct Layout {
    screens: Vec<[i32; 4]>,
    panels: Vec<Bar>,
}

#[derive(Debug, Deserialize)]
struct Bar {
    location: String,
    thickness: i32,
    screen: i32,
}

/// A área de trabalho da tela onde o usuário clicou e a borda da barra mais perto do clique (sem
/// clique: a primeira tela). `None` se a resposta do Plasma não é a esperada.
pub(crate) fn work_area_at(layout_json: &str, click: Option<(i32, i32)>) -> Option<(Rect, Edge)> {
    let layout: Layout = serde_json::from_str(layout_json).ok()?;
    let screens: Vec<Rect> = layout
        .screens
        .iter()
        .map(|&[x, y, width, height]| Rect {
            x,
            y,
            width,
            height,
        })
        .collect();
    let index = click
        .and_then(|point| screens.iter().position(|&screen| contains(screen, point)))
        .unwrap_or(0);
    let screen = *screens.get(index)?;
    let bars: Vec<(Edge, i32)> = layout
        .panels
        .iter()
        .filter(|bar| usize::try_from(bar.screen) == Ok(index))
        .filter_map(|bar| Some((edge_named(&bar.location)?, bar.thickness)))
        .collect();

    let mut work = screen;
    for &(edge, thickness) in &bars {
        match edge {
            Edge::Top => {
                work.y += thickness;
                work.height -= thickness;
            }
            Edge::Bottom => work.height -= thickness,
            Edge::Left => {
                work.x += thickness;
                work.width -= thickness;
            }
            Edge::Right => work.width -= thickness,
        }
    }

    let edges: Vec<Edge> = if bars.is_empty() {
        vec![Edge::Bottom, Edge::Top, Edge::Left, Edge::Right]
    } else {
        bars.iter().map(|&(edge, _)| edge).collect()
    };
    let edge = match click {
        Some(point) => edges
            .into_iter()
            .min_by_key(|&edge| distance_to_edge(screen, edge, point))?,
        None => edges[0],
    };
    Some((work, edge))
}

fn edge_named(location: &str) -> Option<Edge> {
    match location {
        "top" => Some(Edge::Top),
        "bottom" => Some(Edge::Bottom),
        "left" => Some(Edge::Left),
        "right" => Some(Edge::Right),
        // "floating" e outros: a barra não fica encostada numa borda.
        _ => None,
    }
}

fn contains(screen: Rect, (x, y): (i32, i32)) -> bool {
    (screen.x..screen.x + screen.width).contains(&x)
        && (screen.y..screen.y + screen.height).contains(&y)
}

fn distance_to_edge(screen: Rect, edge: Edge, (x, y): (i32, i32)) -> i32 {
    match edge {
        Edge::Top => y - screen.y,
        Edge::Bottom => screen.y + screen.height - y,
        Edge::Left => x - screen.x,
        Edge::Right => screen.x + screen.width - x,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE_SCREEN_TOP_BAR: &str = r#"{"screens":[[0,0,1920,1080]],
        "panels":[{"location":"top","thickness":46,"screen":0}]}"#;

    #[test]
    fn a_top_bar_takes_its_thickness_from_the_top() {
        assert_eq!(
            work_area_at(ONE_SCREEN_TOP_BAR, Some((1380, 20))),
            Some((
                Rect {
                    x: 0,
                    y: 46,
                    width: 1920,
                    height: 1034
                },
                Edge::Top
            ))
        );
    }

    #[test]
    fn without_a_click_the_first_screen_is_used() {
        assert_eq!(
            work_area_at(ONE_SCREEN_TOP_BAR, None).map(|(_, edge)| edge),
            Some(Edge::Top)
        );
    }

    #[test]
    fn with_two_bars_the_edge_is_the_one_closest_to_the_click() {
        let layout = r#"{"screens":[[0,0,1920,1080]], "panels":[
            {"location":"bottom","thickness":44,"screen":0},
            {"location":"left","thickness":60,"screen":0}]}"#;

        let (work, edge) = work_area_at(layout, Some((30, 500))).unwrap();

        assert_eq!(
            work,
            Rect {
                x: 60,
                y: 0,
                width: 1860,
                height: 1036
            }
        );
        assert_eq!(edge, Edge::Left);
        assert_eq!(
            work_area_at(layout, Some((1800, 1060))).unwrap().1,
            Edge::Bottom
        );
    }

    #[test]
    fn the_screen_is_the_one_that_contains_the_click() {
        let layout = r#"{"screens":[[0,0,1920,1080],[1920,0,2560,1440]], "panels":[
            {"location":"bottom","thickness":44,"screen":0},
            {"location":"right","thickness":50,"screen":1}]}"#;

        assert_eq!(
            work_area_at(layout, Some((4450, 700))),
            Some((
                Rect {
                    x: 1920,
                    y: 0,
                    width: 2510,
                    height: 1440
                },
                Edge::Right
            ))
        );
    }

    #[test]
    fn a_screen_without_bars_uses_the_screen_edge_closest_to_the_click() {
        let layout = r#"{"screens":[[0,0,1920,1080]], "panels":[]}"#;

        assert_eq!(
            work_area_at(layout, Some((1900, 540))),
            Some((
                Rect {
                    x: 0,
                    y: 0,
                    width: 1920,
                    height: 1080
                },
                Edge::Right
            ))
        );
    }

    #[test]
    fn floating_bars_and_unknown_answers_are_handled() {
        let floating = r#"{"screens":[[0,0,1920,1080]],
            "panels":[{"location":"floating","thickness":40,"screen":0}]}"#;
        assert_eq!(
            work_area_at(floating, Some((10, 10))).map(|(work, _)| work.height),
            Some(1080),
            "barra solta não ocupa borda"
        );
        assert_eq!(work_area_at("não é json", None), None);
        assert_eq!(work_area_at(r#"{"screens":[],"panels":[]}"#, None), None);
    }
}
