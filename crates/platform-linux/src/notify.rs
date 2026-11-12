//! Notificações pelo `org.freedesktop.Notifications` (o Plasma mostra como notificação comum).

use std::collections::HashMap;

use elgin_application::ports::Notifier;
use zbus::zvariant::Value;

const APP_NAME: &str = "elgin-tray";
/// Nome do ícone e do `.desktop` instalados pelo pacote (o Plasma usa os dois para mostrar de onde
/// veio a notificação).
const APP_ID: &str = "elgin-tray";

/// Argumentos do método `Notify`: app, id a substituir, ícone, título, corpo, ações, dicas e
/// validade (-1: o servidor decide).
type NotifyArgs<'a> = (
    &'a str,
    u32,
    &'a str,
    &'a str,
    &'a str,
    Vec<&'a str>,
    HashMap<&'a str, Value<'a>>,
    i32,
);

fn notify_args<'a>(title: &'a str, body: &'a str) -> NotifyArgs<'a> {
    let hints = HashMap::from([("desktop-entry", Value::from(APP_ID))]);
    (APP_NAME, 0, APP_ID, title, body, Vec::new(), hints, -1)
}

/// O corpo aceita uma marcação simples (`<b>`, `&amp;`...): escapa para o texto sair como está.
fn escape_markup(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Notificações do sistema. Sem barramento da sessão, só vão para o log.
#[derive(Debug, Clone)]
pub struct DesktopNotifier {
    connection: Option<zbus::Connection>,
}

impl DesktopNotifier {
    /// Conecta no barramento da sessão.
    pub async fn connect() -> Self {
        let connection = zbus::Connection::session()
            .await
            .inspect_err(|error| log::warn!("notificações só no log (sem DBus: {error})"))
            .ok();
        Self { connection }
    }
}

impl Notifier for DesktopNotifier {
    /// Não espera o servidor de notificações: a chamada vai numa tarefa do tokio (precisa rodar
    /// dentro de um runtime).
    fn notify(&mut self, title: &str, body: &str) {
        log::info!("notificação: {title} — {body}");
        let Some(connection) = self.connection.clone() else {
            return;
        };
        let (title, body) = (title.to_owned(), escape_markup(body));
        tokio::spawn(async move {
            let sent = connection
                .call_method(
                    Some("org.freedesktop.Notifications"),
                    "/org/freedesktop/Notifications",
                    Some("org.freedesktop.Notifications"),
                    "Notify",
                    &notify_args(&title, &body),
                )
                .await;
            if let Err(error) = sent {
                log::warn!("não foi possível mostrar a notificação \"{title}\": {error}");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_notification_names_the_app_and_its_desktop_entry() {
        let (app, replaces, icon, title, body, actions, hints, expires) =
            notify_args("Atalhos não registrados", "Ctrl+Alt+L");

        assert_eq!((app, replaces, icon), ("elgin-tray", 0, "elgin-tray"));
        assert_eq!((title, body), ("Atalhos não registrados", "Ctrl+Alt+L"));
        assert!(actions.is_empty());
        assert_eq!(hints.get("desktop-entry"), Some(&Value::from("elgin-tray")));
        assert_eq!(expires, -1);
    }

    #[test]
    fn markup_characters_are_shown_as_text() {
        assert_eq!(
            escape_markup("Liga & desliga <Ctrl+Alt+L> > 1"),
            "Liga &amp; desliga &lt;Ctrl+Alt+L&gt; &gt; 1"
        );
        assert_eq!(escape_markup("sem nada especial"), "sem nada especial");
    }
}
