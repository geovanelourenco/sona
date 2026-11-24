//! Núcleo de áudio do Sona.
//!
//! Este crate não faz I/O. Ele recebe blocos de amostras `f32`, processa e
//! devolve blocos. Dispositivos, rede e interface ficam em outros crates.
//!
//! Regra de tempo real: [`engine::Engine::process`] não aloca, não bloqueia e
//! não faz I/O. Toda alocação acontece em [`plan::compile`], fora da thread de
//! áudio. A thread de áudio recebe o plano pronto por uma fila sem trava.

// Laços com índice são mais claros em DSP com vários buffers paralelos.
#![allow(clippy::needless_range_loop)]

pub mod biquad;
pub mod config;
pub mod dynamics;
pub mod engine;
pub mod level;
pub mod meters;
pub mod plan;
pub mod smooth;

pub use config::*;
pub use engine::{Engine, EngineHandle};
pub use meters::Meters;
pub use plan::{compile, Layout, Plan, Validation};
