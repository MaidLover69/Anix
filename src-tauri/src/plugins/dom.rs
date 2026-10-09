//! The document behind `LoadDoc`: parsed HTML and the node-set operations of its selection API.
//! Selections live on the JavaScript side as lists of node handles; everything here works on
//! those handles.

use ego_tree::{NodeId, NodeRef};
use scraper::{ElementRef, Html, Node, Selector};
use std::collections::HashMap;

struct Document {
    html: Html,
    nodes: Vec<NodeId>,
    handles: HashMap<NodeId, u32>,
}

impl Document {
    fn handle(&mut self, id: NodeId) -> u32 {
        if let Some(handle) = self.handles.get(&id) {
            return *handle;
        }
        let handle = self.nodes.len() as u32;
        self.nodes.push(id);
        self.handles.insert(id, handle);
        handle
    }

    fn node(&self, handle: u32) -> Option<NodeRef<'_, Node>> {
        self.html.tree.get(*self.nodes.get(handle as usize)?)
    }
}

/// Documents of one plugin context, addressed by the handle `load` returned.
#[derive(Default)]
pub struct DomStore {
    documents: Vec<Document>,
}

fn parse_selector(selector: &str) -> Result<Selector, String> {
    Selector::parse(selector).map_err(|_| format!("invalid or unsupported selector '{}'", selector))
}

fn optional_selector(selector: &str) -> Result<Option<Selector>, String> {
    if selector.trim().is_empty() {
        Ok(None)
    } else {
        parse_selector(selector).map(Some)
    }
}

fn accepts(filter: &Option<Selector>, element: &ElementRef) -> bool {
    filter.as_ref().map_or(true, |selector| selector.matches(element))
}

fn element_siblings<'a>(
    node: NodeRef<'a, Node>,
    forward: bool,
) -> impl Iterator<Item = ElementRef<'a>> {
    let siblings: Box<dyn Iterator<Item = NodeRef<'a, Node>>> = if forward {
        Box::new(node.next_siblings())
    } else {
        Box::new(node.prev_siblings())
    };
    siblings.filter_map(ElementRef::wrap)
}

fn node_text(node: NodeRef<'_, Node>, out: &mut String) {
    for descendant in node.descendants() {
        if let Node::Text(text) = descendant.value() {
            out.push_str(text);
        }
    }
}

impl DomStore {
    pub fn load(&mut self, html: &str) -> u32 {
        self.documents.push(Document {
            html: Html::parse_document(html),
            nodes: Vec::new(),
            handles: HashMap::new(),
        });
        (self.documents.len() - 1) as u32
    }

    /// Forgets every document; handles given out before become empty selections.
    pub fn clear(&mut self) {
        self.documents.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.documents.is_empty()
    }

    /// Operations that produce a node set, in document order per input node and without
    /// duplicates.
    pub fn select(&mut self, doc: u32, op: &str, nodes: &[u32], arg: &str) -> Result<Vec<u32>, String> {
        let Some(document) = self.documents.get_mut(doc as usize) else {
            return Ok(Vec::new());
        };
        let mut found: Vec<NodeId> = Vec::new();
        {
            let document = &*document;
            let mut push = |id: NodeId| {
                if !found.contains(&id) {
                    found.push(id);
                }
            };
            let inputs: Vec<NodeRef<'_, Node>> =
                nodes.iter().filter_map(|handle| document.node(*handle)).collect();

            match op {
                "document" => push(document.html.tree.root().id()),
                "root" => {
                    let selector = parse_selector(arg)?;
                    for element in document.html.select(&selector) {
                        push(element.id());
                    }
                }
                "find" => {
                    let selector = parse_selector(arg)?;
                    for node in &inputs {
                        match ElementRef::wrap(*node) {
                            Some(element) => {
                                for hit in element.select(&selector) {
                                    push(hit.id());
                                }
                            }
                            None => {
                                for hit in node.descendants().filter_map(ElementRef::wrap) {
                                    if selector.matches(&hit) {
                                        push(hit.id());
                                    }
                                }
                            }
                        }
                    }
                }
                "children" => {
                    let filter = optional_selector(arg)?;
                    for node in &inputs {
                        for child in node.children().filter_map(ElementRef::wrap) {
                            if accepts(&filter, &child) {
                                push(child.id());
                            }
                        }
                    }
                }
                "contents" => {
                    for node in &inputs {
                        for child in node.children() {
                            push(child.id());
                        }
                    }
                }
                "parent" => {
                    let filter = optional_selector(arg)?;
                    for node in &inputs {
                        if let Some(parent) = node.parent().and_then(ElementRef::wrap) {
                            if accepts(&filter, &parent) {
                                push(parent.id());
                            }
                        }
                    }
                }
                "parents" => {
                    let filter = optional_selector(arg)?;
                    for node in &inputs {
                        for ancestor in node.ancestors().filter_map(ElementRef::wrap) {
                            if accepts(&filter, &ancestor) {
                                push(ancestor.id());
                            }
                        }
                    }
                }
                "parentsUntil" => {
                    let stop = parse_selector(arg)?;
                    for node in &inputs {
                        for ancestor in node.ancestors().filter_map(ElementRef::wrap) {
                            if stop.matches(&ancestor) {
                                break;
                            }
                            push(ancestor.id());
                        }
                    }
                }
                "closest" => {
                    let selector = parse_selector(arg)?;
                    for node in &inputs {
                        let hit = std::iter::once(*node)
                            .chain(node.ancestors())
                            .filter_map(ElementRef::wrap)
                            .find(|element| selector.matches(element));
                        if let Some(element) = hit {
                            push(element.id());
                        }
                    }
                }
                "next" | "prev" => {
                    let filter = optional_selector(arg)?;
                    for node in &inputs {
                        if let Some(sibling) = element_siblings(*node, op == "next").next() {
                            if accepts(&filter, &sibling) {
                                push(sibling.id());
                            }
                        }
                    }
                }
                "nextAll" | "prevAll" => {
                    let filter = optional_selector(arg)?;
                    for node in &inputs {
                        for sibling in element_siblings(*node, op == "nextAll") {
                            if accepts(&filter, &sibling) {
                                push(sibling.id());
                            }
                        }
                    }
                }
                "nextUntil" | "prevUntil" => {
                    let stop = parse_selector(arg)?;
                    for node in &inputs {
                        for sibling in element_siblings(*node, op == "nextUntil") {
                            if stop.matches(&sibling) {
                                break;
                            }
                            push(sibling.id());
                        }
                    }
                }
                "siblings" => {
                    let filter = optional_selector(arg)?;
                    for node in &inputs {
                        let Some(parent) = node.parent() else { continue };
                        for sibling in parent.children().filter_map(ElementRef::wrap) {
                            if sibling.id() != node.id() && accepts(&filter, &sibling) {
                                push(sibling.id());
                            }
                        }
                    }
                }
                "has" => {
                    let selector = parse_selector(arg)?;
                    for node in &inputs {
                        let holds = node
                            .descendants()
                            .skip(1)
                            .filter_map(ElementRef::wrap)
                            .any(|element| selector.matches(&element));
                        if holds {
                            push(node.id());
                        }
                    }
                }
                "filter" | "not" => {
                    let selector = parse_selector(arg)?;
                    for node in &inputs {
                        let matches = ElementRef::wrap(*node).map_or(false, |el| selector.matches(&el));
                        if matches == (op == "filter") {
                            push(node.id());
                        }
                    }
                }
                other => return Err(format!("unknown selection operation '{}'", other)),
            }
        }
        Ok(found.into_iter().map(|id| document.handle(id)).collect())
    }

    /// Operations that read from a selection. `None` is "no value" (a missing attribute).
    pub fn read(&self, doc: u32, op: &str, nodes: &[u32], arg: &str) -> Result<Option<String>, String> {
        let Some(document) = self.documents.get(doc as usize) else {
            return Ok(match op {
                "is" | "hasClass" => Some("0".to_string()),
                "text" | "html" | "outerHtml" => Some(String::new()),
                _ => None,
            });
        };
        let inputs: Vec<NodeRef<'_, Node>> =
            nodes.iter().filter_map(|handle| document.node(*handle)).collect();
        let first_element = inputs.iter().copied().find_map(ElementRef::wrap);
        let flag = |value: bool| Some(if value { "1" } else { "0" }.to_string());

        Ok(match op {
            "text" => {
                let mut out = String::new();
                for node in &inputs {
                    node_text(*node, &mut out);
                }
                Some(out)
            }
            "html" => Some(match inputs.first() {
                Some(node) => match ElementRef::wrap(*node) {
                    Some(element) => element.inner_html(),
                    None if matches!(node.value(), Node::Document) => document.html.html(),
                    None => String::new(),
                },
                None => String::new(),
            }),
            "outerHtml" => Some(match inputs.first() {
                Some(node) => match (ElementRef::wrap(*node), node.value()) {
                    (Some(element), _) => element.html(),
                    (None, Node::Text(text)) => text.to_string(),
                    (None, Node::Document) => document.html.html(),
                    _ => String::new(),
                },
                None => String::new(),
            }),
            "attr" => first_element.and_then(|element| element.attr(arg).map(String::from)),
            "attrs" => first_element.map(|element| {
                let map: serde_json::Map<String, serde_json::Value> = element
                    .value()
                    .attrs()
                    .map(|(name, value)| (name.to_string(), serde_json::Value::String(value.to_string())))
                    .collect();
                serde_json::Value::Object(map).to_string()
            }),
            "is" => {
                let selector = parse_selector(arg)?;
                flag(inputs
                    .iter()
                    .copied()
                    .filter_map(ElementRef::wrap)
                    .any(|element| selector.matches(&element)))
            }
            "hasClass" => flag(inputs.iter().copied().filter_map(ElementRef::wrap).any(|element| {
                element
                    .value()
                    .has_class(arg, scraper::CaseSensitivity::CaseSensitive)
            })),
            other => return Err(format!("unknown selection operation '{}'", other)),
        })
    }
}
