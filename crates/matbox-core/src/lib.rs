//! MATBOX core: lexer, MATLAB-flavored parser, interpreter, and column-major
//! workspace. Pure Rust. Reused by the Tauri desktop shell, the CLI, and the
//! Rust/Python/C++ SDKs.

#![allow(
    clippy::needless_range_loop,
    clippy::if_same_then_else
)]

pub mod ast;
pub mod builtins;
pub mod eval;
pub mod lexer;
pub mod parser;

pub use eval::{Engine, EngineError, Value};
pub use parser::Parser;
