//! A tool's command line (ADR-0053): a line typed at the dev overlay's console, or later at a shell, split into words,
//! read by the flags the tool's sheet carries into its input's JSON and then its bytes; and an answer's value printed
//! in the forms those flags read, so it reads as it would be typed.
//!
//! ```text
//! line ── split_words ──> words ── CommandLine::read, by the sheet's flags and each field's Form ──> JSON
//!      ── the input's schema ──> bytes ──> Call
//! Called::Output ── print_value, each value in its Form ──> text
//! sheets ── help_listing, a row a tool in padded columns ──> `help`;  CommandLine::help ──> `help <tool>`
//! ```
//!
//! [`CommandLine::of`] is also what a holder checks of each sheet offered, so a sheet whose flags do not fit its
//! input is refused as it is offered ([`Offered::BadFlags`](crate::Offered::BadFlags)), never as it is typed.

mod error;
mod flags;
mod help;
mod layout;
mod listing;
mod print;
mod read;
#[cfg(test)]
mod tests;
mod words;

pub use error::{CommandError, FlagsFault};
pub use flags::CommandLine;
pub use help::{Usage, first_sentence};
pub use listing::help_listing;
pub use print::{print_image, print_value};
pub use words::split_words;
