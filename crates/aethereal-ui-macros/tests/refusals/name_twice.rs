// One name on two nodes, one of them an item of a list: a name is one node's.
aethereal_ui::view! { Hud, ui,
    Column {
        Text { name: gold, "{self.gold}" }
        Column { items: self.coins as coin key coin.id,
            Text { name: gold, "{coin.value}" }
        }
    }
}

fn main() {}
