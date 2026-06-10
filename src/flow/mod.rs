//! Sliding-window flow control.

pub mod error;
pub mod window;

pub use error::FlowError;
pub use window::{ReceiverWindow, SenderWindow};
