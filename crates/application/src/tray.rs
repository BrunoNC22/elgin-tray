//! O que a bandeja mostra (o presenter completo chega na #23).

use elgin_domain::Rgb;

/// Como o ícone da bandeja deve ser desenhado.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IconSpec {
    /// Ligada: lâmpada preenchida com a cor que ela está mostrando.
    On(Rgb),
    /// Desligada: só o contorno.
    Off,
    /// Sem controle da lâmpada: cinza com um ponto indicador.
    Unavailable(Indicator),
}

/// O ponto no canto do ícone quando a lâmpada não está disponível.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Indicator {
    /// Conectando, offline ou não configurada (laranja).
    Warning,
    /// Chave recusada ou precisa reconfigurar (vermelho).
    Error,
}
