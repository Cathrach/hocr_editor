use eframe::egui;
use egui::{FontData, FontDefinitions, FontFamily};
use itertools::Itertools;
use lazy_static::lazy_static;
use rfd::FileDialog;
use scraper::{ElementRef, Html, Selector};
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::read_to_string;
use std::path::PathBuf;
use std::str::FromStr;

// global "constants" for egui stuff
lazy_static! {
    static ref OCR_SELECTOR: Selector =
        Selector::parse(".ocr_page, .ocr_carea, .ocr_line, .ocr_par, .ocrx_word, .ocr_caption, .ocr_separator, .ocr_photo").unwrap();
    static ref OCR_WORD_SELECTOR: Selector = Selector::parse(".ocrx_word").unwrap();
    static ref OCR_PAGE_SELECTOR: Selector = Selector::parse(".ocr_page").unwrap();
    static ref UNCLICKED_STROKE : egui::Stroke = egui::Stroke::new(STROKE_WEIGHT, egui::Color32::LIGHT_BLUE);
    static ref CLICKED_STROKE : egui::Stroke = egui::Stroke::new(STROKE_WEIGHT, egui::Color32::BLACK);
    static ref FOCUS_FILL: egui::Color32 = egui::Color32::LIGHT_BLUE.gamma_multiply(0.3);
}

fn main() {
    let options = eframe::NativeOptions::default();
    let _ = eframe::run_native(
        "HOCR Editor",
        options,
        Box::new(|cc| Box::new(HOCREditor::new(cc))),
    );
}

type InternalID = u32;

// main struct: the state of our app
#[derive(Default, Debug)]
struct HOCREditor {
    file_path: Option<PathBuf>,
    html_tree: Option<Html>,
    image_path: Option<String>,
    selected_id: RefCell<String>,
    file_path_changed: bool,
    internal_ocr_tree: RefCell<Tree<OCRElement>>,
}

#[derive(Default, Debug)]
struct IntPos2 {
    x: u32,
    y: u32,
}

impl IntPos2 {
    fn to_pos2(&self) -> egui::Pos2 {
        egui::Pos2 {
            x: self.x as f32,
            y: self.y as f32,
        }
    }
}

#[derive(Default, Debug)]
struct BBox {
    top_left: IntPos2,
    bottom_right: IntPos2,
}

impl BBox {
    fn to_rect(&self) -> egui::Rect {
        egui::Rect {
            min: self.top_left.to_pos2(),
            max: self.bottom_right.to_pos2(),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct ParseError;

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

// when you select the bbox, you change select_id to assoc_id
struct SelectableRect {
    adj_bbox: egui::Rect,
    selected: bool,
}

impl SelectableRect {
    fn new(adj_bbox: egui::Rect, selected: bool) -> Self {
        Self {
            adj_bbox: adj_bbox,
            selected: selected,
        }
    }
}

const STROKE_WEIGHT: f32 = 4.0;
const UNFOCUS_FILL: egui::Color32 = egui::Color32::TRANSPARENT;

impl egui::Widget for SelectableRect {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let Self { adj_bbox, selected } = self;
        let response = ui.allocate_rect(adj_bbox, egui::Sense::click());
        let stroke: egui::Stroke = if selected {
            *CLICKED_STROKE
        } else {
            *UNCLICKED_STROKE
        };
        let fill: egui::Color32 = if response.hovered() || selected {
            *FOCUS_FILL
        } else {
            UNFOCUS_FILL
        };
        // TODO: widgetinfo
        if ui.is_rect_visible(response.rect) {
            ui.painter()
                .rect(adj_bbox, egui::Rounding::ZERO, fill, stroke);
        }
        response.on_hover_and_drag_cursor(egui::CursorIcon::PointingHand)
    }
}

// this mimics selectable_value in egui but adapts it to SelectableRect instead of SelectableLabel
fn selectable_rect<Value: PartialEq>(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    current_value: &mut Value,
    selected_value: Value,
) -> egui::Response {
    let mut response = ui.add(SelectableRect::new(rect, *current_value == selected_value));
    if response.clicked() && *current_value != selected_value {
        *current_value = selected_value;
        response.mark_changed();
    }
    response
}

#[derive(Debug)]
enum OCRProperty {
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
struct OCRElement {
    html_element_type: String,
    ocr_element_type: OCRClass,
    // id: String, // these will be auto-generated during HTML writing
    ocr_properties: HashMap<String, OCRProperty>,
    ocr_text: String,
    ocr_lang: Option<String>, // only ocr_par has lang I think
}

#[derive(Default, Debug)]
enum OCRClass {
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

// the "tree" is a dictionary of IDs to nodes
#[derive(Default, Debug)]
struct Tree<D> {
    nodes: HashMap<InternalID, Node<D>>,
    roots: Vec<InternalID>,
    curr_id: InternalID,
}

#[derive(Debug)]
// a node has a value, a parent (an ID), and children (a vector of IDs)
// yes, removing and inserting are O(n), but whatever, I need order to be preserved
struct Node<D> {
    value: D,
    parent: Option<InternalID>,
    children: Vec<InternalID>,
    id: InternalID,
}

enum Position {
    Before,
    After,
}

impl<D> Tree<D> {
    // return an empty tree
    fn new() -> Self {
        Tree {
            nodes: HashMap::new(),
            roots: Vec::new(),
            curr_id: 0,
        }
    }

    // add a node as a root
    fn add_root(&mut self, root: D) -> InternalID {
        let id = self.curr_id;
        self.nodes.insert(
            id,
            Node {
                value: root,
                parent: None,
                children: Vec::new(),
                id: id,
            },
        );
        self.roots.push(id);
        self.curr_id += 1;
        id
    }

    // add a child to the end of id's children
    fn push_child(&mut self, id: &InternalID, child: D) -> Option<InternalID> {
        if let Some(parent) = self.nodes.get_mut(id) {
            let new_id = self.curr_id;
            parent.children.push(new_id);
            self.nodes.insert(
                new_id,
                Node {
                    value: child,
                    parent: Some(*id),
                    children: Vec::new(),
                    id: new_id,
                },
            );
            self.curr_id += 1;
            return Some(new_id);
        }
        None
    }

    // add a sibling to a node
    fn add_sibling(&mut self, id: &InternalID, sibling: D, pos: Position) -> Option<InternalID> {
        // if id exists, find node's parent
        // if node's parent doesn't exist, add a root
        // if node's parent exists
        // insert sibling into the hash map
        // insert sibling's ID into the parent's child vector before id
        if let Some(node) = self.nodes.get(id) {
            if let Some(par_id) = node.parent {
                let new_id = self.curr_id;
                self.nodes.insert(
                    new_id,
                    Node {
                        value: sibling,
                        parent: Some(par_id),
                        children: Vec::new(),
                        id: new_id,
                    },
                );
                self.curr_id += 1;
                let par_child_index = self
                    .nodes
                    .get(&par_id)
                    .unwrap()
                    .children
                    .binary_search(id)
                    .unwrap();
                let insert_index = par_child_index
                    + match pos {
                        Position::After => 1,
                        Position::Before => 0,
                    };
                self.nodes
                    .get_mut(&par_id)
                    .unwrap()
                    .children
                    .insert(insert_index, *id);
                return Some(new_id);
            } else {
                return Some(self.add_root(sibling));
            }
        } else {
            None
        }
    }

    // get a (ref to) node value by ID -- wrapper around hash map function
    fn get_node(&self, id: &InternalID) -> Option<&D> {
        match self.nodes.get(id) {
            Some(node) => Some(&node.value),
            None => None,
        }
    }

    // mutable ref to node val by ID -- used when we need to modify bbox or text
    fn get_mut_node(&mut self, id: &InternalID) -> Option<&mut D> {
        match self.nodes.get_mut(id) {
            Some(node) => Some(&mut node.value),
            None => None,
        }
    }

    // this is only a helper! never call it outside!
    fn delete_child_from_parent(&mut self, par_id: &InternalID, child_id: &InternalID) {
        let par = self.nodes.get_mut(par_id).unwrap();
        let index = par.children.binary_search(child_id).unwrap();
        par.children.remove(index);
    }

    // helper for delete_node
    // this doesn't disconnect a node from its parent, it just recursively removes a node and its children
    // any node passed in here will just get removed from the hashmap
    // it returns whether the node actually existed and the parent ID for use in delete_node
    fn delete_rec_node(&mut self, id: &InternalID) -> (bool, Option<InternalID>) {
        let removed = self.nodes.remove(id);
        if let Some(node) = removed {
            for child in node.children {
                self.delete_rec_node(&child);
            }
            return (true, node.parent);
        }
        return (false, None);
    }

    // delete a node from the tree. This ALSO DELETES ITS CHILDREN!
    fn delete_node(&mut self, id: &InternalID) {
        // remove the node and its children from hashmap
        let (existed, parent_id) = self.delete_rec_node(id);
        if existed {
            match parent_id {
                // node is a root
                None => {
                    let index = self.roots.binary_search(id).unwrap();
                    self.roots.remove(index);
                }
                Some(par_id) => self.delete_child_from_parent(&par_id, id),
            }
        }
    }
}

fn parse_properties(title_content: &str) -> HashMap<String, OCRProperty> {
    let mut property_dict = HashMap::new();
    for pattern in title_content.split_terminator("; ") {
        println!("{}", pattern);
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
                },
                "ppageno" => Some(OCRProperty::UInt(suffix.parse::<u32>().unwrap())),
                "scan_res" => {
                    let parts: Vec<&str> = suffix.splitn(2, " ").collect();
                    Some(OCRProperty::ScanRes(
                        parts[0].parse::<u32>().unwrap(),
                        parts[1].parse::<u32>().unwrap(),
                    ))
                },
                "x_size" | "x_descenders" | "x_ascenders" => {
                    Some(OCRProperty::Float(suffix.parse::<f32>().unwrap()))
                },
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
            parse_properties(text)
        } else {
            HashMap::new()
        },
        ocr_text: if OCR_WORD_SELECTOR.matches(&elt) {
            get_root_text(elt)
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

fn add_children_to_ocr_tree(elt_ref: ElementRef, par_id: u32, tree: &mut Tree<OCRElement>) {
    for child in elt_ref.children() {
        if let Some(child_ref) = ElementRef::wrap(child) {
            if OCR_SELECTOR.matches(&child_ref) {
                let added_id = tree.push_child(&par_id, html_elt_to_ocr_elt(child_ref));
                if let Some(add_id) = added_id {
                    add_children_to_ocr_tree(child_ref, add_id, tree);
                }
            }
        }
    }
}

fn html_to_ocr_tree(html_tree: scraper::Html) -> Tree<OCRElement> {
    // recursively walk the html_tree starting from the root html node
    // look through all children
    // if child matches an OCR selector, it is a root
    // then walk through chlidren matching an OCR selector of roots, etc.
    let mut tree: Tree<OCRElement> = Tree::new();
    // TODO: don't just grab ocr_pages
    for page_elt in html_tree.select(&OCR_PAGE_SELECTOR) {
        let root_id = tree.add_root(html_elt_to_ocr_elt(page_elt));
        add_children_to_ocr_tree(page_elt, root_id, &mut tree);
    }
    tree
}

fn load_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();

    fonts.font_data.insert(
        String::from("Japanese"),
        FontData::from_static(include_bytes!("resources/NotoSansJP-Regular.ttf")),
    );
    fonts
        .families
        .get_mut(&FontFamily::Proportional)
        .unwrap()
        .push("Japanese".to_owned());

    ctx.set_fonts(fonts);
}

fn get_root_preview_text(root: scraper::ElementRef) -> String {
    let mut count = 0;
    let mut s = String::new();
    for text in root.text() {
        // if text is entirely whitespace, skip
        if !text.trim().is_empty() {
            if count == 0 {
                s.push_str(text.trim_start());
            } else {
                s.push_str(text);
            }
            count += 1;
        }
        if count >= 2 {
            break;
        }
    }
    s.push_str("...");
    s
}

fn get_root_text(root: scraper::ElementRef) -> String {
    root.text().filter(|s| !s.trim().is_empty()).join("")
}

fn get_image(root: scraper::ElementRef) -> String {
    let ocr_props = root.value().attr("title").unwrap();
    let mut ret = String::from("file://");
    for pattern in ocr_props.split_terminator(";") {
        if let Some((prefix, suffix)) = pattern.split_once(" ") {
            if prefix == "image" {
                ret.push_str(suffix.trim_matches('"'));
                return ret;
            }
        }
    }
    // TODO: error handle
    return ret;
}

fn get_bbox(root: scraper::ElementRef) -> BBox {
    let ocr_props = root.value().attr("title").unwrap();
    for pattern in ocr_props.split_terminator(";") {
        if let Some((prefix, suffix)) = pattern.split_once(" ") {
            if prefix == "bbox" {
                let coords: Vec<u32> = suffix
                    .split(" ")
                    .map(|s| u32::from_str(s).unwrap())
                    .collect();
                return BBox {
                    top_left: IntPos2 {
                        x: coords[0],
                        y: coords[1],
                    },
                    bottom_right: IntPos2 {
                        x: coords[2],
                        y: coords[3],
                    },
                };
            }
        }
    }
    return BBox::default();
}

impl HOCREditor {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        load_fonts(&cc.egui_ctx);
        egui_extras::install_image_loaders(&cc.egui_ctx);
        println!("Installed image loaders?");
        Self::default()
    }
    fn get_ocr_pages(&self) -> Vec<ElementRef<'_>> {
        if let Some(html_tree) = &self.html_tree {
            let ocr_pages = html_tree.select(&OCR_PAGE_SELECTOR);
            return ocr_pages.collect::<Vec<ElementRef<'_>>>();
        }
        Vec::new()
    }
    fn get_selected_elt(&self) -> Option<ElementRef<'_>> {
        if !self.selected_id.borrow().is_empty() {
            let selector = String::from("#") + &self.selected_id.borrow();
            let id_sel = Selector::parse(selector.as_str()).unwrap();
            if let Some(html_tree) = &self.html_tree {
                let mut found_elt = html_tree.select(&id_sel);
                return found_elt.next();
            }
        }
        return None;
    }

    // TODO: rename
    fn render_tree(&self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            for ocr_page in self.get_ocr_pages() {
                // call renderTreeForRoot on each ocr_page
                // note that the HOCR specification says that ocr_page MUST be present
                self.render_tree_for_root(ocr_page, ui);
            }
        });
    }
    // TODO: rename
    fn render_tree_for_root(&self, root: scraper::ElementRef, ui: &mut egui::Ui) {
        // check if root matches the ocr_page, etc. selector
        if OCR_SELECTOR.matches(&root) {
            let ocr_type: OCRClass = root
                .value()
                .attr("class")
                .expect("No class!")
                .parse()
                .unwrap();
            let label_text = format!("{}: {}", ocr_type.to_string(), get_root_preview_text(root));
            let label_id = root.value().attr("id").expect("No ID");
            if !OCR_WORD_SELECTOR.matches(&root) {
                // if it is not a word, turn root into a
                // - collapsible header whose header indicates its class and value (selectable value here)
                let id = ui.make_persistent_id(label_id);
                egui::collapsing_header::CollapsingState::load_with_default_open(
                    ui.ctx(),
                    id,
                    false,
                )
                .show_header(ui, |ui| {
                    // ui.label(label_text)
                    ui.selectable_value(
                        &mut *self.selected_id.borrow_mut(),
                        label_id.to_string(),
                        label_text,
                    );
                })
                // - body created by recursively calling renderTree on the children
                .body(|ui| {
                    for child in root.children() {
                        if let Some(child_elt) = scraper::ElementRef::wrap(child) {
                            self.render_tree_for_root(child_elt, ui);
                        }
                    }
                });
            } else {
                ui.selectable_value(
                    &mut *self.selected_id.borrow_mut(),
                    label_id.to_string(),
                    get_root_text(root),
                );
                // if it is a word, turn root into a selectable value
                // label: type (word, carea, par, etc.) preview text
            }
        }
    }
}

impl eframe::App for HOCREditor {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::SidePanel::right("HOCR Tree").show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.heading("HOCR Tree");
            });

            self.render_tree(ui);
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            if ui.button("Open HOCR File").clicked() {
                self.file_path = FileDialog::new()
                    .add_filter("hocr", &["html", "xml", "hocr"])
                    .pick_file();
                self.file_path_changed = true;
            }

            // let's not re-parse the file every frame
            if self.file_path_changed {
                if let Some(path) = &self.file_path {
                    let html_buffer = read_to_string(path).expect("Failed to read file");
                    self.html_tree = Some(Html::parse_document(&html_buffer));
                    if !self.get_ocr_pages().is_empty() {
                        self.image_path = Some(get_image(self.get_ocr_pages()[0]));
                    }
                    self.file_path_changed = false;
                    if let Some(tree) = &self.html_tree {
                        self.internal_ocr_tree = RefCell::new(html_to_ocr_tree(tree.clone()));
                        println!("{:?}", self.internal_ocr_tree);
                    }
                }
            }

            // ui.label(format!("Selected ID: {}", self.selected_id.borrow()));
            if let Some(image_path) = &self.image_path {
                egui::ScrollArea::both().show(ui, |ui| {
                    // ui.image(image_path);
                    let response =
                        ui.add(egui::Image::from_uri(image_path).fit_to_original_size(1.0));
                    // if we have a selected ID, draw bboxes for it and its siblings
                    if let Some(elt) = self.get_selected_elt() {
                        let offset = response.rect.min.to_vec2();
                        selectable_rect(
                            ui,
                            get_bbox(elt).to_rect().translate(offset),
                            &mut *self.selected_id.borrow_mut(),
                            elt.value().attr("id").unwrap().to_string(),
                        );
                        for sib_elt in elt.prev_siblings().chain(elt.next_siblings()) {
                            if let Some(sibling_elt) = scraper::ElementRef::wrap(sib_elt) {
                                selectable_rect(
                                    ui,
                                    get_bbox(sibling_elt).to_rect().translate(offset),
                                    &mut *self.selected_id.borrow_mut(),
                                    sibling_elt.value().attr("id").unwrap().to_string(),
                                );
                            }
                        }
                    }
                });
                // TODO: center the bbox when we make the bbox widget using scroll_to_me
            }
        });
    }
}
