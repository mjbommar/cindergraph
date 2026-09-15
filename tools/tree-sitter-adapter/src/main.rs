use std::env;
use std::io::{self, Read};
use std::time::Instant;

use serde_json::{Value, json};
use tree_sitter::{Node, Parser};

const TREE_SITTER_VERSION: &str = "0.27.0";
const TREE_SITTER_C_VERSION: &str = "0.24.2";

fn function_name(node: Node<'_>, source: &[u8]) -> String {
    let Some(declarator) = node.child_by_field_name("declarator") else {
        return String::new();
    };
    let mut stack = vec![declarator];
    while let Some(current) = stack.pop() {
        if current.kind() == "identifier" {
            return String::from_utf8_lossy(&source[current.byte_range()]).into_owned();
        }
        for index in (0..current.child_count()).rev() {
            if let Some(child) = current.child(index) {
                stack.push(child);
            }
        }
    }
    String::new()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if env::args().nth(1).as_deref() == Some("--version") {
        println!(
            "cindergraph-robustness-tree-sitter 0.1.0; tree-sitter {TREE_SITTER_VERSION}; tree-sitter-c {TREE_SITTER_C_VERSION}"
        );
        return Ok(());
    }
    if env::args().len() != 1 {
        return Err("this worker accepts source bytes on standard input only".into());
    }

    let mut source = Vec::new();
    io::stdin().read_to_end(&mut source)?;
    let mut parser = Parser::new();
    let language = tree_sitter_c::LANGUAGE.into();
    parser.set_language(&language)?;
    let started = Instant::now();
    let tree = parser
        .parse(&source, None)
        .ok_or("parser returned no tree")?;
    let parse_ns = u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX);
    let root = tree.root_node();

    let mut nodes: Vec<Value> = Vec::new();
    let mut edges: Vec<Value> = Vec::new();
    let mut functions: Vec<Value> = Vec::new();
    let mut error_nodes = 0_u64;
    let mut missing_nodes = 0_u64;
    let mut stack = vec![(root, None)];
    while let Some((node, parent)) = stack.pop() {
        if node.is_error() {
            error_nodes += 1;
        }
        if node.is_missing() {
            missing_nodes += 1;
        }
        let projected = node.is_named() || node.is_error() || node.is_missing();
        let id = nodes.len();
        let next_parent = if projected {
            nodes.push(json!({
                "id": id,
                "kind": node.kind(),
                "start": node.start_byte(),
                "end": node.end_byte(),
                "error": node.is_error(),
                "missing": node.is_missing(),
            }));
            if let Some(parent_id) = parent {
                edges.push(json!({"source": parent_id, "target": id}));
            }
            Some(id)
        } else {
            parent
        };
        if node.kind() == "function_definition" {
            functions.push(json!({
                "name": function_name(node, &source),
                "start": node.start_byte(),
                "end": node.end_byte(),
            }));
        }
        for index in (0..node.child_count()).rev() {
            if let Some(child) = node.child(index) {
                stack.push((child, next_parent));
            }
        }
    }

    let document = json!({
        "schema": 1,
        "tree_sitter_version": TREE_SITTER_VERSION,
        "grammar_version": TREE_SITTER_C_VERSION,
        "language_abi": language.abi_version(),
        "parse_ns": parse_ns,
        "has_error": root.has_error(),
        "error_nodes": error_nodes,
        "missing_nodes": missing_nodes,
        "root_end_byte": root.end_byte(),
        "functions": functions,
        "nodes": nodes,
        "edges": edges,
    });
    serde_json::to_writer(io::stdout().lock(), &document)?;
    Ok(())
}
