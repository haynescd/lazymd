# Codon showcase

Every element codon knows how to draw, in one file. Resize the terminal to
watch paragraphs, tables, and code blocks re-flow to the new width.

## Inline styles

Plain, **bold**, *italic*, ***both***, ~~struck~~, `inline code`, a
[link](https://example.com), an autolink https://example.com, and
**bold with a [link](https://example.com) inside**.
A hard break follows\
and this line starts fresh. So does this<br>one.

## Lists

- Bullets change glyph and color as they nest
  - second level
    - third level
      - fourth wraps back to the first glyph
- A long item that goes on and on so that it has to wrap onto a second line, which stays indented under its text
- [x] Finished task, dimmed
- [ ] Open task

1. Ordered
2. Numbers right-align
3. Once they reach
4. two
5. digits
6. like
7. this
8. list
9. does
10. here

- A loose list item

  with a second paragraph, a quote:

  > quoted inside a list

  and code:

  ```sh
  cargo run -- test/showcase.md
  ```

### Heading three
#### Heading four
##### Heading five
###### Heading six

## Quotes and alerts

> A blockquote gets a bar down the left. Long lines wrap and the bar follows
> them down.
>
> > Nested quotes stack their bars.

> [!NOTE]
> Alerts are blockquotes with a colored bar and a title.

> [!TIP]
> Helpful advice.

> [!WARNING]
> Watch out.

## Code

```rust
use std::collections::HashMap;

/// Counts word frequencies.
fn count(text: &str) -> HashMap<&str, usize> {
    let mut counts = HashMap::new();
    for word in text.split_whitespace() {
        *counts.entry(word).or_insert(0) += 1;
    }
    counts
}
```

```python
def greet(name: str) -> str:
    return f"hello, {name}"  # a comment
```

```
No language: plain text on the same background.
```

## Tables

| Element       | Before           | After                        |
| :------------ | :--------------: | ---------------------------: |
| Heading       | `# text`         | **bold** + color, no `#`     |
| Link          | `text (url)`     | underlined                   |
| Table         | `[table]`        | a real grid                  |

| Column | A very long column whose cells will need to wrap when the terminal is narrow |
| ------ | ------------------------------------------------------------------------------ |
| short  | This cell has a lot of text in it so that the table has to shrink to fit the screen. |

## Everything else

A thematic break:

---

Raw HTML is shown dimmed; comments are hidden.

<details>
<summary>HTML block</summary>
</details>

<!-- you should not see this -->

![An image's alt text](image.png)

Footnotes work too.[^1]

[^1]: This is the footnote.
