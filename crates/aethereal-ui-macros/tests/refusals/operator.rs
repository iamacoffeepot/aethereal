// An operator in a reference: a computed value is a `&self` method that computes it.
aethereal_ui::view! { Hud, ui,
    Column { show: self.gold > 1 }
}

fn main() {}
