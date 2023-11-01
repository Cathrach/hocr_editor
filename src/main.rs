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

lazy_static! {
    static ref OCR_SELECTOR: Selector =
        Selector::parse(".ocr_page, .ocr_carea, .ocr_line, .ocr_par, .ocrx_word, .ocr_caption, .ocr_separator, .ocr_photo").unwrap();
    static ref OCR_WORD_SELECTOR: Selector = Selector::parse(".ocrx_word").unwrap();
    static ref OCR_PAGE_SELECTOR: Selector = Selector::parse(".ocr_page").unwrap();
}

fn main() {
    let options = eframe::NativeOptions::default();
    let _ = eframe::run_native(
        "HOCR Editor",
        options,
        Box::new(|cc| Box::new(HOCREditor::new(cc))),
    );
}

#[derive(Default)]
struct HOCREditor {
    file_path: Option<PathBuf>,
    html_tree: Option<Html>,
    image_path: Option<String>,
    selected_id: RefCell<String>,
    file_path_changed: bool,
}

struct IntPos2 {
    x: u32,
    y: u32,
}

struct BBox {
    top_left: IntPos2,
    bottom_right: IntPos2,
}

enum OCRProperty {
    BBox(BBox),
    Image(PathBuf),
    Float(f32),
    UInt(u32),
    Int(i32),
    Baseline(f32, i32),
    ScanRes(u32, u32),
}

// internal representation of a node in the HTML tree containing OCR data
// TODO: transform the html tree into a tree of these
// TODO: subclasses because page, word, line have different properties
struct OCRElement<'a> {
    html_element_type: String,
    ocr_element_type: OCRClass,
    id: String,
    ocr_properties: HashMap<String, OCRProperty>,
    ocr_text: String,
    ocr_lang: Option<String>, // only ocr_par has lang I think
    parent: Option<&'a OCRElement<'a>>,
    children: Vec<OCRElement<'a>>,
}

enum OCRClass {
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
    for pattern in ocr_props.split_terminator(";") {
        if let Some((prefix, suffix)) = pattern.split_once(" ") {
            if prefix == "image" {
                return suffix.to_string();
            }
        }
    }
    // TODO: error handle
    return String::new();
}

impl HOCREditor {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        load_fonts(&cc.egui_ctx);
        egui_extras::install_image_loaders(&cc.egui_ctx);
        Self::default()
    }
    fn get_ocr_pages(&self) -> Vec<ElementRef<'_>> {
        if let Some(html_tree) = &self.html_tree {
            let ocr_pages = html_tree.select(&OCR_PAGE_SELECTOR);
            return ocr_pages.collect::<Vec<ElementRef<'_>>>();
        }
        Vec::new()
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
                // if it is a word, turn root into a selectable value
                // label: type (word, carea, par, etc.) preview text
                ui.label(get_root_text(root));
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
                }
            }

            ui.label(format!("Selected ID: {}", self.selected_id.borrow()));
            if let Some(image_path) = &self.image_path {
                ui.image(image_path);
            }
        });
    }
}
