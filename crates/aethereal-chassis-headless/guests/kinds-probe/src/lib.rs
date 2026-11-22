//! Spike: kinds in a crate the echo guest links, some its code reaches and some it never names (ADR-0054).

pub mod used {
    use aethereal_data::Plain;

    /// Made by a function the echo guest calls from a handler.
    #[derive(Plain)]
    #[kind(name = "spike.probe.used")]
    pub struct Used {
        pub n: u64,
    }

    /// Beside `Used` in its module; nothing names it.
    #[derive(Plain)]
    #[kind(name = "spike.probe.beside")]
    pub struct Beside {
        pub n: u64,
    }

    /// A `Used` of `n`, which the echo guest calls.
    #[inline(never)]
    #[must_use]
    pub fn make(n: u64) -> Used {
        Used { n: n.wrapping_mul(3) }
    }
}

pub mod typed {
    use aethereal_data::Plain;

    /// Named by the echo guest only in a constant: its id, no code.
    #[derive(Plain)]
    #[kind(name = "spike.probe.typed")]
    pub struct Typed {
        pub n: u64,
    }
}

pub mod apart {
    use aethereal_data::Plain;

    /// In a module of its own that nothing names.
    #[derive(Plain)]
    #[kind(name = "spike.probe.apart")]
    pub struct Apart {
        pub n: u64,
    }

    /// A generic kind: the derive accepts it, and writes no id for it.
    #[derive(Plain)]
    #[kind(name = "spike.probe.wrapped")]
    pub struct Wrapped<T> {
        pub value: T,
    }
}
