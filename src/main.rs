use crate::ocr_element::get_root_preview_text;
use crate::ocr_element::get_root_text;
use eframe::egui;
use egui::{FontData, FontDefinitions, FontFamily};
use lazy_static::lazy_static;
use rfd::FileDialog;
use scraper::{ElementRef, Html, Selector};
use std::cell::RefCell;

use std::fs::read_to_string;
use std::path::PathBuf;
use std::str::FromStr;

use crate::ocr_element::{BBox, IntPos2, OCRClass, OCRElement};
use crate::ocr_element::{OCR_PAGE_SELECTOR, OCR_SELECTOR, OCR_WORD_SELECTOR};
use crate::tree::Tree;
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

fn get_bbox(root: scraper::ElementRef) -> ocr_element::BBox {
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

    fn reparse_file(&mut self) {
        if let Some(path) = &self.file_path {
            let html_buffer = read_to_string(path).expect("Failed to read file");
            self.html_tree = Some(Html::parse_document(&html_buffer));
            if !self.get_ocr_pages().is_empty() {
                self.image_path = Some(get_image(self.get_ocr_pages()[0]));
            }
            self.file_path_changed = false;
            if let Some(tree) = &self.html_tree {
                self.internal_ocr_tree = RefCell::new(OCRElement::html_to_ocr_tree(tree.clone()));
                println!("{:?}", self.internal_ocr_tree);
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
                self.reparse_file();
            }
            self.draw_img_and_bboxes(ui);
        });
    }
}
