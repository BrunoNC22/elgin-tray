/// Registra (ou remove) o app para iniciar junto com o sistema.
pub trait AutostartRegistrar {
    /// Liga ou desliga. O erro é uma descrição para o log.
    fn set_enabled(&mut self, enabled: bool) -> Result<(), String>;
}
