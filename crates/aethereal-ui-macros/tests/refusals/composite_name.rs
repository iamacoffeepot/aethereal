// A name within a composite, which is placed at many paths, so its own nodes have none.
aethereal_ui::widget! {
    struct Gauge { level: f32 }

    Column { Bar { name: fill, fraction: level, .. } }
}

fn main() {}
