//! Tests for the parts of the add-in the server actually computes: the
//! manifest's origin, the id's stability, and the PNG encoder.
//!
//! The pane's JavaScript is exercised by `web`'s suite (`addin.dom.test.ts`),
//! which loads the same file this module embeds, and the id's stability across
//! requests is asserted end to end in `tests/api.rs` — it needs a real store.

use super::*;

fn headers(host: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    if !host.is_empty() {
        headers.insert(header::HOST, host.parse().unwrap());
    }
    headers
}

#[test]
fn origin_follows_the_host_the_request_arrived_on() {
    // Office's exemption from HTTPS is written in terms of the literal name
    // "localhost", so a manifest fetched over localhost has to say localhost
    // and not the loopback address it resolves to.
    assert_eq!(origin_from(&headers("localhost:23119"), 23119), "http://localhost:23119");
    assert_eq!(origin_from(&headers("127.0.0.1:9000"), 23119), "http://127.0.0.1:9000");
}

#[test]
fn origin_falls_back_to_the_configured_port() {
    assert_eq!(origin_from(&headers(""), 23119), "http://127.0.0.1:23119");
}

#[test]
fn a_host_that_could_break_out_of_the_xml_is_refused() {
    // Not escaped — refused. A Host header carrying a quote is an attack, and
    // the safe answer is the address we know we are listening on.
    for hostile in ["evil\"/><script>", "a b", "host/../x", "\u{4f60}\u{597d}"] {
        assert_eq!(origin_from(&headers(hostile), 23119), "http://127.0.0.1:23119", "{hostile}");
    }
}

#[test]
fn an_absurdly_long_host_is_refused() {
    let long = format!("{}:23119", "a".repeat(300));
    assert_eq!(origin_from(&headers(&long), 23119), "http://127.0.0.1:23119");
}

#[test]
fn the_manifest_points_every_url_at_the_same_origin() {
    let xml = render_manifest("11111111-2222-3333-4444-555555555555", "http://localhost:23119");
    assert!(xml.contains("<Id>11111111-2222-3333-4444-555555555555</Id>"));
    assert!(xml.contains("<SourceLocation DefaultValue=\"http://localhost:23119/addin/taskpane.html\"/>"));
    assert!(xml.contains("<AppDomain>http://localhost:23119</AppDomain>"));
    // Every URL in an Office manifest is absolute; a relative one silently
    // resolves against Microsoft's CDN.
    assert!(!xml.contains("DefaultValue=\"/addin"));
}

#[test]
fn the_manifest_declares_the_api_the_pane_uses() {
    let xml = render_manifest("id", "http://localhost:1");
    // The pane calls insertContentControl and content control insertHtml,
    // which are WordApi 1.1/1.3. Declaring too low a version gets it loaded
    // into a host where it cannot work.
    assert!(xml.contains("<Set Name=\"WordApi\" MinVersion=\"1.3\"/>"));
    assert!(xml.contains("<Permissions>ReadWriteDocument</Permissions>"));
}

#[test]
fn the_manifest_is_well_formed_xml() {
    // Word rejects the whole add-in on a single unbalanced tag, with an error
    // that names no line, so this is worth asserting cheaply.
    let xml = render_manifest("id", "http://localhost:1");
    let mut stack: Vec<&str> = Vec::new();
    let mut rest = xml.as_str();
    while let Some(open) = rest.find('<') {
        rest = &rest[open + 1..];
        let close = rest.find('>').expect("unterminated tag");
        let body = &rest[..close];
        rest = &rest[close + 1..];
        if body.starts_with('?') || body.starts_with('!') {
            continue;
        }
        if let Some(name) = body.strip_prefix('/') {
            assert_eq!(stack.pop(), Some(name.trim()), "closing tag does not match");
        } else if !body.ends_with('/') {
            stack.push(body.split([' ', '\n', '\t']).next().unwrap());
        }
    }
    assert!(stack.is_empty(), "unclosed: {stack:?}");
}

#[test]
fn the_icon_is_a_png_of_the_size_requested() {
    for size in [16u32, 32, 80] {
        let png = solid_png(size, [1, 2, 3]);
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
        // IHDR's width and height sit at a fixed offset: 8 signature + 8 chunk
        // header.
        assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), size);
        assert_eq!(u32::from_be_bytes(png[20..24].try_into().unwrap()), size);
        assert!(png.ends_with(&[b'I', b'E', b'N', b'D', 0xae, 0x42, 0x60, 0x82]));
    }
}

#[test]
fn the_png_chunk_crcs_are_right() {
    // A wrong CRC is the kind of thing every viewer tolerates and Word does
    // not, so check the encoder against the known CRC of an empty IEND.
    let mut out = Vec::new();
    chunk(&mut out, b"IEND", &[]);
    assert_eq!(out, vec![0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xae, 0x42, 0x60, 0x82]);
}

#[test]
fn the_stored_deflate_stream_round_trips() {
    // Two blocks' worth, to cover the chunking that a 256px icon would reach.
    let data: Vec<u8> = (0..70_000u32).map(|i| (i % 251) as u8).collect();
    let stream = zlib_stored(&data);
    assert_eq!(&stream[..2], &[0x78, 0x01]);
    assert_eq!(&stream[stream.len() - 4..], &adler32(&data).to_be_bytes());

    let mut back = Vec::new();
    let mut at = 2;
    loop {
        let last = stream[at] == 1;
        let len = u16::from_le_bytes([stream[at + 1], stream[at + 2]]) as usize;
        assert_eq!(
            u16::from_le_bytes([stream[at + 3], stream[at + 4]]),
            !(len as u16),
            "the length complement is what makes a stored block valid"
        );
        back.extend_from_slice(&stream[at + 5..at + 5 + len]);
        at += 5 + len;
        if last {
            break;
        }
    }
    assert_eq!(back, data);
    assert_eq!(at, stream.len() - 4);
}

#[test]
fn an_empty_stream_still_terminates() {
    let stream = zlib_stored(&[]);
    assert_eq!(stream[2] & 1, 1, "the only block must be marked final");
}
