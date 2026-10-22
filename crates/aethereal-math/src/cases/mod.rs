//! Tables of fixed inputs and the answers the math types give for them, which another implementation of the same
//! meanings follows: a shader's port of a type is checked against the table its CPU original is.
//!
//! Each table is asserted against the type here, so a table and the type it describes cannot drift apart. The tables
//! are behind the feature `cases`, as only the tests of another implementation read them.

pub mod segment;
