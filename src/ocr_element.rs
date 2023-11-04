use crate::tree::Tree;
use crate::InternalID;
use eframe::egui;
use html5ever::interface::tree_builder::TreeSink;
use html5ever::interface::ElementFlags;
use html5ever::interface::{AppendNode, AppendText};
use html5ever::{local_name, namespace_prefix, namespace_url, ns};
use html5ever::{Attribute, LocalName, QualName};
use itertools::Itertools;

use lazy_static::lazy_static;
use scraper::{ElementRef, Selector};
use std::{collections::HashMap, path::PathBuf, str::FromStr};

lazy_static! {
    pub static ref OCR_SELECTOR: Selector =
        Selector::parse(".ocr_page, .ocr_carea, .ocr_line, .ocr_par, .ocrx_word, .ocr_caption, .ocr_separator, .ocr_photo").unwrap();
    pub static ref OCR_WORD_SELECTOR: Selector = Selector::parse(".ocrx_word").unwrap();
    pub static ref OCR_PAGE_SELECTOR: Selector = Selector::parse(".ocr_page").unwrap();
}

#[derive(Default, Debug)]
pub struct IntPos2 {
    pub x: u32,
    pub y: u32,
}

impl IntPos2 {
    pub fn to_pos2(&self) -> egui::Pos2 {
        egui::Pos2 {
            x: self.x as f32,
            y: self.y as f32,
        }
    }
}

#[derive(Default, Debug)]
pub struct BBox {
    pub top_left: IntPos2,
    pub bottom_right: IntPos2,
}

impl BBox {
    pub fn to_rect(&self) -> egui::Rect {
        egui::Rect {
            min: self.top_left.to_pos2(),
            max: self.bottom_right.to_pos2(),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct ParseError;

impl FromStr for BBox {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let coords: Vec<&str> = s.trim().split(" ").collect();
        if coords.len() >= 4 {
            let x_fromstr = coords[0].parse::<u32>().map_err(|_| ParseError)?;
            let y_fromstr = coords[1].parse::<u32>().map_err(|_| ParseError)?;
            let z_fromstr = coords[2].parse::<u32>().map_err(|_| ParseError)?;
            let w_fromstr = coords[3].parse::<u32>().map_err(|_| ParseError)?;

            return Ok(BBox {
                top_left: IntPos2 {
                    x: x_fromstr,
                    y: y_fromstr,
                },
                bottom_right: IntPos2 {
                    x: z_fromstr,
                    y: w_fromstr,
                },
            });
        } else {
            return Err(ParseError);
        }
    }
}

#[derive(Debug)]
pub enum OCRProperty {
    BBox(BBox),
    Image(PathBuf),
    Float(f32),
    UInt(u32),
    Int(i32),
    Baseline(f32, f32),
    ScanRes(u32, u32),
}

impl OCRProperty {
    fn to_str(&self) -> String {
        match self {
            OCRProperty::BBox(bbox) => format!(
                "{} {} {} {}",
                bbox.top_left.x, bbox.top_left.y, bbox.bottom_right.x, bbox.bottom_right.y
            ),
            OCRProperty::Image(path) => format!(r#""{}""#, path.display()),
            OCRProperty::Float(f) => f.to_string(),
            OCRProperty::UInt(u) => u.to_string(),
            OCRProperty::Int(u) => u.to_string(),
            OCRProperty::Baseline(f1, f2) => format!("{} {}", f1, f2),
            OCRProperty::ScanRes(f1, f2) => format!("{} {}", f1, f2),
        }
    }
}

// internal representation of a node in the HTML tree containing OCR data
// TODO: transform the html tree into a tree of these
// TODO: subclasses because page, word, line have different properties
#[derive(Default, Debug)]
pub struct OCRElement {
    pub html_element_type: String,
    pub ocr_element_type: OCRClass,
    // id: String, // these will be auto-generated during HTML writing
    pub ocr_properties: HashMap<String, OCRProperty>,
    pub ocr_text: String,
    pub ocr_lang: Option<String>, // only ocr_par has lang I think
}

impl OCRElement {
    fn add_children_to_ocr_tree(elt_ref: ElementRef, par_id: u32, tree: &mut Tree<OCRElement>) {
        for child in elt_ref.children() {
            if let Some(child_ref) = ElementRef::wrap(child) {
                if OCR_SELECTOR.matches(&child_ref) {
                    let added_id = tree.push_child(&par_id, Self::html_elt_to_ocr_elt(child_ref));
                    if let Some(add_id) = added_id {
                        Self::add_children_to_ocr_tree(child_ref, add_id, tree);
                    }
                }
            }
        }
    }

    fn get_root_text(root: scraper::ElementRef) -> String {
        root.text().filter(|s| !s.trim().is_empty()).join("")
    }

    fn html_elt_to_ocr_elt(elt: ElementRef) -> OCRElement {
        let mut ocr_class = "";
        // assumes this element matcehs the OCR selector
        for class in elt.value().classes() {
            if class.starts_with("ocr") {
                ocr_class = class;
            }
        }
        // TODO: exit gracefully if parsing fails

        OCRElement {
            html_element_type: elt.value().name().to_string(),
            ocr_element_type: ocr_class.parse().unwrap(),
            ocr_properties: if let Some(text) = elt.value().attr("title") {
                OCRProperty::parse_properties(text)
            } else {
                HashMap::new()
            },
            ocr_text: if OCR_WORD_SELECTOR.matches(&elt) {
                Self::get_root_text(elt)
            } else {
                String::new()
            },
            ocr_lang: if let Some(lang) = elt.value().attr("lang") {
                Some(lang.to_string())
            } else {
                None
            },
        }
    }

    pub fn html_to_ocr_tree(html_tree: scraper::Html) -> Tree<OCRElement> {
        // recursively walk the html_tree starting from the root html node
        // look through all children
        // if child matches an OCR selector, it is a root
        // then walk through chlidren matching an OCR selector of roots, etc.
        let mut tree: Tree<OCRElement> = Tree::new();
        // TODO: don't just grab ocr_pages
        for page_elt in html_tree.select(&OCR_PAGE_SELECTOR) {
            let root_id = tree.add_root(Self::html_elt_to_ocr_elt(page_elt));
            Self::add_children_to_ocr_tree(page_elt, root_id, &mut tree);
        }
        tree
    }

    fn to_html_elt_with_id(&self, html_id: String) -> String {
        let mut props = Vec::new();
        for (name, prop) in self.ocr_properties.iter() {
            props.push(format!("{} {}", name, prop.to_str()));
        }
        format!(
            r#"<{} class='{}' id='{}' title="{}">{}"#,
            self.html_element_type,
            self.ocr_element_type.to_string(),
            html_id,
            props.as_slice().join("; "),
            self.ocr_text,
        )
    }

    fn close_me(&self) -> String {
        format!("</{}>", self.html_element_type)
    }
}

#[derive(Default, Debug)]
pub enum OCRClass {
    #[default]
    Page,
    CArea,
    Par,
    Line,
    Word,
    Separator,
    Photo,
    Caption,
}

impl OCRClass {
    pub fn to_user_str(&self) -> String {
        match self {
            Self::CArea => "Area".to_string(),
            Self::Page => "Page".to_string(),
            Self::Line => "Line".to_string(),
            Self::Par => "Par".to_string(),
            Self::Word => "Word".to_string(),
            Self::Photo => "Photo".to_string(),
            Self::Separator => "Separator".to_string(),
            Self::Caption => "Caption".to_string(),
        }
    }
    pub fn to_id_str(&self) -> String {
        match self {
            Self::CArea | Self::Separator | Self::Photo => "block".to_string(),
            Self::Page => "page".to_string(),
            Self::Line | Self::Caption => "line".to_string(),
            Self::Par => "par".to_string(),
            Self::Word => "word".to_string(),
        }
    }
}

impl FromStr for OCRClass {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ocr_page" => Ok(Self::Page),
            "ocr_carea" => Ok(Self::CArea),
            "ocr_line" => Ok(Self::Line),
            "ocr_par" => Ok(Self::Par),
            "ocrx_word" => Ok(Self::Word),
            "ocr_photo" => Ok(Self::Photo),
            "ocr_separator" => Ok(Self::Separator),
            "ocr_caption" => Ok(Self::Caption),
            _ => Err(()),
        }
    }
}

impl ToString for OCRClass {
    fn to_string(&self) -> String {
        match self {
            Self::CArea => "ocr_carea".to_string(),
            Self::Page => "ocr_page".to_string(),
            Self::Line => "ocr_line".to_string(),
            Self::Par => "ocr_par".to_string(),
            Self::Word => "ocrx_word".to_string(),
            Self::Photo => "ocr_photo".to_string(),
            Self::Separator => "ocr_separator".to_string(),
            Self::Caption => "ocr_caption".to_string(),
        }
    }
}

impl OCRProperty {
    pub fn parse_properties(title_content: &str) -> HashMap<String, OCRProperty> {
        let mut property_dict = HashMap::new();
        for pattern in title_content.split_terminator("; ") {
            // println!("{}", pattern);
            if let Some((prefix, suffix)) = pattern.split_once(" ") {
                let trimmed = prefix.trim();
                let ocr_prop = match trimmed {
                    "image" => Some(OCRProperty::Image(PathBuf::from(suffix.trim_matches('"')))),
                    "bbox" => Some(OCRProperty::BBox(BBox::from_str(suffix).unwrap())),
                    "baseline" => {
                        let parts: Vec<&str> = suffix.splitn(2, " ").collect();
                        Some(OCRProperty::Baseline(
                            parts[0].parse::<f32>().unwrap(),
                            parts[1].parse::<f32>().unwrap(),
                        ))
                    }
                    "ppageno" => Some(OCRProperty::UInt(suffix.parse::<u32>().unwrap())),
                    "scan_res" => {
                        let parts: Vec<&str> = suffix.splitn(2, " ").collect();
                        Some(OCRProperty::ScanRes(
                            parts[0].parse::<u32>().unwrap(),
                            parts[1].parse::<u32>().unwrap(),
                        ))
                    }
                    "x_size" | "x_descenders" | "x_ascenders" => {
                        Some(OCRProperty::Float(suffix.parse::<f32>().unwrap()))
                    }
                    "x_wconf" => Some(OCRProperty::UInt(suffix.parse::<u32>().unwrap())),
                    _ => None,
                };
                if !ocr_prop.is_none() {
                    property_dict.insert(trimmed.to_string(), ocr_prop.unwrap());
                }
            }
        }
        property_dict
    }
}

pub fn add_as_body(tree: &Tree<OCRElement>, html_head: &scraper::Html) -> scraper::Html {
    let mut html_final = html_head.clone();
    // debug
    // TODO: this guy doesn't have the doctype or XML comment
    println!("head of cloned: {}", html_final.html());
    let mut ids = HashMap::<String, u32>::new();
    ids.insert("page".to_string(), 1);
    ids.insert("block".to_string(), 1);
    ids.insert("par".to_string(), 1);
    ids.insert("line".to_string(), 1);
    ids.insert("word".to_string(), 1);
    // add body element to html
    let html_id = html_final.root_element().id();
    let body_id = html_final.create_element(
        QualName::new(None, ns!(html), local_name!("body")),
        Vec::new(),
        Default::default(),
    );
    html_final.append(&html_id, AppendNode(body_id));
    // now add the roots
    for root in tree.roots() {
        add_ocr_tree(&tree, root, &mut ids, &mut html_final, &body_id);
    }
    html_final
}

// add node as a child of parent in html
fn add_ocr_tree(
    tree: &Tree<OCRElement>,
    node: &InternalID,
    ids: &mut HashMap<String, u32>,
    html: &mut scraper::Html,
    parent_id: &ego_tree::NodeId,
) {
    if let Some(n) = tree.get_node(node) {
        let type_id = n.ocr_element_type.to_id_str();
        let curr_no = *ids.get(&type_id).unwrap();
        ids.insert(type_id.clone(), curr_no + 1);
        let html_id = if type_id == "page" {
            format! {"page_{}", curr_no}
        } else {
            format!("{}_{}_{}", type_id, *ids.get("page").unwrap() - 1, curr_no)
        };
        let mut props = Vec::new();
        for (name, prop) in n.ocr_properties.iter() {
            props.push(format!("{} {}", name, prop.to_str()));
        }
        let mut attrs: Vec<Attribute> = Vec::new();
        attrs.push(Attribute {
            name: QualName::new(None, ns!(), local_name!("title")),
            value: props.as_slice().join("; ").into(),
        });
        attrs.push(Attribute {
            name: QualName::new(None, ns!(), local_name!("id")),
            value: html_id.into(),
        });
        attrs.push(Attribute {
            name: QualName::new(None, ns!(), local_name!("class")),
            value: n.ocr_element_type.to_string().into(),
        });
        if let Some(lang) = &n.ocr_lang {
            attrs.push(Attribute {
                name: QualName::new(None, ns!(), local_name!("lang")),
                value: lang.as_str().into(),
            });
        }

        // s.push_str(&n.close_me())
        let child_id = html.create_element(
            QualName::new(
                None,
                ns!(html),
                LocalName::from(n.html_element_type.as_str()),
            ),
            attrs,
            Default::default(),
        );
        html.append(parent_id, AppendNode(child_id));
        // push text as chlid if needed
        if !n.ocr_text.is_empty() {
            html.append(&child_id, AppendText(n.ocr_text.as_str().into()));
        }
        // s.push_str(&n.to_html_elt_with_id(html_id));
        // then serialize my chlidren
        for child in tree.children(node) {
            add_ocr_tree(tree, child, ids, html, &child_id);
            // s.push_str(&serialize_me_and_children(tree, child, ids));
        }
    }
}
