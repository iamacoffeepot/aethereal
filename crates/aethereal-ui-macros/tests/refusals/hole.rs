// An operator in a text's hole: the error marks the whole string, as stable Rust cannot point inside a literal.
aethereal_ui::view! { Hud, ui,
    Text { "Gold: {self.gold + 1}" }
}

fn main() {}
