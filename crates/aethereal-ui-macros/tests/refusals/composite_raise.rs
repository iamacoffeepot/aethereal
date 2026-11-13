// An event the composite does not declare among its `raises`.
aethereal_ui::widget! {
    struct Buy {}
    raises click;

    Button { click => press }
}

fn main() {}
