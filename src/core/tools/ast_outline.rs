use anyhow::Result;
use serde_json::{Value, json};

use crate::core::tools::Tool;
use crate::core::tools::fs::read_to_string;

pub struct AstOutline;

impl AstOutline {
    pub fn new() -> Self { Self }
}

impl Tool for AstOutline {
    fn name(&self) -> &str { "get_ast_outline" }
    fn category(&self) -> crate::core::tools::ToolCategory { crate::core::tools::ToolCategory::Filesystem }

    fn description(&self) -> &str {
        "Return the structural outline of a source file (structs, enums, traits, impl blocks, \
         top-level functions) without the body of each item. \
         Much cheaper than reading the full file when you only need to understand the shape of the code. \
         Currently supports Rust (.rs) files only; returns an error for other extensions."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type":        "string",
                    "description": "Path to the source file. Relative to project root or absolute."
                }
            },
            "required": ["path"]
        })
    }

    fn execute(&self, args: Value) -> Result<String> {
        let path = args["path"].as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing required argument: path"))?;

        let ext = std::path::Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");

        match ext {
            "rs" => outline_rust(path),
            other => Ok(format!("Language not supported for AST outline: .{other}\nSupported: .rs")),
        }
    }
}

fn outline_rust(path: &str) -> Result<String> {
    use syn::{File, Item, ImplItem, TraitItem};

    let content = read_to_string(path)?;
    let file: File = syn::parse_file(&content)
        .map_err(|e| anyhow::anyhow!("Parse error in {path}: {e}"))?;

    let mut out = format!("--- Rust outline: {path} ---\n\n");

    for item in &file.items {
        match item {
            Item::Fn(f) => {
                let vis = tok(&f.vis);
                let sig = tok(&f.sig);
                out.push_str(&fmt_line(&format!("{vis}{sig}"), 0));
            }
            Item::Struct(s) => {
                let vis = tok(&s.vis);
                let name = &s.ident;
                let generics = tok(&s.generics);
                out.push_str(&fmt_line(&format!("{vis}struct {name}{generics}"), 0));
            }
            Item::Enum(e) => {
                let vis = tok(&e.vis);
                let name = &e.ident;
                let generics = tok(&e.generics);
                out.push_str(&fmt_line(&format!("{vis}enum {name}{generics}"), 0));
                for v in &e.variants {
                    out.push_str(&fmt_line(&v.ident.to_string(), 1));
                }
            }
            Item::Trait(t) => {
                let vis = tok(&t.vis);
                let name = &t.ident;
                let generics = tok(&t.generics);
                out.push_str(&fmt_line(&format!("{vis}trait {name}{generics}"), 0));
                for item in &t.items {
                    if let TraitItem::Fn(m) = item {
                        out.push_str(&fmt_line(&tok(&m.sig), 1));
                    }
                }
            }
            Item::Impl(i) => {
                let self_ty = tok(&*i.self_ty);
                let header = if let Some((_, tr, _)) = &i.trait_ {
                    format!("impl {} for {self_ty}", tok(tr))
                } else {
                    format!("impl {self_ty}")
                };
                out.push_str(&fmt_line(&header, 0));
                for item in &i.items {
                    if let ImplItem::Fn(m) = item {
                        let vis = tok(&m.vis);
                        let sig = tok(&m.sig);
                        out.push_str(&fmt_line(&format!("{vis}{sig}"), 1));
                    }
                }
            }
            Item::Type(t) => {
                let vis = tok(&t.vis);
                let name = &t.ident;
                let ty = tok(&*t.ty);
                out.push_str(&fmt_line(&format!("{vis}type {name} = {ty}"), 0));
            }
            Item::Const(c) => {
                let vis = tok(&c.vis);
                let name = &c.ident;
                let ty = tok(&*c.ty);
                out.push_str(&fmt_line(&format!("{vis}const {name}: {ty}"), 0));
            }
            Item::Mod(m) if m.content.is_some() => {
                let vis = tok(&m.vis);
                out.push_str(&fmt_line(&format!("{vis}mod {}", m.ident), 0));
            }
            _ => {}
        }
    }

    Ok(out)
}

fn tok<T: quote::ToTokens>(node: &T) -> String {
    normalize(node.to_token_stream().to_string())
}

/// Collapse token-stream whitespace noise into readable code.
fn normalize(s: String) -> String {
    // quote adds spaces around most punctuation; do minimal cleanup
    s.replace(" :: ", "::")
     .replace("& '", "&'")
     .replace(" ' ", "'")
     .replace("< ", "<")
     .replace(" >", ">")
     .replace("( ", "(")
     .replace(" )", ")")
     .replace(", )", ")")
}

fn fmt_line(s: &str, indent: usize) -> String {
    let prefix = "  ".repeat(indent);
    format!("{prefix}{}\n", s.trim())
}
