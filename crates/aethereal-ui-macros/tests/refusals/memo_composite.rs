// `memo` within a composite, which builds its own tree whole each time it is built.
aethereal_ui::widget! {
    struct Gauge { level: f32 }

    Column { Meter { memo, level: level } }
}

fn main() {}
