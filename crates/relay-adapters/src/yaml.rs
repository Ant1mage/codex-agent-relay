//! A reader for the YAML that "dsh --dump-config" prints.
//!
//! This is deliberately not a YAML implementation. It reads the shape the dump
//! has — a top-level sequence of rows, each a mapping with a scalar "id" and an
//! optional "config" mapping — and it resolves every scalar form that shape can
//! contain, including block scalars.
//!
//! Block scalars are the reason this module exists. js-yaml folds a long or
//! multi-line value across lines ("personaPrefix: >-"), and a reader that only
//! understands "key: value" silently loses the profile's own persona. Relay then
//! cannot restate it, so the honest answers are to resolve it or to leave the row
//! alone — never to write an empty persona over it.

/// One value in the dump.
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    /// A resolved scalar: plain, single-quoted, double-quoted or a block scalar.
    Scalar(String),
    /// A value Relay will not restate: a "!!js" expression, a flow collection, an
    /// alias, or anything else outside the shape this reader models.
    Opaque(String),
    Sequence(Vec<Node>),
    Mapping(Vec<(String, Node)>),
}

impl Node {
    pub fn get(&self, key: &str) -> Option<&Node> {
        match self {
            Node::Mapping(entries) => entries
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    /// The value as text, or None when Relay must not restate it.
    pub fn text(&self) -> Option<&str> {
        match self {
            Node::Scalar(value) => Some(value),
            _ => None,
        }
    }

    pub fn items(&self) -> Option<&[Node]> {
        match self {
            Node::Sequence(items) => Some(items),
            _ => None,
        }
    }

    /// Every key of a mapping, in document order.
    pub fn keys(&self) -> Vec<&str> {
        match self {
            Node::Mapping(entries) => entries.iter().map(|(key, _)| key.as_str()).collect(),
            _ => Vec::new(),
        }
    }

    /// The subset of keys whose values this reader could resolve.
    pub fn resolved_keys(&self) -> Vec<&str> {
        match self {
            Node::Mapping(entries) => entries
                .iter()
                .filter(|(_, value)| matches!(value, Node::Scalar(_)))
                .map(|(key, _)| key.as_str())
                .collect(),
            _ => Vec::new(),
        }
    }
}

/// One entry of the composed entry list.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub id: String,
    pub keys: Vec<(String, Node)>,
}

impl Row {
    pub fn get(&self, key: &str) -> Option<&Node> {
        self.keys
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value)
    }

    /// The row's own "config" mapping, when it has one.
    pub fn config(&self) -> Option<&Node> {
        self.get("config")
    }
}

/// Parses the dump into its rows. Anything it cannot model stays opaque, so a
/// caller can decide to leave that value — or that whole row — untouched.
pub fn rows(text: &str) -> Vec<Row> {
    let mut reader = Reader {
        lines: text.lines().collect(),
        at: 0,
    };
    let mut rows = Vec::new();
    loop {
        reader.skip_noise();
        let Some(line) = reader.lines.get(reader.at) else {
            break;
        };
        let indent = indent_of(line);
        let trimmed = line.trim_start();
        if !trimmed.starts_with("- ") && trimmed != "-" {
            reader.at += 1;
            continue;
        }
        if let Node::Mapping(entries) = reader.read_sequence_item(indent) {
            let id = entries
                .iter()
                .find(|(key, _)| key == "id")
                .and_then(|(_, value)| value.text())
                .map(str::to_string);
            if let Some(id) = id {
                rows.push(Row { id, keys: entries });
            }
        }
    }
    rows
}

struct Reader<'a> {
    lines: Vec<&'a str>,
    at: usize,
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// Splits "key: value" at the first colon that ends a key.
fn split_key(text: &str) -> Option<(String, &str)> {
    let colon = text.find(':')?;
    let key = text[..colon].trim();
    if key.is_empty() || key.contains(char::is_whitespace) {
        return None;
    }
    Some((key.to_string(), text[colon + 1..].trim_start()))
}

fn is_block_header(value: &str) -> bool {
    value.starts_with('|') || value.starts_with('>')
}

impl<'a> Reader<'a> {
    /// Blank lines and whole-line comments are not part of any value.
    fn skip_noise(&mut self) {
        while let Some(line) = self.lines.get(self.at) {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                self.at += 1;
            } else {
                break;
            }
        }
    }

    fn read_sequence_item(&mut self, indent: usize) -> Node {
        let rest = self.lines[self.at].trim_start()[1..]
            .trim_start()
            .to_string();
        self.at += 1;
        if rest.is_empty() {
            return self.read_node(indent + 2);
        }
        match split_key(&rest) {
            Some((key, value)) => {
                let mut entries = vec![(key, self.read_value(value.to_string(), indent + 2))];
                entries.extend(self.read_mapping(indent + 2));
                Node::Mapping(entries)
            }
            None => self.read_value(rest, indent + 2),
        }
    }

    fn read_node(&mut self, indent: usize) -> Node {
        self.skip_noise();
        let Some(line) = self.lines.get(self.at) else {
            return Node::Mapping(Vec::new());
        };
        let current = indent_of(line);
        let starts_item = {
            let trimmed = line.trim_start();
            trimmed.starts_with("- ") || trimmed == "-"
        };
        if current < indent {
            return Node::Mapping(Vec::new());
        }
        if starts_item {
            let mut items = Vec::new();
            loop {
                self.skip_noise();
                let Some(line) = self.lines.get(self.at) else {
                    break;
                };
                let current = indent_of(line);
                let trimmed = line.trim_start();
                if current != indent || (!trimmed.starts_with("- ") && trimmed != "-") {
                    break;
                }
                items.push(self.read_sequence_item(indent));
            }
            Node::Sequence(items)
        } else {
            Node::Mapping(self.read_mapping(current))
        }
    }

    fn read_mapping(&mut self, indent: usize) -> Vec<(String, Node)> {
        let mut entries = Vec::new();
        loop {
            self.skip_noise();
            let Some(line) = self.lines.get(self.at) else {
                break;
            };
            let current = indent_of(line);
            let trimmed = line.trim_start();
            if current != indent || trimmed.starts_with("- ") {
                break;
            }
            let Some((key, value)) = split_key(trimmed) else {
                break;
            };
            let inline = value.to_string();
            self.at += 1;
            entries.push((key, self.read_value(inline, indent)));
        }
        entries
    }

    /// The value written after "key:", or the block that follows it.
    fn read_value(&mut self, inline: String, parent_indent: usize) -> Node {
        if inline.starts_with("!!") {
            return Node::Opaque(inline);
        }
        if inline.is_empty() {
            self.skip_noise();
            let Some(line) = self.lines.get(self.at) else {
                return Node::Scalar(String::new());
            };
            let current = indent_of(line);
            if current > parent_indent {
                return self.read_node(current);
            }
            return Node::Scalar(String::new());
        }
        if is_block_header(&inline) {
            return self.read_block_scalar(&inline, parent_indent);
        }
        if inline.starts_with('[') || inline.starts_with('{') || inline.starts_with('*') {
            return Node::Opaque(inline);
        }
        Node::Scalar(plain_or_quoted(&inline))
    }

    /// YAML's two block scalar styles, with the chomping indicator applied.
    fn read_block_scalar(&mut self, header: &str, parent_indent: usize) -> Node {
        let literal = header.starts_with('|');
        let strip = header.contains('-');
        let keep = header.contains('+');
        let explicit = header
            .chars()
            .find(|character| character.is_ascii_digit())
            .and_then(|digit| digit.to_digit(10))
            .map(|digit| parent_indent + digit as usize);

        let mut collected: Vec<String> = Vec::new();
        let mut content_indent = explicit;
        while let Some(line) = self.lines.get(self.at).copied() {
            if line.trim().is_empty() {
                collected.push(String::new());
                self.at += 1;
                continue;
            }
            let current = indent_of(line);
            if current <= parent_indent {
                break;
            }
            match content_indent {
                None => content_indent = Some(current),
                Some(expected) if current < expected => break,
                _ => {}
            }
            let indent = content_indent.unwrap_or(current);
            collected.push(line.chars().skip(indent).collect());
            self.at += 1;
        }

        if !keep {
            while collected.last().is_some_and(|line| line.is_empty()) {
                collected.pop();
            }
        }

        let content_indent = content_indent.unwrap_or(parent_indent + 2);
        let mut text = if literal {
            collected.join("\n")
        } else {
            fold(&collected)
        };
        if !strip && !text.is_empty() {
            text.push('\n');
        }
        let _ = content_indent;
        Node::Scalar(text)
    }
}

/// Folded style: a line break becomes a space, unless a line is blank or more
/// indented than the block, in which case it stays a line break.
fn fold(lines: &[String]) -> String {
    let mut out = String::new();
    let mut blanks = 0usize;
    let mut previous_indented = false;
    let mut first = true;
    for line in lines {
        let indented = line.starts_with(' ') || line.starts_with('\t');
        if line.is_empty() {
            blanks += 1;
            continue;
        }
        if first {
            out.push_str(line);
            first = false;
        } else if blanks > 0 {
            // n blank lines stand for n line breaks, not n + 1.
            for _ in 0..blanks {
                out.push('\n');
            }
            out.push_str(line);
        } else if indented || previous_indented {
            out.push('\n');
            out.push_str(line);
        } else {
            out.push(' ');
            out.push_str(line);
        }
        blanks = 0;
        previous_indented = indented;
    }
    out
}

/// Plain scalars lose a trailing comment; quoted scalars are unescaped.
fn plain_or_quoted(value: &str) -> String {
    let trimmed = value.trim_end();
    if trimmed.len() >= 2 && trimmed.starts_with('\'') && trimmed.ends_with('\'') {
        return trimmed[1..trimmed.len() - 1].replace("''", "'");
    }
    if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        return unescape_double(&trimmed[1..trimmed.len() - 1]);
    }
    match trimmed.find(" #") {
        Some(at) => trimmed[..at].trim_end().to_string(),
        None => trimmed.to_string(),
    }
}

fn unescape_double(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut characters = value.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            out.push(character);
            continue;
        }
        match characters.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some('u') => {
                let digits: String = characters.by_ref().take(4).collect();
                match u32::from_str_radix(&digits, 16)
                    .ok()
                    .and_then(char::from_u32)
                {
                    Some(character) => out.push(character),
                    None => {
                        out.push_str("\\u");
                        out.push_str(&digits);
                    }
                }
            }
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_of(text: &str, id: &str) -> Node {
        let rows = rows(text);
        rows.iter()
            .find(|row| row.id == id)
            .and_then(|row| row.config().cloned())
            .unwrap_or_else(|| panic!("no config for {id}"))
    }

    /// The case that broke Relay: a folded persona Relay must resolve, not drop.
    #[test]
    fn a_folded_persona_is_resolved_not_lost() {
        let dump = "\
- id: system-prompt
  config:
    personaPrefix: >-
      You are a coding agent powered by {{model}}.
    personaSuffix: Your working directory is {{cwd}}.
    includeHarnessIdentity: false
";
        let config = config_of(dump, "system-prompt");
        assert_eq!(
            config.get("personaPrefix").and_then(Node::text),
            Some("You are a coding agent powered by {{model}}.")
        );
        assert_eq!(
            config.resolved_keys(),
            vec!["personaPrefix", "personaSuffix", "includeHarnessIdentity"]
        );
        assert_eq!(
            config.get("includeHarnessIdentity").and_then(Node::text),
            Some("false")
        );
    }

    #[test]
    fn a_folded_persona_keeps_its_paragraphs() {
        let dump = "\
- id: system-prompt
  config:
    personaPrefix: >-
      First line
      still the first paragraph.

      Second paragraph starts here.
    personaSuffix: tail
";
        let config = config_of(dump, "system-prompt");
        assert_eq!(
            config.get("personaPrefix").and_then(Node::text),
            Some("First line still the first paragraph.\nSecond paragraph starts here.")
        );
    }

    #[test]
    fn a_literal_persona_keeps_its_lines_and_chomping() {
        let dump = "\
- id: system-prompt
  config:
    personaPrefix: |-
      line one
      line two
    personaSuffix: |
      trailing
";
        let config = config_of(dump, "system-prompt");
        assert_eq!(
            config.get("personaPrefix").and_then(Node::text),
            Some("line one\nline two")
        );
        assert_eq!(
            config.get("personaSuffix").and_then(Node::text),
            Some("trailing\n")
        );
    }

    #[test]
    fn quoted_scalars_are_unescaped() {
        let dump = "\
- id: row
  config:
    single: 'it''s here'
    double: \"tab\\there\"
    plain: value with words
";
        let config = config_of(dump, "row");
        assert_eq!(config.get("single").and_then(Node::text), Some("it's here"));
        assert_eq!(config.get("double").and_then(Node::text), Some("tab\there"));
        assert_eq!(
            config.get("plain").and_then(Node::text),
            Some("value with words")
        );
    }

    /// A "!!js" value is never restated, and never silently becomes its text.
    #[test]
    fn javascript_expressions_stay_opaque() {
        let dump = "\
- id: row
  config:
    mode: !!js process.env.DSH_TOOLS_MODE
    script: !!js ctx.get('x')
";
        let config = config_of(dump, "row");
        assert!(matches!(config.get("mode"), Some(Node::Opaque(_))));
        assert!(matches!(config.get("script"), Some(Node::Opaque(_))));
        assert!(config.resolved_keys().is_empty());
    }

    #[test]
    fn a_model_catalogue_is_read_as_a_sequence() {
        let dump = "\
- id: llm-deepseek
  name: '@deepseek-ai/dsh-llm-deepseek-api-key'
  config:
    models:
      - id: deepseek-v4-pro
        name: DeepSeek-V4-Pro
      - id: deepseek-flash
        name: DeepSeek-V41-Flash
";
        let config = config_of(dump, "llm-deepseek");
        let models = config.get("models").and_then(Node::items).unwrap();
        assert_eq!(models.len(), 2);
        assert_eq!(
            models[0].get("id").and_then(Node::text),
            Some("deepseek-v4-pro")
        );
        assert_eq!(
            models[1].get("name").and_then(Node::text),
            Some("DeepSeek-V41-Flash")
        );
    }

    #[test]
    fn the_real_dump_shape_yields_every_row() {
        let dump = "# == @deepseek-ai/dsh-base\n\
- id: timer
  name: '@deepseek-ai/cordis-plugin-timer'
- id: hmr
  name: '@deepseek-ai/dsh-hmr'
  disabled: true
  config:
    root: []
- id: system-prompt
  name: '@deepseek-ai/dsh-system-prompt'
  config:
    personaSuffix: Your working directory is {{cwd}}.
    personaPrefix: You are a coding agent powered by the {{model}} model.
# == @deepseek-ai/dsh-headless
- id: agent-default-model
  config:
    provider: deepseek-official
    model: deepseek-flash
";
        let rows = rows(dump);
        assert_eq!(
            rows.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(),
            vec!["timer", "hmr", "system-prompt", "agent-default-model"]
        );
        let prompt = rows.iter().find(|row| row.id == "system-prompt").unwrap();
        assert_eq!(
            prompt.get("name").and_then(Node::text),
            Some("@deepseek-ai/dsh-system-prompt")
        );
        let default = rows
            .iter()
            .find(|row| row.id == "agent-default-model")
            .unwrap();
        assert_eq!(
            default
                .config()
                .and_then(|config| config.get("provider"))
                .and_then(Node::text),
            Some("deepseek-official")
        );
    }
}
