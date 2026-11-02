//! O processo do painel (`elgin-tray panel`), aberto e fechado pelo daemon.

use std::io;
use std::process::{Child, Command};

/// Onde o ícone da bandeja está na tela (para o painel abrir colado nele).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Anchor {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

/// Abre e fecha o processo do painel.
pub(crate) trait PanelProcess {
    fn is_open(&mut self) -> bool;
    fn open(&mut self, anchor: Option<Anchor>) -> io::Result<()>;
    fn close(&mut self);
}

/// Abre o painel se está fechado; fecha se está aberto (clique no ícone).
pub(crate) fn toggle<P: PanelProcess>(panel: &mut P, anchor: Option<Anchor>) -> io::Result<()> {
    if panel.is_open() {
        panel.close();
        Ok(())
    } else {
        panel.open(anchor)
    }
}

/// Garante o painel aberto (segunda execução do app, configuração inicial).
pub(crate) fn ensure_open<P: PanelProcess>(
    panel: &mut P,
    anchor: Option<Anchor>,
) -> io::Result<()> {
    if panel.is_open() {
        Ok(())
    } else {
        panel.open(anchor)
    }
}

/// O painel de verdade: o próprio executável com o argumento `panel`.
#[derive(Debug, Default)]
pub(crate) struct ChildPanel {
    child: Option<Child>,
}

impl PanelProcess for ChildPanel {
    fn is_open(&mut self) -> bool {
        match &mut self.child {
            Some(child) => matches!(child.try_wait(), Ok(None)),
            None => false,
        }
    }

    fn open(&mut self, anchor: Option<Anchor>) -> io::Result<()> {
        let mut command = Command::new(std::env::current_exe()?);
        command.arg("panel");
        if let Some(anchor) = anchor {
            command.arg("--anchor").arg(format!(
                "{},{},{},{}",
                anchor.x, anchor.y, anchor.width, anchor.height
            ));
        }
        self.child = Some(command.spawn()?);
        Ok(())
    }

    fn close(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Painel falso: registra o que foi pedido.
#[cfg(test)]
#[derive(Debug, Default)]
pub(crate) struct FakePanel {
    pub(crate) open: bool,
    pub(crate) opened_with: Vec<Option<Anchor>>,
    pub(crate) closes: usize,
}

#[cfg(test)]
impl PanelProcess for FakePanel {
    fn is_open(&mut self) -> bool {
        self.open
    }

    fn open(&mut self, anchor: Option<Anchor>) -> io::Result<()> {
        self.open = true;
        self.opened_with.push(anchor);
        Ok(())
    }

    fn close(&mut self) {
        self.open = false;
        self.closes += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ANCHOR: Anchor = Anchor {
        x: 10,
        y: 20,
        width: 24,
        height: 24,
    };

    #[test]
    fn toggle_opens_a_closed_panel_and_closes_an_open_one() {
        let mut panel = FakePanel::default();

        toggle(&mut panel, Some(ANCHOR)).unwrap();
        assert!(panel.open);
        assert_eq!(panel.opened_with, [Some(ANCHOR)]);

        toggle(&mut panel, Some(ANCHOR)).unwrap();
        assert!(!panel.open);
        assert_eq!(panel.closes, 1);
    }

    #[test]
    fn ensure_open_never_closes() {
        let mut panel = FakePanel::default();

        ensure_open(&mut panel, None).unwrap();
        ensure_open(&mut panel, None).unwrap();

        assert!(panel.open);
        assert_eq!(panel.opened_with.len(), 1);
        assert_eq!(panel.closes, 0);
    }
}
