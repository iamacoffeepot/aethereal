// `memo` on an element that is dragged, whose payload reads more than the composite's fields.
aethereal_ui::view! { Hud, ui,
    Column {
        Slot { memo, item: self.item, drag: self.item }
    }
}

fn main() {}
