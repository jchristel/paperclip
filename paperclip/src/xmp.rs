// src/xmp.rs
// Wraps the binder manifest JSON in an XMP metadata packet, compresses it
// with deflate, and attaches it to the PDF document catalog under /Metadata.
// Also provides the read side, used later by rename detection.
//
// Why XMP at all? It's the standard place for document-level metadata in a
// PDF (referenced from the catalog as /Metadata). Standard tooling expects a
// metadata stream to be an XMP packet, so we embed our JSON *inside* a valid
// XMP envelope as a custom property rather than dumping raw JSON. That keeps
// the stream well-formed for other readers while still being trivially
// recoverable by us.
//
// Why compress ourselves? The README notes a large binder's manifest can be
// ~100KB of JSON. We deflate it with flate2 and mark the PDF stream
// /Filter /FlateDecode, which is exactly what a deflate-compressed PDF stream
// declares. We compress explicitly (rather than hoping lopdf does it) so the
// behaviour is predictable and debuggable.

use anyhow::{bail, Context, Result};
use flate2::write::ZlibEncoder;
use flate2::read::ZlibDecoder;
use flate2::Compression;
use lopdf::{Dictionary, Document, Object, Stream};
use std::io::{Read, Write}; // brings write_all / read_to_string methods into scope

// --- XMP envelope --------------------------------------------------------

// The custom XML element we tuck our JSON into. We read it back out by
// slicing between the opening and closing tags, so these must stay in sync
// with `unwrap_xmp` below.
const JSON_OPEN_TAG: &str = "<paperclip:manifest>";
const JSON_CLOSE_TAG: &str = "</paperclip:manifest>";

/// Wraps a JSON string in a minimal but valid XMP packet.
///
/// The packet has the conventional <?xpacket?> header/trailer and an
/// rdf:RDF body. Our JSON lives inside a custom <paperclip:manifest> element.
/// We don't escape the JSON as XML entities; instead we keep it intact and
/// recover it by tag-slicing. (JSON has no `<` or `>` at the top level of our
/// data, and serde escapes string contents, so this is safe for our content.)
fn wrap_xmp(json: &str) -> String {
    format!(
        "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n\
<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n\
  <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n\
    <rdf:Description xmlns:paperclip=\"http://paperclip.local/ns/1.0/\">\n\
      {open}{json}{close}\n\
    </rdf:Description>\n\
  </rdf:RDF>\n\
</x:xmpmeta>\n\
<?xpacket end=\"w\"?>",
        open = JSON_OPEN_TAG,
        json = json,
        close = JSON_CLOSE_TAG,
    )
}

/// Recovers the JSON string from an XMP packet produced by `wrap_xmp`.
/// Returns Err if the custom element isn't found.
fn unwrap_xmp(xmp: &str) -> Result<String> {
    // Find the slice between our open and close tags.
    let start = xmp
        .find(JSON_OPEN_TAG)
        .context("XMP packet has no paperclip manifest element")?
        + JSON_OPEN_TAG.len();
    let end = xmp
        .find(JSON_CLOSE_TAG)
        .context("XMP packet manifest element is not closed")?;

    if end < start {
        bail!("XMP packet manifest tags are malformed");
    }

    Ok(xmp[start..end].to_string())
}

// --- Compression helpers -------------------------------------------------

/// Deflate-compresses bytes using zlib framing (what PDF FlateDecode expects).
fn deflate(bytes: &[u8]) -> Result<Vec<u8>> {
    // ZlibEncoder wraps a writer (here a Vec<u8>) and compresses on write.
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(bytes)
        .context("Failed to write bytes into deflate encoder")?;
    // finish() flushes remaining data and hands back the inner Vec.
    encoder.finish().context("Failed to finish deflate compression")
}

/// Inflates zlib-framed bytes back to the original.
fn inflate(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut decoder = ZlibDecoder::new(bytes);
    let mut out = Vec::new();
    decoder
        .read_to_end(&mut out)
        .context("Failed to inflate compressed XMP stream")?;
    Ok(out)
}

// --- Attach (write side) -------------------------------------------------

/// Builds the manifest JSON into an XMP packet, compresses it, and attaches
/// it to the document catalog as a /Metadata stream with /Filter /FlateDecode.
///
/// Call this on the assembled binder document before saving.
pub fn attach_manifest(doc: &mut Document, manifest_json: &str) -> Result<()> {
    // 1. JSON -> XMP packet -> UTF-8 bytes.
    let xmp = wrap_xmp(manifest_json);
    let raw = xmp.into_bytes();
    let uncompressed_len = raw.len();

    // 2. Compress.
    let compressed = deflate(&raw)?;

    // 3. Build the stream dictionary. A PDF metadata stream is conventionally
    //    /Type /Metadata /Subtype /XML. We mark the filter ourselves because
    //    we compressed the bytes ourselves.
    let mut stream_dict = Dictionary::new();
    stream_dict.set("Type", Object::Name(b"Metadata".to_vec()));
    stream_dict.set("Subtype", Object::Name(b"XML".to_vec()));
    stream_dict.set("Filter", Object::Name(b"FlateDecode".to_vec()));
    // /Length is the COMPRESSED length (the actual stream bytes on disk).
    stream_dict.set("Length", Object::Integer(compressed.len() as i64));

    // 4. Create the stream from the already-compressed bytes. We must stop
    //    lopdf from re-compressing or recomputing the filter — we set
    //    allows_compression(false) so it writes our bytes verbatim.
    let mut stream = Stream::new(stream_dict, compressed);
    stream.allows_compression = false;

    let metadata_id = doc.add_object(Object::Stream(stream));

    // 5. Reference it from the catalog (the document Root) under /Metadata.
    let catalog_id = doc
        .trailer
        .get(b"Root")
        .ok()
        .and_then(|o| o.as_reference().ok())
        .context("Document has no Root catalog to attach metadata to")?;

    let catalog = doc
        .get_object_mut(catalog_id)
        .context("Could not resolve Root catalog object")?;

    if let Object::Dictionary(dict) = catalog {
        dict.set("Metadata", Object::Reference(metadata_id));
    } else {
        bail!("Root catalog is not a dictionary");
    }

    println!(
        "  Manifest attached: {} bytes JSON+XMP -> {} bytes compressed",
        uncompressed_len,
        // re-read len for the message; cheap.
        doc.get_object(metadata_id)
            .ok()
            .and_then(|o| o.as_stream().ok())
            .map(|s| s.content.len())
            .unwrap_or(0),
    );

    Ok(())
}

// --- Read side -----------------------------------------------------------

/// Reads the manifest JSON back out of a binder document, if present.
/// Returns Ok(None) if the document has no paperclip metadata stream.
/// Used by rename detection.
pub fn read_manifest_json(doc: &Document) -> Result<Option<String>> {
    // Find the catalog and its /Metadata reference.
    let catalog_id = match doc
        .trailer
        .get(b"Root")
        .ok()
        .and_then(|o| o.as_reference().ok())
    {
        Some(id) => id,
        None => return Ok(None),
    };

    let metadata_ref = doc
        .get_object(catalog_id)
        .ok()
        .and_then(|o| o.as_dict().ok())
        .and_then(|d| d.get(b"Metadata").ok())
        .and_then(|o| o.as_reference().ok());

    let metadata_id = match metadata_ref {
        Some(id) => id,
        None => return Ok(None), // no metadata stream attached
    };

    let stream = match doc.get_object(metadata_id).ok().and_then(|o| o.as_stream().ok()) {
        Some(s) => s,
        None => return Ok(None),
    };

    // The stream content is our deflated XMP. Inflate, then unwrap the JSON.
    let xmp_bytes = inflate(&stream.content)?;
    let xmp = String::from_utf8(xmp_bytes)
        .context("XMP stream is not valid UTF-8")?;

    // If it's an XMP packet but not ours, unwrap_xmp returns Err — treat that
    // as "no paperclip manifest here" rather than a hard failure.
    match unwrap_xmp(&xmp) {
        Ok(json) => Ok(Some(json)),
        Err(_) => Ok(None),
    }
}
