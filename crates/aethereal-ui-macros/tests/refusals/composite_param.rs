// A name the composite does not declare as a param or make with `as`.
aethereal_ui::widget! {
    struct Meter { fraction: f32 }

    Bar { fraction: amount, .. }
}

fn main() {}
