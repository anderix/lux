//! lux — a small teaching language.
//!
//! The pipeline is the classic three stages: `lexer` turns source text into
//! tokens, `parser` turns tokens into an `ast`, and `interpreter` walks the
//! ast and runs it. `diagnostic` carries source positions so an error can
//! point at the exact place it went wrong.

pub mod ast;
pub mod convert;
pub mod diagnostic;
pub mod interpreter;
pub mod lexer;
pub mod parser;
