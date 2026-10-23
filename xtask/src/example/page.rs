//! An example's page: `shared/index.html` filled with the example's name and its layout, the example's own
//! `layout.css` and `layout.html` where it has them, and otherwise `shared/`'s, one canvas filling the page.

use crate::failure::Failure;
use std::fs;
use std::path::Path;

/// Writes the example `name`'s page into `pkg`.
pub fn write(examples: &Path, name: &str, pkg: &Path) -> Result<(), Failure> {
    let template = read(&examples.join("shared/index.html"))?;
    let css = layout(examples, name, "layout.css")?;
    let html = layout(examples, name, "layout.html")?;
    fs::write(pkg.join("index.html"), fill(&template, name, &css, &html))?;
    Ok(())
}

/// The example's own layout `part` where it has one, and otherwise `shared/`'s.
fn layout(examples: &Path, name: &str, part: &str) -> Result<String, Failure> {
    let own = examples.join(name).join(part);
    let chosen = if own.is_file() {
        own
    } else {
        examples.join("shared").join(part)
    };
    read(&chosen)
}

fn read(path: &Path) -> Result<String, Failure> {
    fs::read_to_string(path).map_err(|error| Failure::new(format_args!("failed read={} {error}", path.display())))
}

/// The page: the layout in place of `{{layout.css}}` and `{{layout.html}}`, each without its trailing newlines, then
/// the example's name in place of every `{{name}}`, the layout's own among them.
fn fill(template: &str, name: &str, css: &str, html: &str) -> String {
    template
        .replace("{{layout.css}}", css.trim_end_matches('\n'))
        .replace("{{layout.html}}", html.trim_end_matches('\n'))
        .replace("{{name}}", name)
}

#[cfg(test)]
mod tests {
    use super::fill;

    #[test]
    fn the_layout_goes_in_without_its_trailing_newlines_and_the_name_fills_it_too() {
        let page = fill(
            "<style>\n{{layout.css}}\n</style>\n{{layout.html}}\n<p>{{name}}</p>",
            "draw",
            "a {}\n\n",
            "<canvas id=\"{{name}}\"></canvas>\n",
        );

        assert_eq!(page, "<style>\na {}\n</style>\n<canvas id=\"draw\"></canvas>\n<p>draw</p>");
    }

    #[test]
    fn the_shared_page_holds_no_placeholder_once_filled() {
        let template = include_str!("../../../crates/aethereal-chassis-browser/examples/shared/index.html");

        let page = fill(template, "draw", "", "");

        assert!(!page.contains("{{"), "a placeholder the page names is never filled:\n{page}");
    }
}
