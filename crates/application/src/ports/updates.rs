use std::fmt;

use crate::updates::Version;

/// Um arquivo publicado numa release (ex.: o `.zip` do Windows e a assinatura).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseAsset {
    pub name: String,
    pub url: String,
}

/// Uma versão publicada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    pub assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateError {
    /// Sem conexão ou servidor fora do ar (tentar de novo mais tarde, sem incomodar o usuário).
    Network(String),
    /// A release não serve (sem o arquivo deste sistema, assinatura inválida, etc.).
    Rejected(String),
}

impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Network(reason) => write!(f, "sem conexão com o servidor de versões ({reason})"),
            Self::Rejected(reason) => write!(f, "atualização recusada ({reason})"),
        }
    }
}

impl std::error::Error for UpdateError {}

/// De onde vem a última versão publicada (as releases do GitHub).
pub trait UpdateSource {
    async fn latest_release(&mut self) -> Result<Release, UpdateError>;
}

/// Baixa, confere a assinatura e troca o executável. Quem reinicia é o daemon.
pub trait UpdateInstaller {
    async fn install(&mut self, release: &Release) -> Result<(), UpdateError>;
}
