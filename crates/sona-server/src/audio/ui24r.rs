//! Mapa USB da Soundcraft Ui24R (manual do usuário V1.0, seção 9.0.2, p. 101):
//! o computador recebe 32 canais nesta ordem.
//!
//! | USB | sinal da mesa |
//! |---|---|
//! | 1–2 | Master L/R |
//! | 3–10 | AUX 1–8 |
//! | 11–30 | Ch 1–20 |
//! | 31–32 | Line In L/R (canais 21 e 22 da mesa) |
//!
//! O Player L/R e os retornos de efeito não saem pela USB; só chegam
//! misturados no Master e nos AUX.

pub const USB_CHANNELS: usize = 32;
const MASTER: usize = 0;
const AUX: usize = 2;
const CH: usize = 10;
const LINE: usize = 30;

/// O dispositivo principal é a Ui24R (driver "Soundcraft USB Audio ASIO" ou CORE).
pub fn matches(device: &str) -> bool {
    device.to_ascii_lowercase().contains("soundcraft")
}

/// Nome da mesa para a entrada USB `i` (0 = USB 1).
pub fn input_name(i: usize) -> Option<String> {
    Some(match i {
        MASTER => "Master L".into(),
        1 => "Master R".into(),
        AUX..CH => format!("AUX {}", i - AUX + 1),
        CH..LINE => format!("Ch {}", i - CH + 1),
        LINE => "Line In L".into(),
        31 => "Line In R".into(),
        _ => return None,
    })
}

/// Posição do canal no Sona: primeiro os canais da mesa na numeração dela
/// (Ch 1–20, Line In), depois os retornos (Master, AUX).
pub fn order(i: usize) -> usize {
    match i {
        CH..USB_CHANNELS => i - CH,
        _ => i + (USB_CHANNELS - CH),
    }
}

/// Master e AUX já são mixagens da mesa: somá-los aos canais duplica o áudio.
pub fn is_desk_mix(i: usize) -> bool {
    i < CH
}

pub const DESK_MIX_TAG: &str = "mix da mesa";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapa_do_manual() {
        assert_eq!(input_name(0).unwrap(), "Master L");
        assert_eq!(input_name(2).unwrap(), "AUX 1");
        assert_eq!(input_name(9).unwrap(), "AUX 8");
        assert_eq!(input_name(10).unwrap(), "Ch 1");
        assert_eq!(input_name(29).unwrap(), "Ch 20");
        assert_eq!(input_name(31).unwrap(), "Line In R");
        assert!(input_name(32).is_none());
        assert_eq!(order(10), 0);
        assert_eq!(order(31), 21);
        assert_eq!(order(0), 22);
        assert_eq!(order(9), 31);
        assert!(matches("ASIO:Soundcraft USB Audio ASIO"));
    }
}
