//! The Word side of the integration: an Office.js task pane the server hosts
//! itself.
//!
//! Everything here is the *client* half of [`crate::integration`]. That module
//! decides what a document's citations should say; this one is the pane the
//! author clicks, and the glue that reads and writes Word's content controls.
//!
//! Three decisions worth knowing about:
//!
//! **The manifest is generated, not stored.** Office resolves nothing: every
//! URL in a manifest is absolute, so the file has to contain the host and port
//! this server is actually listening on. The port is configurable and the
//! install is per-machine, so a checked-in `manifest.xml` would be wrong for
//! anybody who changed it. [`manifest`] renders it from the request's `Host`.
//!
//! **The add-in id is stable across restarts.** Word keys a sideloaded add-in
//! by the GUID in its manifest; minting a fresh one each boot would leave the
//! author with a Ribbon full of duplicates that all do the same thing. The id
//! is generated once and kept in settings under `integration.addinId`.
//!
//! **The assets are embedded in the binary.** The pane has to work whether or
//! not the workbench was built, and it must not go through the SPA fallback,
//! which would answer `manifest.xml` with `index.html` — valid HTML, invalid
//! everything else, and a spectacularly unhelpful error inside Word.

#[cfg(test)]
mod tests;

use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;

use crate::state::App;

/// Where the add-in's id lives once minted.
const ID_KEY: &str = "integration.addinId";

const TASKPANE_HTML: &str = include_str!("assets/taskpane.html");
const TASKPANE_JS: &str = include_str!("assets/taskpane.js");
const TASKPANE_CSS: &str = include_str!("assets/taskpane.css");

pub fn router() -> Router<App> {
    Router::new()
        .route("/addin/manifest.xml", get(manifest))
        .route("/addin/taskpane.html", get(|| async { asset("text/html; charset=utf-8", TASKPANE_HTML) }))
        .route("/addin/taskpane.js", get(|| async { asset("text/javascript; charset=utf-8", TASKPANE_JS) }))
        .route("/addin/taskpane.css", get(|| async { asset("text/css; charset=utf-8", TASKPANE_CSS) }))
        .route("/addin/icon-:size", get(icon))
}

fn asset(content_type: &'static str, body: &'static str) -> Response {
    ([(header::CONTENT_TYPE, content_type)], body).into_response()
}

/// The Office add-in manifest, rendered for whatever origin the request
/// arrived on.
async fn manifest(State(app): State<App>, headers: HeaderMap) -> Response {
    let origin = origin_from(&headers, app.config().port);
    let id = addin_id(&app).await;
    (
        [
            (header::CONTENT_TYPE, "application/xml; charset=utf-8"),
            (header::CONTENT_DISPOSITION, "attachment; filename=\"yinkote-manifest.xml\""),
        ],
        render_manifest(&id, &origin),
    )
        .into_response()
}

/// Read the add-in id, minting and storing one on first use.
///
/// A failure to persist is not worth refusing the manifest over: the author
/// gets a working add-in and a different id next time, which is a far better
/// outcome than an error dialog inside Word.
async fn addin_id(app: &App) -> String {
    if let Ok(Some(value)) = app.store().settings.get(ID_KEY).await {
        if let Some(id) = value.as_str() {
            if uuid::Uuid::parse_str(id).is_ok() {
                return id.to_string();
            }
        }
    }
    let id = uuid::Uuid::new_v4().to_string();
    let _ = app.store().settings.set(ID_KEY, &serde_json::json!(id)).await;
    id
}

/// Which origin to write into the manifest.
///
/// The `Host` header is what the author's Word will actually be able to reach,
/// which is the point: a manifest fetched over `localhost` must say
/// `localhost`, because Office's rules for what may run without HTTPS are
/// written in terms of that exact name. Anything that could break out of the
/// surrounding XML is refused rather than escaped — a `Host` containing a
/// quote is an attack, not a hostname.
fn origin_from(headers: &HeaderMap, port: u16) -> String {
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .filter(|h| !h.is_empty() && h.len() < 256)
        .filter(|h| h.bytes().all(|b| b.is_ascii_alphanumeric() || b":.-_[]".contains(&b)))
        .unwrap_or("");
    if host.is_empty() {
        format!("http://127.0.0.1:{port}")
    } else {
        format!("http://{host}")
    }
}

/// The manifest itself.
///
/// A format string rather than an XML builder: it is written once against a
/// schema that will not move, and the literal is far easier to check against
/// Microsoft's documentation than a tree of builder calls would be.
fn render_manifest(id: &str, origin: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<OfficeApp xmlns="http://schemas.microsoft.com/office/appforoffice/1.1"
           xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
           xmlns:bt="http://schemas.microsoft.com/office/officeappbasictypes/1.0"
           xmlns:ov="http://schemas.microsoft.com/office/taskpaneappversionoverrides"
           xsi:type="TaskPaneApp">
  <Id>{id}</Id>
  <Version>1.0.0.0</Version>
  <ProviderName>Yinkote</ProviderName>
  <DefaultLocale>en-US</DefaultLocale>
  <DisplayName DefaultValue="Yinkote"/>
  <Description DefaultValue="Cite from your Yinkote library."/>
  <IconUrl DefaultValue="{origin}/addin/icon-32.png"/>
  <HighResolutionIconUrl DefaultValue="{origin}/addin/icon-80.png"/>
  <SupportUrl DefaultValue="{origin}/"/>
  <AppDomains>
    <AppDomain>{origin}</AppDomain>
  </AppDomains>
  <Hosts>
    <Host Name="Document"/>
  </Hosts>
  <Requirements>
    <Sets DefaultMinVersion="1.3">
      <Set Name="WordApi" MinVersion="1.3"/>
    </Sets>
  </Requirements>
  <DefaultSettings>
    <SourceLocation DefaultValue="{origin}/addin/taskpane.html"/>
  </DefaultSettings>
  <Permissions>ReadWriteDocument</Permissions>
  <VersionOverrides xmlns="http://schemas.microsoft.com/office/taskpaneappversionoverrides" xsi:type="VersionOverridesV1_0">
    <Hosts>
      <Host xsi:type="Document">
        <DesktopFormFactor>
          <GetStarted>
            <Title resid="GetStarted.Title"/>
            <Description resid="GetStarted.Description"/>
            <LearnMoreUrl resid="Url.Home"/>
          </GetStarted>
          <ExtensionPoint xsi:type="PrimaryCommandSurface">
            <OfficeTab id="TabHome">
              <Group id="Yinkote.Group">
                <Label resid="Group.Label"/>
                <Icon>
                  <bt:Image size="16" resid="Icon.16"/>
                  <bt:Image size="32" resid="Icon.32"/>
                  <bt:Image size="80" resid="Icon.80"/>
                </Icon>
                <Control xsi:type="Button" id="Yinkote.Open">
                  <Label resid="Open.Label"/>
                  <Supertip>
                    <Title resid="Open.Label"/>
                    <Description resid="Open.Tip"/>
                  </Supertip>
                  <Icon>
                    <bt:Image size="16" resid="Icon.16"/>
                    <bt:Image size="32" resid="Icon.32"/>
                    <bt:Image size="80" resid="Icon.80"/>
                  </Icon>
                  <Action xsi:type="ShowTaskpane">
                    <TaskpaneId>Yinkote.Taskpane</TaskpaneId>
                    <SourceLocation resid="Url.Taskpane"/>
                  </Action>
                </Control>
              </Group>
            </OfficeTab>
          </ExtensionPoint>
        </DesktopFormFactor>
      </Host>
    </Hosts>
    <Resources>
      <bt:Images>
        <bt:Image id="Icon.16" DefaultValue="{origin}/addin/icon-16.png"/>
        <bt:Image id="Icon.32" DefaultValue="{origin}/addin/icon-32.png"/>
        <bt:Image id="Icon.80" DefaultValue="{origin}/addin/icon-80.png"/>
      </bt:Images>
      <bt:Urls>
        <bt:Url id="Url.Taskpane" DefaultValue="{origin}/addin/taskpane.html"/>
        <bt:Url id="Url.Home" DefaultValue="{origin}/"/>
      </bt:Urls>
      <bt:ShortStrings>
        <bt:String id="Group.Label" DefaultValue="Yinkote"/>
        <bt:String id="Open.Label" DefaultValue="Citations"/>
        <bt:String id="GetStarted.Title" DefaultValue="Yinkote is ready."/>
      </bt:ShortStrings>
      <bt:LongStrings>
        <bt:String id="Open.Tip" DefaultValue="Insert and refresh citations from your library."/>
        <bt:String id="GetStarted.Description" DefaultValue="Open the Citations pane to cite from your library."/>
      </bt:LongStrings>
    </Resources>
  </VersionOverrides>
</OfficeApp>
"#
    )
}

/// A flat square in the product's ink, at whatever size the Ribbon asked for.
///
/// Generated rather than checked in: three PNGs of one colour are more bytes in
/// the repository than the encoder that makes them, and Office only ever asks
/// for 16, 32 and 80.
async fn icon(Path(name): Path<String>) -> Response {
    let size: u32 = match name.strip_suffix(".png").unwrap_or(&name).parse() {
        Ok(n) if (8..=256).contains(&n) => n,
        _ => return (StatusCode::NOT_FOUND, "no such icon").into_response(),
    };
    (
        [(header::CONTENT_TYPE, "image/png"), (header::CACHE_CONTROL, "public, max-age=86400")],
        solid_png(size, [0x1f, 0x6f, 0xeb]),
    )
        .into_response()
}

/// Encode a solid square as a PNG.
///
/// Hand-rolled because the alternative is an image crate in the dependency tree
/// for one flat colour. PNG allows uncompressed deflate blocks, so there is no
/// compressor here — just the framing.
fn solid_png(size: u32, rgb: [u8; 3]) -> Vec<u8> {
    // One row, repeated. Every row of a solid square is identical, and the
    // leading zero is PNG's per-row filter byte ("none").
    let mut row = Vec::with_capacity(size as usize * 3 + 1);
    row.push(0);
    for _ in 0..size {
        row.extend_from_slice(&rgb);
    }
    let mut raw = Vec::with_capacity(row.len() * size as usize);
    for _ in 0..size {
        raw.extend_from_slice(&row);
    }

    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&size.to_be_bytes());
    ihdr.extend_from_slice(&size.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]); // 8 bits per channel, truecolour

    let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &zlib_stored(&raw));
    chunk(&mut out, b"IEND", &[]);
    out
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    // The CRC covers the type and the body, but not the length.
    let mut crc = 0xffff_ffffu32;
    crc = crc32(crc, kind);
    crc = crc32(crc, body);
    out.extend_from_slice(&(crc ^ 0xffff_ffff).to_be_bytes());
}

/// A zlib stream made of uncompressed ("stored") deflate blocks.
fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    if data.is_empty() {
        // An empty payload still needs a final block, or the stream never ends.
        out.extend_from_slice(&[1, 0, 0, 0xff, 0xff]);
    }
    let mut parts = data.chunks(0xffff).peekable();
    while let Some(part) = parts.next() {
        out.push(u8::from(parts.peek().is_none()));
        out.extend_from_slice(&(part.len() as u16).to_le_bytes());
        out.extend_from_slice(&(!(part.len() as u16)).to_le_bytes());
        out.extend_from_slice(part);
    }
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in data {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

/// One step of CRC-32 over `data`, continuing from `crc`.
fn crc32(mut crc: u32, data: &[u8]) -> u32 {
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
        }
    }
    crc
}
