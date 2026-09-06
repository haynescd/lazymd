use comrak::{Options, markdown_to_html};

pub fn render_markdown(md: &str) {
    // Define GFM extensions
    let mut options = Options::default();
    options.extension.table = true;
    options.extension.strikethrough = true;
    options.extension.tasklist = true;
    options.extension.autolink = true;

    let html = markdown_to_html(md, &options);
    print!("{html}");
}
