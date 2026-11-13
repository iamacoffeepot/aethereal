// A closure for a value: logic lives in Rust, never in the view.
aethereal_ui::view! { Hud, ui,
    Column { show: |gold| gold }
}

fn main() {}
