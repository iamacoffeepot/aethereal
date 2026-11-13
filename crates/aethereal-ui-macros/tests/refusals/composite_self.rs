// `self` in a composite, which has no state of its own.
aethereal_ui::widget! {
    struct Gauge {}

    Column { Bar { fraction: self.level, .. } }
}

fn main() {}
