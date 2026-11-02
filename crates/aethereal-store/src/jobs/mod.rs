//! The jobs only the store makes: each the stockroom's work, run on `Io` as it waits on the host (ADR-0018,
//! ADR-0039, ADR-0040). The store runs one at a time, so the stockroom does what it is asked in the order it was asked.

mod issue;
mod recall;
mod recount;
mod shelve;
mod stock;

pub use issue::{Issue, Issued, Issuing};
pub use recall::{Recall, Recalled, Recalling};
pub use recount::{Counted, Counting, Recount};
pub use shelve::{Shelve, Shelved, Shelving};
pub use stock::{Stock, Stocked, Stocking};
