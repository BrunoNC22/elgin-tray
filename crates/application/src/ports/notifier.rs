/// Mostra uma notificação do sistema. Melhor esforço: se não der, o adaptador registra no log.
pub trait Notifier {
    fn notify(&mut self, title: &str, body: &str);
}
