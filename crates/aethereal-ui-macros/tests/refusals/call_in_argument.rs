// A call inside a call's argument: an event's arguments are copied paths.
aethereal_ui::view! { Hud, ui,
    Button { click => self.buy(self.price()) }
}

fn main() {}
