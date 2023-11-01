use eframe::egui;
use egui::{FontData, FontDefinitions, FontFamily};
use itertools::Itertools;
use rfd::FileDialog;
use scraper::{ElementRef, Html, Selector};
use std::fs::read_to_string;
use std::path::PathBuf;
use std::str::FromStr;
use lazy_static::lazy_static;

lazy_static! {
    static ref OCR_SELECTOR: Selector = Selector::parse(".ocr_page, .ocr_carea, .ocr_line, .ocr_par, .ocrx_word").unwrap();
    static ref OCR_WORD_SELECTOR: Selector = Selector::parse(".ocrx_word").unwrap();
    static ref OCR_PAGE_SELECTOR: Selector = Selector::parse(".ocr_page").unwrap();
}

fn main() {
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "HOCR Editor",
        options,
        Box::new(|cc| Box::new(HOCREditor::new(cc))),
    );
}

struct HOCREditor {
    file_path: Option<PathBuf>,
    html_tree: Option<Html>,
    // selected_id: String,
}

// internal representation of a node in the HTML tree containing OCR data
// TODO: transform the html tree into a tree of these
// TODO: subclasses because page, word, line have different properties
struct OCRElement {
    element_type: OCRClass,
    // TODO: not use Rect because bounding boxes are integer only
    bounding_box: egui::Rect,
    // TODO: apparently lifetime parameter? IDK
    // element_ref: scraper::ElementRef,
}

enum OCRClass {
    Page,
    CArea,
    Par,
    Line,
    Word,
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

// create a selector for class = ocr_page, ocr_carea, ocr_par, ocr_line, or ocrx_word

impl HOCREditor {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        load_fonts(&cc.egui_ctx);
        HOCREditor {
            file_path: None,
            html_tree: None,
            // selected_id: String::default(),
        }
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
                self.render_tree_for_node(ocr_page, ui);
            }
        });
    }
    // TODO: rename
    fn render_tree_for_node(&self, root: scraper::ElementRef, ui: &mut egui::Ui) {
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
                    ui.label(label_text)
                    // ui.selectable_value(&mut self.selected_id, label_id.to_string(), label_text);
                })
                // - body created by recursively calling renderTree on the children
                .body(|ui| {
                    for child in root.children() {
                        if let Some(child_elt) = scraper::ElementRef::wrap(child) {
                            self.render_tree_for_node(child_elt, ui);
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
            }

            if let Some(path) = &self.file_path {
                ui.horizontal(|ui| {
                    ui.label("Picked file:");
                    ui.monospace(path.display().to_string());
                });
                let html_buffer = read_to_string(path).expect("Failed to read file");
                self.html_tree = Some(Html::parse_document(&html_buffer));
            }

            /*
            if let Some(html_tree) = &self.html_tree {
                let ocr_page_sel = Selector::parse(".ocr_page").unwrap();
                let input = html_tree.select(&ocr_page_sel).next().unwrap();
                ui.horizontal(|ui| {
                    if let Some(class) = input.value().attr("class") {
                        ui.label(class);
                    }
                    if let Some(title) = input.value().attr("title") {
                        ui.label(title);
                    } else {
                        ui.label("Couldn't find an ocr_page with title");
                    }
                });
            }
            */
        });
    }
}
