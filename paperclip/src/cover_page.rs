// src/cover_page.rs
// Generates a cover page for a single source document using lopdf.
//
// Cover page layout:
//   File Name  : RHH-HDR-AR-DRG-H200000 Drawing Title (B).pdf
//   Revision   : B
//   Start Page : 3
//   End Page   : 7
//   Date/Time  : 2026-05-26 14:32:00

use anyhow::Result;
use lopdf::content::{Content, Operation};
use lopdf::{Dictionary, Document, Object, Stream};

// --- Page dimensions (in points, 1mm = 2.835pt) --------------------------
const A4_WIDTH_PT: f64  = 595.0;   // 210mm
const A4_HEIGHT_PT: f64 = 842.0;   // 297mm

// --- Layout (in points from bottom-left) ---------------------------------
const MARGIN_PT: f64      = 56.0;   // ~20mm
const LABEL_X_PT: f64     = 56.0;   // left edge of label column
const VALUE_X_PT: f64     = 170.0;  // left edge of value column (~60mm)
const TITLE_Y_PT: f64     = 720.0;  // Y position of the title row
const FIRST_ROW_Y_PT: f64 = 680.0;  // Y position of first data row
const ROW_SPACING_PT: f64 = 34.0;   // vertical gap between rows (~12mm)
const FONT_SIZE_TITLE: f64 = 14.0;
const FONT_SIZE_LABEL: f64 = 11.0;

/// All the data needed to render one cover page.
pub struct CoverPageData<'a> {
    pub filename:   &'a str,
    pub revision:   &'a str,
    pub start_page: u32,
    pub end_page:   u32,
    pub datetime:   &'a str,
}

/// Generates a cover page and returns it as a lopdf::Document.
/// The caller merges this document's page into the binder.
pub fn generate(data: &CoverPageData) -> Result<Document> {
    let mut doc = Document::with_version("1.5");

    // --- Font references -------------------------------------------------
    // PDF built-in fonts are referenced by name — no embedding needed.
    // We declare them in the page's resource dictionary.
    let font_regular = Dictionary::from_iter(vec![
        ("Type",     Object::Name(b"Font".to_vec())),
        ("Subtype",  Object::Name(b"Type1".to_vec())),
        ("BaseFont", Object::Name(b"Helvetica".to_vec())),
    ]);
    let font_bold = Dictionary::from_iter(vec![
        ("Type",     Object::Name(b"Font".to_vec())),
        ("Subtype",  Object::Name(b"Type1".to_vec())),
        ("BaseFont", Object::Name(b"Helvetica-Bold".to_vec())),
    ]);

    // Add fonts to the document and get their object IDs
    let font_regular_id = doc.add_object(Object::Dictionary(font_regular));
    let font_bold_id    = doc.add_object(Object::Dictionary(font_bold));

    // --- Build content stream --------------------------------------------
    // PDF content streams are sequences of operators.
    // Each Operation is (operator_name, [operands]).
    // This is equivalent to writing PostScript-style drawing commands.
    let mut ops: Vec<Operation> = Vec::new();

    // Set text colour to dark grey for title
    ops.push(op("rg", vec![0.2.into(), 0.2.into(), 0.2.into()]));  // fill colour RGB

    // Title
    write_text(
        &mut ops, "Document Cover Sheet",
        "F2", FONT_SIZE_TITLE,  // F2 = bold font (see resources dict below)
        MARGIN_PT, TITLE_Y_PT,
    );

    // Divider line
    ops.push(op("w", vec![0.5.into()]));                             // line width
    ops.push(op("RG", vec![0.7.into(), 0.7.into(), 0.7.into()]));   // stroke colour
    ops.push(op("m", vec![MARGIN_PT.into(), (FIRST_ROW_Y_PT + ROW_SPACING_PT * 1.2).into()]));
    ops.push(op("l", vec![(A4_WIDTH_PT - MARGIN_PT).into(), (FIRST_ROW_Y_PT + ROW_SPACING_PT * 1.2).into()]));
    ops.push(op("S", vec![]));  // stroke the path

    // Property rows
    let rows: Vec<(&str, String)> = vec![
        ("File Name",  data.filename.to_string()),
        ("Revision",   data.revision.to_string()),
        ("Start Page", data.start_page.to_string()),
        ("End Page",   data.end_page.to_string()),
        ("Date/Time",  data.datetime.to_string()),
    ];

    for (i, (label, value)) in rows.iter().enumerate() {
        let y = FIRST_ROW_Y_PT - (i as f64 * ROW_SPACING_PT);

        // Label in bold, grey
        ops.push(op("rg", vec![0.4.into(), 0.4.into(), 0.4.into()]));
        write_text(&mut ops, label, "F2", FONT_SIZE_LABEL, LABEL_X_PT, y);

        // Value in regular, black
        ops.push(op("rg", vec![0.0.into(), 0.0.into(), 0.0.into()]));
        write_text(&mut ops, value, "F1", FONT_SIZE_LABEL, VALUE_X_PT, y);
    }

    // Serialise the operations into a PDF content stream
    let content = Content { operations: ops };
    let content_bytes = content.encode()
        .map_err(|e| anyhow::anyhow!("Failed to encode content stream: {}", e))?;

    let content_stream = Stream::new(Dictionary::new(), content_bytes);
    let content_id = doc.add_object(Object::Stream(content_stream));

    // --- Resource dictionary ---------------------------------------------
    // Tells the page which fonts are available, keyed by the names
    // we used in the content stream (F1, F2)
    let resources = Dictionary::from_iter(vec![
        ("Font", Object::Dictionary(Dictionary::from_iter(vec![
            ("F1", Object::Reference(font_regular_id)),
            ("F2", Object::Reference(font_bold_id)),
        ]))),
    ]);

    // --- Page dictionary -------------------------------------------------
    let page_dict = Dictionary::from_iter(vec![
        ("Type",      Object::Name(b"Page".to_vec())),
        ("MediaBox",  Object::Array(vec![
            0.into(), 0.into(),
            A4_WIDTH_PT.into(), A4_HEIGHT_PT.into(),
        ])),
        ("Contents",  Object::Reference(content_id)),
        ("Resources", Object::Dictionary(resources)),
    ]);

    // --- Page tree -------------------------------------------------------
    // PDF requires a Pages tree even for a single page
    let page_id    = doc.add_object(Object::Dictionary(page_dict));
    let pages_dict = Dictionary::from_iter(vec![
        ("Type",  Object::Name(b"Pages".to_vec())),
        ("Kids",  Object::Array(vec![Object::Reference(page_id)])),
        ("Count", Object::Integer(1)),
    ]);
    let pages_id = doc.add_object(Object::Dictionary(pages_dict));

    // Point the page back to its parent
    if let Ok(Object::Dictionary(d)) = doc.get_object_mut(page_id) {
        d.set("Parent", Object::Reference(pages_id));
    }

    // Set the document catalog
    let catalog = Dictionary::from_iter(vec![
        ("Type",  Object::Name(b"Catalog".to_vec())),
        ("Pages", Object::Reference(pages_id)),
    ]);
    let catalog_id = doc.add_object(Object::Dictionary(catalog));
    doc.trailer.set("Root", Object::Reference(catalog_id));

    Ok(doc)
}

// --- Helpers -------------------------------------------------------------

/// Shorthand for creating a lopdf Operation.
/// operator: PDF operator string e.g. "Tf", "Td", "Tj"
/// operands: list of Object values
fn op(operator: &str, operands: Vec<Object>) -> Operation {
    Operation::new(operator, operands)
}

/// Appends PDF text operations to position and draw a string.
/// font_key: "F1" (regular) or "F2" (bold) — must match resource dict
fn write_text(
    ops: &mut Vec<Operation>,
    text: &str,
    font_key: &str,
    size: f64,
    x: f64,
    y: f64,
) {
    ops.push(op("BT", vec![]));  // Begin Text block
    ops.push(op("Tf", vec![     // Set font and size
        Object::Name(font_key.as_bytes().to_vec()),
        size.into(),
    ]));
    ops.push(op("Td", vec![x.into(), y.into()]));  // Move to position
    ops.push(op("Tj", vec![                         // Show text string
        Object::String(text.as_bytes().to_vec(), lopdf::StringFormat::Literal),
    ]));
    ops.push(op("ET", vec![]));  // End Text block
}