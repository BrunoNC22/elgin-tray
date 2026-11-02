//! As telas da configuração inicial (login por QR do Smart Life): o que cada passo mostra.
//! Puro (testável); o `runner` passa para as propriedades do Slint.

use elgin_ipc::messages::SetupStateDto;

/// Qual tela mostrar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupStep {
    /// Campo do user code (também depois de um erro, para tentar de novo).
    UserCode,
    /// QR code esperando a confirmação no celular.
    Qr,
    /// Lista de lâmpadas da conta.
    Choose,
    /// Esperando (procurando a lâmpada na rede).
    Working,
    /// Terminou.
    Done,
}

impl SetupStep {
    /// Número da tela no Slint (`setup-step`).
    pub fn index(self) -> i32 {
        match self {
            Self::UserCode => 0,
            Self::Qr => 1,
            Self::Choose => 2,
            Self::Working => 3,
            Self::Done => 4,
        }
    }
}

/// Uma lâmpada da conta, na lista para escolher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupDeviceItem {
    pub id: String,
    pub name: String,
    pub product: String,
}

/// O que a tela da configuração mostra.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupView {
    pub step: SetupStep,
    /// Texto principal do passo.
    pub message: String,
    /// Dica de rodapé (vazia = sem dica).
    pub hint: String,
    /// Erro do passo anterior (vazio = sem erro).
    pub error: String,
    /// Conteúdo do QR code (só no passo [`SetupStep::Qr`]).
    pub qr_content: Option<String>,
    pub devices: Vec<SetupDeviceItem>,
    /// Texto do botão que fecha a configuração.
    pub close_label: &'static str,
}

const USER_CODE_HELP: &str = "Digite o código de usuário do Smart Life. No app: Eu → \
     engrenagem (Configurações) → Conta e segurança → Código de usuário.";
const ELGIN_HINT: &str = "A lâmpada precisa estar no app Smart Life. Se ela está no app Elgin \
     Smart, remova-a de lá antes de parear no Smart Life.";
const FIREWALL_HINT: &str = "Ainda não achei a lâmpada na rede local; o app continua \
     procurando. Se o Windows perguntar, permita o elgin-tray no firewall.";

impl SetupView {
    pub fn of(state: &SetupStateDto) -> Self {
        let base = Self {
            step: SetupStep::UserCode,
            message: USER_CODE_HELP.to_owned(),
            hint: ELGIN_HINT.to_owned(),
            error: String::new(),
            qr_content: None,
            devices: Vec::new(),
            close_label: "Cancelar",
        };
        match state {
            SetupStateDto::AwaitingUserCode => base,
            SetupStateDto::Failed { reason } => Self {
                error: format!("Não deu certo: {reason}"),
                ..base
            },
            SetupStateDto::WaitingConfirmation { qr_content } => Self {
                step: SetupStep::Qr,
                message: "No app Smart Life, toque no ícone de leitura (canto superior direito da \
                          tela inicial), leia este código e confirme o login no celular."
                    .to_owned(),
                hint: "Aguardando a confirmação no celular…".to_owned(),
                qr_content: Some(qr_content.clone()),
                ..base
            },
            SetupStateDto::ChooseDevice { devices } => Self {
                step: SetupStep::Choose,
                message: "Escolha a lâmpada:".to_owned(),
                hint: String::new(),
                devices: devices
                    .iter()
                    .map(|device| SetupDeviceItem {
                        id: device.id.clone(),
                        name: device.name.clone(),
                        product: device.product_name.clone(),
                    })
                    .collect(),
                ..base
            },
            SetupStateDto::Discovering => Self {
                step: SetupStep::Working,
                message: "Procurando a lâmpada na rede…".to_owned(),
                hint: String::new(),
                ..base
            },
            SetupStateDto::Done {
                bulb_name,
                found_on_network,
            } => Self {
                step: SetupStep::Done,
                message: format!("Pronto! \"{bulb_name}\" está configurada."),
                hint: if *found_on_network {
                    String::new()
                } else {
                    FIREWALL_HINT.to_owned()
                },
                close_label: "Concluir",
                ..base
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use elgin_ipc::messages::SetupDeviceDto;

    use super::*;

    #[test]
    fn asks_for_the_user_code_with_directions() {
        let view = SetupView::of(&SetupStateDto::AwaitingUserCode);

        assert_eq!(view.step, SetupStep::UserCode);
        assert!(view.message.contains("Conta e segurança"));
        assert!(view.hint.contains("Elgin"));
        assert!(view.error.is_empty());
        assert_eq!(view.close_label, "Cancelar");
    }

    #[test]
    fn a_failure_asks_again_showing_the_reason() {
        let view = SetupView::of(&SetupStateDto::Failed {
            reason: "o QR code expirou".to_owned(),
        });

        assert_eq!(view.step, SetupStep::UserCode);
        assert_eq!(view.error, "Não deu certo: o QR code expirou");
    }

    #[test]
    fn shows_the_qr_code_while_waiting() {
        let view = SetupView::of(&SetupStateDto::WaitingConfirmation {
            qr_content: "tuyaSmart--qrLogin?token=abc".to_owned(),
        });

        assert_eq!(view.step, SetupStep::Qr);
        assert_eq!(
            view.qr_content.as_deref(),
            Some("tuyaSmart--qrLogin?token=abc")
        );
        assert!(view.hint.contains("Aguardando"));
    }

    #[test]
    fn lists_the_bulbs_to_choose() {
        let view = SetupView::of(&SetupStateDto::ChooseDevice {
            devices: vec![SetupDeviceDto {
                id: "bf1".to_owned(),
                name: "Quarto".to_owned(),
                product_name: "10W Smart color".to_owned(),
            }],
        });

        assert_eq!(view.step, SetupStep::Choose);
        assert_eq!(
            view.devices,
            [SetupDeviceItem {
                id: "bf1".to_owned(),
                name: "Quarto".to_owned(),
                product: "10W Smart color".to_owned(),
            }]
        );
        assert_eq!(view.qr_content, None);
    }

    #[test]
    fn shows_progress_and_the_result() {
        assert_eq!(
            SetupView::of(&SetupStateDto::Discovering).step,
            SetupStep::Working
        );

        let found = SetupView::of(&SetupStateDto::Done {
            bulb_name: "Quarto".to_owned(),
            found_on_network: true,
        });
        assert_eq!(found.step, SetupStep::Done);
        assert_eq!(found.message, "Pronto! \"Quarto\" está configurada.");
        assert!(found.hint.is_empty());
        assert_eq!(found.close_label, "Concluir");

        let not_found = SetupView::of(&SetupStateDto::Done {
            bulb_name: "Quarto".to_owned(),
            found_on_network: false,
        });
        assert!(not_found.hint.contains("firewall"));
    }

    #[test]
    fn step_indexes_match_the_slint_screens() {
        let steps = [
            SetupStep::UserCode,
            SetupStep::Qr,
            SetupStep::Choose,
            SetupStep::Working,
            SetupStep::Done,
        ];
        let indexes: Vec<i32> = steps.iter().map(|step| step.index()).collect();
        assert_eq!(indexes, [0, 1, 2, 3, 4]);
    }
}
