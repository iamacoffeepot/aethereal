// `memo` on an element with children, which read the screen beside the composite's fields.
aethereal_ui::view! { Hud, ui,
    Column {
        Slot { memo, item: self.item,
            Text { "{self.gold}" }
        }
    }
}

fn main() {}
