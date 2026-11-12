//! Painel colado na bandeja no KDE (Wayland e X11). No Wayland um app não escolhe a posição da
//! própria janela: antes de a janela aparecer, um script do KWin (carregado pelo DBus) fica
//! esperando por ela para posicioná-la. A conta é a mesma do Windows ([`panel_position`]), com a
//! tela e a barra lidas do Plasma ([`crate::plasma`]). Decisão do spike #26 (CLAUDE.md).

use std::time::Duration;

use slint::winit_030::winit;

use crate::placement::{Rect, panel_position};
use crate::plasma;

/// app_id (Wayland) e WM_CLASS (X11) da janela do painel: é por ele que o script acha a janela.
pub(crate) const PANEL_CLASS: &str = "elgin-tray-panel";
/// Nome do script no KWin (carregar de novo com o mesmo nome substitui o anterior).
const SCRIPT_NAME: &str = "elgin-tray-panel";
/// Tamanho do painel em pixels lógicos (o mesmo do `panel.slint`).
const PANEL_SIZE: (i32, i32) = (320, 490);
/// Quanto esperar pelo Plasma e pelo KWin antes de abrir o painel sem posição.
const DBUS_TIMEOUT: Duration = Duration::from_millis(500);

/// Script do KWin: quando a janela do painel aparecer, coloca em (x, y), tira da barra de tarefas
/// e do alternador, deixa por cima, dá o foco e se desliga (vale para uma abertura só).
pub(crate) fn placement_script(x: i32, y: i32) -> String {
    format!(
        r#"(function () {{
    function place(window) {{
        if (window.resourceClass !== "{PANEL_CLASS}") return;
        workspace.windowAdded.disconnect(place);
        const size = window.frameGeometry;
        window.frameGeometry = {{x: {x}, y: {y}, width: size.width, height: size.height}};
        window.skipTaskbar = true;
        window.skipPager = true;
        window.skipSwitcher = true;
        window.keepAbove = true;
        workspace.activeWindow = window;
    }}
    workspace.windowAdded.connect(place);
}})();
"#
    )
}

/// Antes de criar a janela: dá o nome [`PANEL_CLASS`] a ela e prepara o script que a posiciona
/// colada em `anchor` (ou no canto da barra, sem `anchor`). Sem Plasma/KWin, o painel abre onde
/// o KWin quiser.
pub(crate) fn prepare_window(anchor: Option<Rect>) {
    let named = slint::BackendSelector::new()
        .with_winit_window_attributes_hook(|attributes| {
            use winit::platform::wayland::WindowAttributesExtWayland;
            use winit::platform::x11::WindowAttributesExtX11;
            let attributes =
                WindowAttributesExtWayland::with_name(attributes, PANEL_CLASS, PANEL_CLASS);
            WindowAttributesExtX11::with_name(attributes, PANEL_CLASS, PANEL_CLASS)
        })
        .select();
    if let Err(error) = named {
        eprintln!("painel: não foi possível escolher o backend da janela: {error}");
        return;
    }
    let placed = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())
        .and_then(|runtime| {
            runtime.block_on(async {
                tokio::time::timeout(DBUS_TIMEOUT, load_placement(anchor))
                    .await
                    .unwrap_or_else(|_| Err("o Plasma/KWin não respondeu a tempo".to_owned()))
            })
        });
    if let Err(error) = placed {
        eprintln!("painel: abrindo sem posição ({error})");
    }
}

async fn load_placement(anchor: Option<Rect>) -> Result<(), String> {
    let dbus = |error: zbus::Error| error.to_string();
    let connection = zbus::Connection::session().await.map_err(dbus)?;
    let layout: String = connection
        .call_method(
            Some("org.kde.plasmashell"),
            "/PlasmaShell",
            Some("org.kde.PlasmaShell"),
            "evaluateScript",
            &(plasma::LAYOUT_QUERY,),
        )
        .await
        .map_err(dbus)?
        .body()
        .deserialize()
        .map_err(dbus)?;
    let click = anchor.map(|icon| (icon.x + icon.width / 2, icon.y + icon.height / 2));
    let (work_area, edge) = plasma::work_area_at(&layout, click)
        .ok_or_else(|| format!("resposta inesperada do Plasma: {layout}"))?;
    let (x, y) = panel_position(anchor, PANEL_SIZE, work_area, edge);

    let path = std::env::var_os("XDG_RUNTIME_DIR")
        .map_or_else(std::env::temp_dir, std::path::PathBuf::from)
        .join(format!("{SCRIPT_NAME}.js"));
    std::fs::write(&path, placement_script(x, y)).map_err(|error| error.to_string())?;
    let path = path.to_str().ok_or("caminho do script inválido")?;

    // Um script de uma abertura anterior que não chegou a ver a janela é substituído.
    let _ = connection
        .call_method(
            Some("org.kde.KWin"),
            "/Scripting",
            Some("org.kde.kwin.Scripting"),
            "unloadScript",
            &(SCRIPT_NAME,),
        )
        .await;
    connection
        .call_method(
            Some("org.kde.KWin"),
            "/Scripting",
            Some("org.kde.kwin.Scripting"),
            "loadScript",
            &(path, SCRIPT_NAME),
        )
        .await
        .map_err(dbus)?;
    // `start` roda os scripts carregados que ainda não rodaram. O `run` em `/Scripting/Script<id>`
    // não serve: o KWin numera pela posição na lista, e o id do nosso script recarregado pode
    // apontar para o caminho de outro script carregado depois dele.
    connection
        .call_method(
            Some("org.kde.KWin"),
            "/Scripting",
            Some("org.kde.kwin.Scripting"),
            "start",
            &(),
        )
        .await
        .map_err(dbus)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_script_places_only_the_panel_window_once() {
        let script = placement_script(1220, 58);

        assert!(script.contains(r#"resourceClass !== "elgin-tray-panel""#));
        assert!(script.contains("x: 1220, y: 58"));
        assert!(script.contains("skipTaskbar = true"));
        assert!(script.contains("keepAbove = true"));
        assert!(script.contains("workspace.activeWindow = window"));
        assert!(
            script.find("windowAdded.disconnect").unwrap()
                < script.find("frameGeometry =").unwrap(),
            "desliga antes de mexer na janela"
        );
    }
}
