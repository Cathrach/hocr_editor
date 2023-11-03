use crate::tree::Tree;
use eframe::egui;
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
