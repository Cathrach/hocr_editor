use crate::ocr_element::{OCRClass, OCRElement, OCRProperty};
use crate::tree::{Position, Tree};
use crate::Mode::Select;
use eframe::egui;
use egui::CursorIcon::{ResizeHorizontal, ResizeNeSw, ResizeNwSe, ResizeVertical};
use egui::{FontData, FontDefinitions, FontFamily, Pos2, Rect, Sense, Vec2};
use html5ever::interface::tree_builder::TreeSink;
use html5ever::interface::AppendNode;
use html5ever::interface::ElementFlags;
use html5ever::{namespace_url, ns};
use lazy_static::lazy_static;
use rfd::FileDialog;
use scraper::Node::*;
use scraper::Selector;
use scraper::{ElementRef, Html};
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::read_to_string;
use std::path::PathBuf;

mod ocr_element;
mod tree;

// global "constants" for egui stuff
lazy_static! {
    static ref UNCLICKED_STROKE: egui::Stroke =
        egui::Stroke::new(STROKE_WEIGHT, egui::Color32::LIGHT_BLUE);
    static ref CLICKED_STROKE: egui::Stroke =
        egui::Stroke::new(STROKE_WEIGHT, egui::Color32::BLACK);
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

// TODO: do I need this?
#[derive(Default, Debug, PartialEq)]
enum Mode {
    #[default]
    Select,
    Edit,
}

// main struct: the state of our app
#[derive(Debug)]
struct HOCREditor {
    file_path: Option<PathBuf>,
    html_write_head: Html,
    image_path: Option<String>,
    file_path_changed: bool,
    internal_ocr_tree: RefCell<Tree<OCRElement>>,
    mode: Mode,
    // to allow the rendered tree to interact with state
    // we update these first
    // then when we detect updates we update the tree
    selected_id: RefCell<Option<InternalID>>,
    merge_id: RefCell<Option<InternalID>>,
    merge_position: RefCell<Position>,
    parent_id: RefCell<Option<InternalID>>,
    sibling_id: RefCell<Option<InternalID>>,
    sibling_position: RefCell<Position>,
}

impl Default for HOCREditor {
    fn default() -> Self {
        HOCREditor {
            file_path: None,
            html_write_head: Html::new_document(),
            merge_id: RefCell::new(None),
            merge_position: RefCell::new(Position::Before),
            file_path_changed: false,
            internal_ocr_tree: RefCell::new(Default::default()),
            mode: Default::default(),
            parent_id: RefCell::new(None),
            sibling_id: RefCell::new(None),
            sibling_position: RefCell::new(Position::Before),
            image_path: None,
            selected_id: RefCell::new(None),
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
        Self { adj_bbox, selected }
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

impl HOCREditor {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        load_fonts(&cc.egui_ctx);
        egui_extras::install_image_loaders(&cc.egui_ctx);
        Self::default()
    }
    /*
    fn get_selected_elt(&self) -> Option<&OCRElement> {
        self.internal_ocr_tree.borrow().get_node(self.selected_id.borrow().deref())
    }
    */

    fn update_internal_tree(&self) {
        self.merge();
        self.make_new_sibling();
        self.make_new_child();
    }

    fn make_new_child(&self) {
        if let Some(id) = *self.parent_id.borrow() {
            // child bbox should be parent bbox
            let bbox = self
                .internal_ocr_tree
                .borrow()
                .get_node(&id)
                .expect(format!("id {} doesn't exist in tree", id).as_str())
                .ocr_properties
                .get("bbox")
                .expect(format!("node {} doesn't have a bbox", id).as_str())
                .clone();
            let mut properties = HashMap::new();
            properties.insert("bbox".to_string(), bbox);
            self.internal_ocr_tree.borrow_mut().push_child(
                &id,
                OCRElement {
                    html_element_type: "span".to_string(),
                    ocr_element_type: OCRClass::Word,
                    ocr_properties: properties,
                    ocr_text: "".to_string(),
                    ocr_lang: None,
                },
            );
        }
        *self.parent_id.borrow_mut() = None;
    }

    fn make_new_sibling(&self) {
        if let Some(id) = *self.sibling_id.borrow() {
            let sibling = self
                .internal_ocr_tree
                .borrow()
                .get_node(&id)
                .expect(format!("sibling id {} doesn't exist in tree", id).as_str())
                .clone();
            self.internal_ocr_tree.borrow_mut().add_sibling(
                &id,
                sibling,
                &*self.sibling_position.borrow(),
            );
        }
        *self.sibling_id.borrow_mut() = None;
    }

    fn merge(&self) {
        if let Some(id) = *self.merge_id.borrow() {
            // reparent children of old node
            self.internal_ocr_tree
                .borrow_mut()
                .merge_sibling(&id, &*self.merge_position.borrow());
        }
        *self.merge_id.borrow_mut() = None;
    }

    // TODO: rename
    fn render_tree(&self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            for root in self.internal_ocr_tree.borrow().roots() {
                // call renderTreeForRoot on each ocr_page
                // note that the HOCR specification says that ocr_page MUST be present
                self.render_tree_for_root(*root, ui);
            }
        });
    }
    // TODO: rename
    fn render_tree_for_root(&self, root: InternalID, ui: &mut egui::Ui) {
        let ocr_tree = self.internal_ocr_tree.borrow();
        if let Some(elt) = ocr_tree.get_node(&root) {
            let label_text = format!("{}{}", elt.ocr_element_type.to_user_str(), {
                let s = ocr_element::get_root_preview_text(&*ocr_tree, root);
                if !s.is_empty() {
                    format! {": {}", s}
                } else {
                    s
                }
            },);
            if ocr_tree.has_children(&root) {
                let id = ui.make_persistent_id(root);
                egui::collapsing_header::CollapsingState::load_with_default_open(
                    ui.ctx(),
                    id,
                    false,
                )
                .show_header(ui, |ui| {
                    // ui.label(label_text)
                    ui.selectable_value(
                        &mut *self.selected_id.borrow_mut(),
                        Some(root),
                        label_text,
                    )
                    .context_menu(|ui| {
                        if ui.button("Merge below").clicked() {
                            *self.merge_id.borrow_mut() = Some(root);
                            *self.merge_position.borrow_mut() = Position::After;
                        }
                        if ui.button("Merge above").clicked() {
                            *self.merge_id.borrow_mut() = Some(root);
                            *self.merge_position.borrow_mut() = Position::Before;
                        }
                        if ui.button("Sibling below").clicked() {
                            *self.sibling_id.borrow_mut() = Some(root);
                            *self.sibling_position.borrow_mut() = Position::After;
                        }
                        if ui.button("Sibling above").clicked() {
                            *self.sibling_id.borrow_mut() = Some(root);
                            *self.sibling_position.borrow_mut() = Position::Before;
                        }
                        if ui.button("New child").clicked() {
                            *self.parent_id.borrow_mut() = Some(root);
                        }
                    });
                })
                // - body created by recursively calling renderTree on the children
                .body(|ui| {
                    for child in ocr_tree.children(&root) {
                        self.render_tree_for_root(*child, ui);
                    }
                });
            } else {
                let childless_label_text = format!("{}{}", elt.ocr_element_type.to_user_str(), {
                    if !elt.ocr_text.is_empty() {
                        format! {": {}", elt.ocr_text}
                    } else {
                        String::new()
                    }
                });

                ui.selectable_value(
                    &mut *self.selected_id.borrow_mut(),
                    Some(root),
                    childless_label_text,
                )
                .context_menu(|ui| {
                    if ui.button("Merge below").clicked() {
                        *self.merge_id.borrow_mut() = Some(root);
                        *self.merge_position.borrow_mut() = Position::After;
                    }
                    if ui.button("Merge above").clicked() {
                        *self.merge_id.borrow_mut() = Some(root);
                        *self.merge_position.borrow_mut() = Position::Before;
                    }
                    if ui.button("Sibling below").clicked() {
                        *self.sibling_id.borrow_mut() = Some(root);
                        *self.sibling_position.borrow_mut() = Position::After;
                    }
                    if ui.button("Sibling above").clicked() {
                        *self.sibling_id.borrow_mut() = Some(root);
                        *self.sibling_position.borrow_mut() = Position::Before;
                    }
                    if ui.button("New child").clicked() {
                        *self.parent_id.borrow_mut() = Some(root);
                    }
                });
            }
        }
    }

    fn reparse_file(&mut self) {
        if let Some(path) = &self.file_path {
            let html_buffer = read_to_string(path).expect("Failed to read file");
            let mut html_tree = Html::parse_document(&html_buffer);
            // read the ocr parts into an internal tree
            self.internal_ocr_tree = RefCell::new(OCRElement::html_to_ocr_tree(html_tree.clone()));
            // set the path of the displayed image
            // TODO: actually make the loop do smth instead of just outputting last image
            for root_id in self.internal_ocr_tree.borrow().roots() {
                if let Some(ocr_prop) = self
                    .internal_ocr_tree
                    .borrow()
                    .get_node(root_id)
                    .expect(
                        format!(
                            "{} was marked as root id but doesn't exist in tree",
                            root_id
                        )
                        .as_str(),
                    )
                    .ocr_properties
                    .get("image")
                {
                    match ocr_prop {
                        OCRProperty::Image(path) => {
                            let mut s = String::from("file://");
                            s.push_str(path.as_str());
                            self.image_path = Some(s);
                        }
                        _ => (),
                    }
                }
            }
            self.file_path_changed = false;
            // copy over the xml, doctype, and head into a new html document
            let doc = html_tree.get_document();
            // copy over the html node first
            let root = html_tree.root_element().value();
            let html_id = self.html_write_head.create_element(
                root.name.clone(),
                root.attrs().map(|tup| create_attr(tup)).collect(),
                Default::default(),
            );
            for child in html_tree
                .tree
                .get(doc)
                .expect("HTML Tree didn't have document node")
                .children()
            {
                match child.value() {
                    Doctype(doc_node) => {
                        println!("Found doctype {:?}", doc_node);
                        self.html_write_head.append_doctype_to_document(
                            doc_node.name.clone(),
                            doc_node.public_id.clone(),
                            doc_node.system_id.clone(),
                        );
                    }
                    ProcessingInstruction(pi) => {
                        println!("Found PI {:?}", pi);
                        self.html_write_head
                            .create_pi(pi.target.clone(), pi.data.clone());
                    }
                    Comment(comment) => {
                        println!("Found comment {:?}", comment);
                        let c_id = self.html_write_head.create_comment(comment.comment.clone());
                        self.html_write_head.append(&doc, AppendNode(c_id));
                    }
                    _ => println!("Debug extra node: {:?}", child.value()),
                };
            }
            self.html_write_head.append(&doc, AppendNode(html_id));
            if let Some(head) = html_tree.select(&Selector::parse("head").unwrap()).next() {
                let root_elt_id = self.html_write_head.root_element().id();
                append_elt_tree(&mut self.html_write_head, &root_elt_id, head);
            }
        }
    }

    // TODO: return the rect we drew if successful
    fn draw_bbox(&self, offset: egui::Vec2, elt_id: &InternalID, ui: &mut egui::Ui) {
        if let Some(node) = self.internal_ocr_tree.borrow().get_node(elt_id) {
            if let OCRProperty::BBox(bbox) = node
                .ocr_properties
                .get("bbox")
                .expect(format!("Node {} doesn't have a bbox", elt_id).as_str())
            {
                let egui_rect = bbox.translate(offset);
                selectable_rect(
                    ui,
                    egui_rect,
                    &mut *self.selected_id.borrow_mut(),
                    Some(*elt_id),
                );
            }
        }
    }

    fn draw_img_and_bboxes(&mut self, ui: &mut egui::Ui) {
        // ui.label(format!("Selected ID: {}", self.selected_id.borrow()));
        if let Some(image_path) = &self.image_path {
            egui::ScrollArea::both().show(ui, |ui| {
                // ui.image(image_path);
                let response = ui.add(egui::Image::from_uri(image_path).fit_to_original_size(1.0));
                // if we have a selected ID, draw bboxes for it and its siblings
                if self.selected_id.borrow().is_some() {
                    let elt = self.selected_id.borrow().unwrap();
                    let offset = response.rect.min.to_vec2();
                    // self.draw_bbox(offset, &elt, ui);
                    if let Some(node) = self.internal_ocr_tree.borrow_mut().get_mut_node(&elt) {
                        if let Some(OCRProperty::BBox(bbox)) = node.ocr_properties.get_mut("bbox") {
                            let egui_rect = bbox.translate(offset);
                            // sense drags around the border of the rect
                            // sense drags in any direction around the corners
                            //                 let point_rect = Rect::from_center_size(point_in_screen, size);
                            //                 let point_id = response.id.with(i);
                            //                 let point_response = ui.interact(point_rect, point_id, Sense::drag());
                            //
                            //                 *point += point_response.drag_delta();
                            //                 *point = to_screen.from().clamp(*point);
                            let top_left = Pos2 {
                                x: egui_rect.left(),
                                y: egui_rect.top(),
                            };
                            let top_right = Pos2 {
                                x: egui_rect.right(),
                                y: egui_rect.top(),
                            };
                            let bottom_left = Pos2 {
                                x: egui_rect.left(),
                                y: egui_rect.bottom(),
                            };
                            let bottom_right = Pos2 {
                                x: egui_rect.right(),
                                y: egui_rect.bottom(),
                            };
                            // TODO: is this a good size?
                            let size = Vec2::splat(16.0);
                            let top_left_rect = Rect::from_center_size(top_left, size);
                            let top_right_rect = Rect::from_center_size(top_right, size);
                            let bottom_left_rect = Rect::from_center_size(bottom_left, size);
                            let bottom_right_rect = Rect::from_center_size(bottom_right, size);
                            let top_left_id = response.id.with(0);
                            let top_right_id = response.id.with(1);
                            let bottom_left_id = response.id.with(2);
                            let bottom_right_id = response.id.with(3);
                            let top_left_response = ui
                                .interact(top_left_rect, top_left_id, Sense::drag())
                                .on_hover_and_drag_cursor(ResizeNwSe);
                            let top_right_response = ui
                                .interact(top_right_rect, top_right_id, Sense::drag())
                                .on_hover_and_drag_cursor(ResizeNeSw);
                            let bottom_left_response = ui
                                .interact(bottom_left_rect, bottom_left_id, Sense::drag())
                                .on_hover_and_drag_cursor(ResizeNeSw);
                            let bottom_right_response = ui
                                .interact(bottom_right_rect, bottom_right_id, Sense::drag())
                                .on_hover_and_drag_cursor(ResizeNwSe);
                            // sense drags in only vertical or horiz at the sides
                            let top_rect = Rect::from_min_max(
                                top_left + Vec2 { x: 8.0, y: -8.0 },
                                top_right + Vec2 { x: -8.0, y: 8.0 },
                            );
                            let bottom_rect = Rect::from_min_max(
                                bottom_left + Vec2 { x: 8.0, y: -8.0 },
                                bottom_right + Vec2 { x: -8.0, y: 8.0 },
                            );
                            let left_rect = Rect::from_min_max(
                                top_left + Vec2 { x: -8.0, y: 8.0 },
                                bottom_left + Vec2 { x: 8.0, y: -8.0 },
                            );
                            let right_rect = Rect::from_min_max(
                                top_right + Vec2 { x: -8.0, y: -8.0 },
                                bottom_right + Vec2 { x: 8.0, y: 8.0 },
                            );
                            let top_id = response.id.with(4);
                            let bottom_id = response.id.with(5);
                            let left_id = response.id.with(6);
                            let right_id = response.id.with(7);
                            let top_response = ui
                                .interact(top_rect, top_id, Sense::drag())
                                .on_hover_and_drag_cursor(ResizeVertical);
                            let right_response = ui
                                .interact(right_rect, right_id, Sense::drag())
                                .on_hover_and_drag_cursor(ResizeHorizontal);
                            let left_response = ui
                                .interact(left_rect, left_id, Sense::drag())
                                .on_hover_and_drag_cursor(ResizeHorizontal);
                            let bottom_response = ui
                                .interact(bottom_rect, bottom_id, Sense::drag())
                                .on_hover_and_drag_cursor(ResizeVertical);
                            bbox.min.x = (bbox.min.x
                                + top_left_response.drag_delta().x
                                + bottom_left_response.drag_delta().x
                                + left_response.drag_delta().x)
                                .max(0.0);
                            bbox.min.y = (bbox.min.y
                                + top_left_response.drag_delta().y
                                + top_right_response.drag_delta().y
                                + top_response.drag_delta().y)
                                .max(0.0);
                            bbox.max.x = (bbox.max.x
                                + top_right_response.drag_delta().x
                                + bottom_right_response.drag_delta().x
                                + right_response.drag_delta().x)
                                .max(0.0);
                            bbox.max.y = (bbox.max.y
                                + bottom_left_response.drag_delta().y
                                + bottom_right_response.drag_delta().y
                                + bottom_response.drag_delta().y)
                                .max(0.0);
                        }
                    }
                    self.draw_bbox(offset, &elt, ui);
                    // only draw siblings if we are selecting
                    if self.mode == Mode::Select {
                        for sib_elt in self
                            .internal_ocr_tree
                            .borrow()
                            .prev_siblings(&elt)
                            .chain(self.internal_ocr_tree.borrow().next_siblings(&elt))
                        {
                            self.draw_bbox(offset, sib_elt, ui);
                        }
                    }
                    // if we are editing, allow the bbox to be draggable
                }
            });
        }
    }

    fn open_file(&mut self) {
        self.file_path = FileDialog::new()
            .add_filter("hocr", &["html", "xml", "hocr"])
            .pick_file();
        self.file_path_changed = true;
    }

    fn save_file(&self) {
        if let Some(path) = &self.file_path {
            let new_path = path.with_file_name("test.html");
            let _ = std::fs::write(
                new_path,
                ocr_element::add_as_body(&self.internal_ocr_tree.borrow(), &self.html_write_head)
                    .html(),
            );
        }
    }

    fn delete_selected(&mut self) {
        let mut next_sib = None;
        if let Some(elt) = *self.selected_id.borrow() {
            next_sib = self.internal_ocr_tree.borrow().next_sibling(&elt);
            self.internal_ocr_tree.borrow_mut().delete_node(&elt);
        }
        *self.selected_id.borrow_mut() = next_sib;
    }
}

impl eframe::App for HOCREditor {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Open").clicked() {
                        self.open_file();
                        ui.close_menu();
                    }
                    if ui.button("Save").clicked() {
                        self.save_file();
                        ui.close_menu();
                    }
                })
            })
        });
        if let Some(elt) = *self.selected_id.borrow() {
            if self.mode == Mode::Select {
                if let Some(node) = self.internal_ocr_tree.borrow().get_node(&elt) {
                    egui::SidePanel::left("OCR Properties").show(ctx, |ui| {
                        egui::Grid::new("properties grid")
                            .num_columns(2)
                            .spacing([40.0, 4.0])
                            .striped(true)
                            .show(ui, |ui| {
                                ui.label("Type");
                                ui.label(node.ocr_element_type.to_user_str());
                                ui.end_row();
                                for (name, prop) in &node.ocr_properties {
                                    ui.label(name);
                                    ui.add(egui::Label::new(prop.to_str()).wrap(true));
                                    // ui.label(prop.to_str());
                                    ui.end_row();
                                }
                                if node.ocr_element_type == OCRClass::Word {
                                    ui.label("text");
                                    ui.label(&node.ocr_text);
                                    ui.end_row();
                                }
                            });
                    });
                }
            } else if self.mode == Mode::Edit {
                if let Some(node) = self.internal_ocr_tree.borrow_mut().get_mut_node(&elt) {
                    egui::SidePanel::left("OCR Properties").show(ctx, |ui| {
                        egui::Grid::new("properties grid")
                            .num_columns(2)
                            .spacing([40.0, 4.0])
                            .striped(true)
                            .show(ui, |ui| {
                                ui.label("Type");
                                egui::ComboBox::from_id_source("Type")
                                    .selected_text(node.ocr_element_type.to_user_str())
                                    .show_ui(ui, |ui| {
                                        for variant in OCRClass::variants() {
                                            ui.selectable_value(
                                                &mut node.ocr_element_type,
                                                variant.clone(),
                                                variant.to_user_str(),
                                            );
                                        }
                                    });
                                ui.end_row();
                                for (name, prop) in node.ocr_properties.iter_mut() {
                                    ui.label(name);
                                    match prop {
                                        OCRProperty::BBox(egui::Rect {
                                            min: egui::Pos2 { x: min_x, y: min_y },
                                            max: egui::Pos2 { x: max_x, y: max_y },
                                        }) => {
                                            ui.vertical(|ui| {
                                                ui.horizontal(|ui| {
                                                    ui.add(
                                                        egui::DragValue::new(min_x)
                                                            .speed(0.1)
                                                            .prefix("tl x: "),
                                                    );
                                                    ui.add(
                                                        egui::DragValue::new(min_y)
                                                            .speed(0.1)
                                                            .prefix("tl y: "),
                                                    );
                                                });
                                                ui.horizontal(|ui| {
                                                    ui.add(
                                                        egui::DragValue::new(max_x)
                                                            .speed(0.1)
                                                            .prefix("br x: "),
                                                    );
                                                    ui.add(
                                                        egui::DragValue::new(max_y)
                                                            .speed(0.1)
                                                            .prefix("br y: "),
                                                    );
                                                });
                                            });
                                        }
                                        OCRProperty::Image(path) => {
                                            ui.text_edit_singleline(path);
                                        }
                                        OCRProperty::Float(f) => {
                                            ui.add(egui::DragValue::new(f).speed(0.1));
                                        }
                                        OCRProperty::UInt(u) => {
                                            ui.add(egui::DragValue::new(u).speed(0.1));
                                        }
                                        OCRProperty::Int(i) => {
                                            ui.add(egui::DragValue::new(i).speed(0.1));
                                        }
                                        OCRProperty::Baseline(slope, con) => {
                                            ui.horizontal(|ui| {
                                                ui.add(
                                                    egui::DragValue::new(slope)
                                                        .speed(0.1)
                                                        .prefix("baseline slope: "),
                                                );
                                                ui.add(
                                                    egui::DragValue::new(con)
                                                        .speed(0.1)
                                                        .prefix("baseline y-int: "),
                                                );
                                            });
                                        }
                                        OCRProperty::ScanRes(dpi, dpi2) => {
                                            ui.horizontal(|ui| {
                                                ui.add(
                                                    egui::DragValue::new(dpi)
                                                        .speed(0.1)
                                                        .prefix("dpi: "),
                                                );
                                                ui.add(
                                                    egui::DragValue::new(dpi2)
                                                        .speed(0.1)
                                                        .prefix("also dpi?: "),
                                                );
                                            });
                                        }
                                    };
                                    ui.end_row();
                                }
                                // TODO: pressing delete here deletes the element! what should I do
                                if node.ocr_element_type == OCRClass::Word {
                                    ui.label("text");
                                    ui.text_edit_singleline(&mut node.ocr_text);
                                    ui.end_row();
                                }
                                // if editable, the numbers turn into drag values
                                // wconf is bounded by 0 and 100
                                // update while editing is false
                                // the text is textedit box for words
                            })
                    });
                }
            }
        }
        // TODO: you can also add a new property???
        egui::SidePanel::right("HOCR Tree").show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.heading("HOCR Tree");
            });

            self.render_tree(ui);
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            // let's not re-parse the file every frame
            if self.file_path_changed {
                self.reparse_file();
            }
            // for now: you can edit the selected bbox by pressing "e"
            if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::E)) {
                self.mode = Mode::Edit;
            }
            if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
                self.mode = Mode::Select;
            }
            // and if you've selected a word, you can edit the text by...
            self.draw_img_and_bboxes(ui);
            if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Backspace)) {
                self.delete_selected();
            }
        });
        self.update_internal_tree();
    }
}

fn create_attr(tup: (&str, &str)) -> html5ever::Attribute {
    html5ever::Attribute {
        // TODO: idk if this is the right ns!
        name: html5ever::QualName::new(None, ns!(), tup.0.into()),
        value: tup.1.into(),
    }
}

fn append_elt_tree(html: &mut Html, parent: &ego_tree::NodeId, elt: ElementRef) {
    // recursively calls append on a copied element
    // create attribute

    let id = html.create_element(
        elt.value().name.clone(),
        elt.value().attrs().map(|tup| create_attr(tup)).collect(),
        ElementFlags::default(),
    );
    html.append(parent, AppendNode(id));
    // now take the children and pass them in
    for child in elt.children() {
        if let Some(elt) = ElementRef::wrap(child) {
            append_elt_tree(html, &id, elt);
        }
    }
}
