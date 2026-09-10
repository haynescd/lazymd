use comrak::{
    Arena, Options, markdown_to_html,
    nodes::{AstNode, ListType, NodeValue},
    options::Extension,
    parse_document,
};

#[derive(Debug)]
struct Render {
    lines: Vec<String>,
}

impl Render {
    pub fn new() -> Self {
        Render { lines: Vec::new() }
    }

    fn inline_children<'a>(&self, n: &'a AstNode<'a>) -> String {
        let mut s = String::new();
        for child in n.children() {
            s.push_str(&self.inline(child));
        }
        s
    }
    fn block<'a>(&mut self, n: &'a AstNode<'a>, prefix: &str) {
        let data = n.data.borrow();

        match &data.value {
            NodeValue::Document => {
                for child in n.children() {
                    self.block(child, prefix);
                }
            }
            NodeValue::Heading(h) => {
                let line = format!(
                    "{prefix}{} {}",
                    "#".repeat(h.level as usize),
                    self.inline_children(n)
                );
                self.lines.push(line);
                self.lines.push(String::new());
            }
            NodeValue::Paragraph => {
                self.lines
                    .push(format!("{prefix}{}", self.inline_children(n)));
            }
            NodeValue::List(nl) => {
                for (i, item) in n.children().enumerate() {
                    let marker = match nl.list_type {
                        ListType::Bullet => "- ".to_string(),
                        ListType::Ordered => format!("{}. ", nl.start + i),
                    };

                    self.block(item, &format!("{prefix}{marker}"));
                }
                self.lines.push(String::new());
            }
            NodeValue::Item(_) => {
                let cont = " ".repeat(prefix.chars().count()); // continuation = marker-width spaces
                for (j, child) in n.children().enumerate() {
                    let p = if j == 0 { prefix } else { &cont };
                    self.block(child, p);
                }
            }
            NodeValue::TaskItem(task_item) => {
                let mark = task_item.symbol.unwrap_or(' ');
                let first = format!("{prefix}[{mark}] ");
                let cont = " ".repeat(first.chars().count()); // continuation = box-width spaces
                for (j, child) in n.children().enumerate() {
                    let p = if j == 0 { &first } else { &cont };
                    self.block(child, p);
                }
            }
            NodeValue::CodeBlock(cb) => {
                let marker = format!("{prefix}|");
                for l in cb.literal.lines() {
                    self.lines.push(format!("{marker}{l}"));
                }
                self.lines.push(String::new());
            }

            NodeValue::ThematicBreak => {
                self.lines.push(String::new());
                self.lines.push("--".repeat(10));
                self.lines.push(String::new());
            }
            NodeValue::BlockQuote => {
                for child in n.children() {
                    self.block(child, &format!("{prefix}> "));
                }
            }
            NodeValue::Table(_) => {
                self.lines.push(format!("{prefix}[table]"));
                self.lines.push(String::new());
            }
            _ => {}
        };
    }

    fn inline<'a>(&self, n: &'a AstNode<'a>) -> String {
        let data = n.data.borrow();
        match &data.value {
            NodeValue::Text(t) => t.to_string(),
            NodeValue::Strong => format!("**{}**", self.inline_children(n)),
            NodeValue::Emph => format!("*{}*", self.inline_children(n)),
            NodeValue::Code(c) => format!("`{}`", c.literal),
            NodeValue::SoftBreak => " ".into(),
            NodeValue::LineBreak => "\n".into(),
            NodeValue::Strikethrough => format!("~{}~", self.inline_children(n)),
            NodeValue::Link(link) => {
                let url = &link.url;
                let title = self.inline_children(n);
                format!("{title} ({url})")
            }
            _ => self.inline_children(n),
        }
    }
}

fn generate_options() -> Options<'static> {
    Options {
        extension: Extension {
            table: true,
            strikethrough: true,
            tasklist: true,
            autolink: true,
            ..Default::default()
        },
        ..Default::default()
    }
}

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

pub fn render_ast(md: &str) {
    let options = generate_options();
    let arena = Arena::new();

    let root = parse_document(&arena, md, &options);

    let mut render = Render::new();
    render.block(root, "");

    for line in render.lines {
        println!("{line}");
    }
}

//fn walk<'a>(n: &'a AstNode<'a>, depth: usize) {
//    let data = n.data.borrow();
//
//    let label = match &data.value {
//        NodeValue::Heading(h) => format!("Heading (level {})", h.level),
//        NodeValue::Text(t) => format!("Text {:?}", t),
//        NodeValue::Code(c) => format!("Code {:?}", c.literal),
//        NodeValue::List(_) => "List".to_string(),
//        other => format!("{:?}", other),
//    };
//
//    println!("{}{}", " ".repeat(depth), label);
//
//    for child in n.children() {
//        walk(child, depth + 1);
//    }
//}
